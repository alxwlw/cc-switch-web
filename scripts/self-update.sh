#!/usr/bin/env bash
# Safe self-update of the systemd deployment of cc-switch-web.
#
#   scripts/self-update.sh <source_dir> <branch>      a build from source (checkout mode)
#   scripts/self-update.sh --release <repo>           an install made by install.sh (release mode)
#
# Checkout mode: fast-forwards the checkout in <source_dir> to origin/<branch>, builds the
# web console and the server next to the live files, swaps them in, restarts the service
# and waits until it reports the new commit. Anything that goes wrong after the swap starts
# restores the previous binary and dist-web from the `.prev` copies, restarts again and
# checks that the old build answers.
#
# Release mode: reads the latest release of <repo> (owner/name), and when it is newer than
# the running webVersion downloads the archive for this machine and SHA256SUMS, verifies the
# archive, unpacks it and swaps in the binary and dist-web the same way, restarts, waits for
# the new webVersion and rolls back the same way. Nothing is replaced before the archive is
# verified, complete and has its system libraries; after a successful update install.sh and
# self-update.sh of $CC_SWITCH_WEB_INSTALL_DIR are replaced by the ones of the archive (a
# temporary file + mv: this very script is running from that file).
#
# Progress is written to ${XDG_STATE_HOME:-~/.local/state}/cc-switch-web/update-status.json
# (atomically: temporary file + mv) as
#   {stage, ok, message, fromSha, toSha, startedAt, finishedAt}
# and, in release mode, with fromVersion and toVersion too (fromSha and toSha are then null).
# Stages (checkout): checking, fetching, merging, building, installing, restarting,
# rolling-back; (release): checking, downloading, verifying, installing, restarting,
# rolling-back; then one of done / failed / rolled-back / rollback-failed.
# Those four are final (finishedAt is set); `failed` also covers a SIGTERM.
# `ok` is false for every final stage but `done`.
#
# Env (all optional, except where noted):
#   BIN_DIR (~/.local/bin)    installed binary is $BIN_DIR/cc-switch-web
#   WEB_DIST (<source_dir>/dist-web, release mode: $CC_SWITCH_WEB_INSTALL_DIR/dist-web)
#                             absolute; .new and .prev live next to it
#   HOST, PORT (18766)        health check polls http://$HOST:$PORT/api/capabilities and /api/health;
#                             HOST empty or 0.0.0.0 -> 127.0.0.1
#   CC_SWITCH_WEB_BUILD_PATH  directories with pnpm/node/cargo, put in front of PATH
#                             (a systemd-run unit has the manager's PATH, not the login one)
#   HEALTH_TIMEOUT (60)       seconds to wait for the restarted service
#   HEALTH_INTERVAL (1)       seconds between probes (a number, may be fractional)
#   FETCH_TIMEOUT (120)       seconds `git fetch origin` may take before it is killed
#                             (GNU timeout: bounds any transport, ssh included)
# Release mode only:
#   CC_SWITCH_WEB_INSTALL_DIR (required)   the install dir of install.sh, absolute
#   CC_SWITCH_WEB_UPDATE_API (https://api.github.com)   GET $API/repos/<repo>/releases/latest
#   CC_SWITCH_WEB_DOWNLOAD_BASE (https://github.com/<repo>/releases/download)
#                             archive $BASE/<tag>/cc-switch-web-<version>-linux-<arch>.tar.gz
#                             and $BASE/<tag>/SHA256SUMS
#   DOWNLOAD_TIMEOUT (600)    seconds each request may take before curl gives up
#
# Build logs go to stdout/stderr, i.e. the journal of the transient unit:
#   journalctl --user -u cc-switch-web-update
# The checkout is never reset: after a failure past the merge it stays on the new commit.
# Release mode keeps its download in $CC_SWITCH_WEB_INSTALL_DIR/.update.* and removes it on exit.

set -euo pipefail

SERVICE=cc-switch-web
JOURNAL_HINT="journalctl --user -u cc-switch-web-update"

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/cc-switch-web"
STATUS_FILE="$STATE_DIR/update-status.json"
STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

MODE=checkout  # or release (--release <repo>)
STAGE=checking
FROM_SHA=""
TO_SHA=""
FROM_VERSION=""
TO_VERSION=""
WORK=""        # release mode: the download directory, removed on exit
NEW_DIST=""    # set by set_layout, with the rest of the layout
BIN_NEW=""
FAIL_MSG=""
MERGED=0       # the checkout moved to the new commit
PHASE=prepare  # prepare -> swapping (replacement started) -> committed (service verified)
RESTARTED=0
TERMINAL=0

# --- status file --------------------------------------------------------------

# json_str <text>: a JSON string literal. Printable ASCII, tab and newline only, so the
# file is valid UTF-8 whatever a tool printed.
json_str() {
  local s
  s="$(printf '%s' "$1" | LC_ALL=C tr -cd '\11\12\40-\176')"
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/\\n}
  s=${s//$'\t'/\\t}
  printf '"%s"' "$s"
}

json_str_or_null() {
  if [ -n "$1" ]; then json_str "$1"; else printf 'null'; fi
}

# write_status <stage> <ok: true|false> <message>
write_status() {
  local stage="$1" ok="$2" message="$3" finished="null" tmp
  case "$stage" in
    done | failed | rolled-back | rollback-failed)
      finished="$(json_str "$(date -u +%Y-%m-%dT%H:%M:%SZ)")"
      ;;
  esac
  STAGE="$stage"
  mkdir -p "$STATE_DIR"
  tmp="$STATUS_FILE.tmp.$$"
  local versions=""
  if [ "$MODE" = release ]; then
    versions="$(printf '"fromVersion":%s,"toVersion":%s,' \
      "$(json_str_or_null "$FROM_VERSION")" "$(json_str_or_null "$TO_VERSION")")"
  fi
  printf '{"stage":%s,"ok":%s,"message":%s,"fromSha":%s,"toSha":%s,%s"startedAt":%s,"finishedAt":%s}\n' \
    "$(json_str "$stage")" "$ok" "$(json_str "$message")" \
    "$(json_str_or_null "$FROM_SHA")" "$(json_str_or_null "$TO_SHA")" "$versions" \
    "$(json_str "$STARTED_AT")" "$finished" >"$tmp"
  mv -f "$tmp" "$STATUS_FILE"
}

# --- failure handling ---------------------------------------------------------

# die <message>: the EXIT trap turns this into the final status.
die() {
  FAIL_MSG="$*"
  exit 1
}

# run <label> <command...>: a failing command ends the update naming itself.
run() {
  local label="$1" rc=0
  shift
  "$@" || rc=$?
  [ "$rc" -eq 0 ] || die "$label failed (exit $rc)"
}

# run_in <dir> <label> <command...>
run_in() {
  local dir="$1" label="$2" rc=0
  shift 2
  (cd "$dir" && "$@") || rc=$?
  [ "$rc" -eq 0 ] || die "$label failed (exit $rc)"
}

short() { printf '%.7s' "$1"; }

checkout_note() {
  if [ "$MERGED" -eq 1 ]; then
    printf ' The checkout stays on %s (no git reset was run).' "$(short "$TO_SHA")"
  fi
}

on_exit() {
  local rc=$?
  trap - EXIT
  set +e
  if [ -n "$WORK" ]; then rm -rf "$WORK"; fi
  if [ "$TERMINAL" -eq 0 ]; then
    [ "$rc" -ne 0 ] || rc=1
    local why="${FAIL_MSG:-update interrupted (exit $rc) during stage $STAGE}"
    if [ "$PHASE" = prepare ] && [ "$MODE" = release ] && [ -n "$NEW_DIST" ]; then
      rm -rf "$NEW_DIST" "$BIN_NEW"
    fi
    if [ "$PHASE" = swapping ]; then
      rollback "$why"
    else
      write_status failed false "$why.$(checkout_note) Log: $JOURNAL_HINT" || true
    fi
  fi
  exit "$rc"
}
trap on_exit EXIT
trap 'exit 143' TERM
trap 'exit 130' INT
trap 'exit 129' HUP

# --- health check -------------------------------------------------------------

PORT="${PORT:-18766}"
HEALTH_TIMEOUT="${HEALTH_TIMEOUT:-60}"
HEALTH_INTERVAL="${HEALTH_INTERVAL:-1}"
FETCH_TIMEOUT="${FETCH_TIMEOUT:-120}"
HEALTH_HOST="${HOST:-}"
case "$HEALTH_HOST" in
  '' | 0.0.0.0 | :: | '[::]') HEALTH_HOST=127.0.0.1 ;;
  \[*) ;;
  *:*) HEALTH_HOST="[$HEALTH_HOST]" ;;
esac

# What identifies a build in /api/capabilities: buildSha (checkout mode, a hex sha) or
# webVersion (release mode, 4.0.35-web.1).
ID_FIELD=buildSha
ID_CHARS='0-9a-fA-F'

# id_of <capabilities body>: the build id in it, empty when there is none.
id_of() {
  printf '%s' "$1" | sed -n "s/.*\"$ID_FIELD\"[[:space:]]*:[[:space:]]*\"\([$ID_CHARS]*\)\".*/\1/p" | head -n 1
}

# probe <wanted id or "">: the service is up and working: /api/capabilities answers and,
# when an id is wanted, reports exactly that one, and /api/health answers 2xx. Both are
# needed: a server whose startup failed (a database newer than the binary) runs in init-error
# mode, where /api/capabilities still answers with the build's own id but /api/health, like
# every other /api route, is 503.
probe() {
  local want="$1" body got
  body="$(curl -fsS --max-time 3 "http://$HEALTH_HOST:$PORT/api/capabilities" 2>/dev/null)" || return 1
  if [ -n "$want" ]; then
    got="$(id_of "$body")"
    [ "$got" = "$want" ] || return 1
  fi
  curl -fsS --max-time 3 -o /dev/null "http://$HEALTH_HOST:$PORT/api/health" 2>/dev/null
}

# running_id: the build id of the live service, empty when it is down or has none.
running_id() {
  local body
  body="$(curl -fsS --max-time 3 "http://$HEALTH_HOST:$PORT/api/capabilities" 2>/dev/null)" || return 0
  id_of "$body"
}

wait_healthy() {
  local want="$1" deadline=$((SECONDS + HEALTH_TIMEOUT))
  while :; do
    if probe "$want"; then return 0; fi
    [ "$SECONDS" -lt "$deadline" ] || return 1
    sleep "$HEALTH_INTERVAL"
  done
}

# --- rollback -----------------------------------------------------------------

# restore_dir / restore_bin copy from .prev and keep it, so an operator still has
# the previous build when the service does not come up after the rollback either.
rollback() {
  local why="$1" ok=1 left=""
  set +e
  TERMINAL=1
  write_status rolling-back true "$why; restoring the previous build"
  if [ -d "$PREV_DIST" ]; then
    rm -rf "$NEW_DIST"
    if [ -e "$WEB_DIST" ]; then mv "$WEB_DIST" "$NEW_DIST" || ok=0; fi
    cp -a "$PREV_DIST" "$WEB_DIST" || ok=0
  fi
  if [ -e "$BIN_PREV" ]; then
    cp -p "$BIN_PREV" "$BIN_RB" && mv -f "$BIN_RB" "$BIN" || ok=0
  fi
  if [ "$ok" -eq 1 ] && { [ "$RESTARTED" -eq 0 ] || systemctl --user restart "$SERVICE"; } &&
    wait_healthy "$RUNNING_ID"; then
    write_status rolled-back false "$why. The previous binary and dist-web are back (copies kept as $BIN_PREV and $PREV_DIST) and the service works.$(checkout_note) Log: $JOURNAL_HINT"
    return 0
  fi
  [ ! -e "$BIN_PREV" ] || left="$left $BIN_PREV"
  [ ! -d "$PREV_DIST" ] || left="$left $PREV_DIST"
  write_status rollback-failed false "$why. ROLLBACK FAILED: the service does not work with the previous build. One possible cause is a database that the new build migrated and the previous binary cannot open: see docs/deploy.md, section \"The database and an update\", to restore db_backup_*. Previous files kept at:${left:- (none)}. Restore them by hand, then: systemctl --user restart $SERVICE.$(checkout_note) Log: $JOURNAL_HINT"
  return 1
}

# --- layout, settings, swap (both modes) ---------------------------------------------

# set_layout <default WEB_DIST>: the installed files and their neighbours, from the env.
set_layout() {
  BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
  BIN="$BIN_DIR/$SERVICE"
  BIN_NEW="$BIN.new"
  BIN_PREV="$BIN.prev"
  BIN_RB="$BIN.rollback"
  WEB_DIST="${WEB_DIST:-$1}"
  while [ "${#WEB_DIST}" -gt 1 ] && [ "${WEB_DIST%/}" != "$WEB_DIST" ]; do WEB_DIST="${WEB_DIST%/}"; done
  NEW_DIST="$WEB_DIST.new"
  PREV_DIST="$WEB_DIST.prev"
  case "$WEB_DIST" in
    /*) ;;
    *) die "WEB_DIST must be an absolute path (got $WEB_DIST)" ;;
  esac
}

validate_settings() {
  case "$PORT" in
    '' | *[!0-9]*) die "PORT must be a number (got $PORT)" ;;
  esac
  case "$HEALTH_TIMEOUT" in
    # a leading zero would be read as octal by the deadline arithmetic ($((SECONDS + 08)) is an error)
    '' | *[!0-9]* | 0[0-9]*) die "HEALTH_TIMEOUT must be a whole number of seconds without a leading zero (got $HEALTH_TIMEOUT)" ;;
  esac
  [[ "$HEALTH_INTERVAL" =~ ^[0-9]+(\.[0-9]+)?$ ]] || die "HEALTH_INTERVAL must be a number of seconds (got $HEALTH_INTERVAL)"
}

# swap_in: .prev copies of the live binary and dist-web, then the new ones ($BIN_NEW and
# $NEW_DIST, built or unpacked next to the live files) take their place. A rename never
# writes over the running binary. From here on a failure rolls back.
swap_in() {
  rm -rf "$PREV_DIST"
  rm -f "$BIN_PREV"
  PHASE=swapping
  mv "$BIN" "$BIN_PREV"
  mv "$BIN_NEW" "$BIN"
  mv "$WEB_DIST" "$PREV_DIST"
  mv "$NEW_DIST" "$WEB_DIST"
}

# --- release mode ---------------------------------------------------------------------

TAG_RE='^v[0-9]+\.[0-9]+\.[0-9]+-web\.[0-9]+$'
VERSION_RE='^[0-9]+\.[0-9]+\.[0-9]+-web\.[0-9]+$'
REPO_RE='^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$'

# version_newer <a> <b>: a is a strictly newer fork version than b. Both must look like
# 4.0.35-web.1; numbers are compared as numbers (web.10 is newer than web.9), digit by digit
# of the stripped string, so no number can overflow. Anything unparseable is "not newer".
version_newer() {
  local a="$1" b="$2" i x y
  [[ "$a" =~ $VERSION_RE ]] && [[ "$b" =~ $VERSION_RE ]] || return 1
  local -a pa pb
  IFS=. read -r -a pa <<<"${a/-web./.}"
  IFS=. read -r -a pb <<<"${b/-web./.}"
  for i in 0 1 2 3; do
    x="${pa[$i]}"
    y="${pb[$i]}"
    x="${x#"${x%%[!0]*}"}"
    y="${y#"${y%%[!0]*}"}"
    if [ "${#x}" -gt "${#y}" ]; then return 0; fi
    if [ "${#x}" -lt "${#y}" ]; then return 1; fi
    if [[ "$x" > "$y" ]]; then return 0; fi
    if [[ "$x" < "$y" ]]; then return 1; fi
  done
  return 1
}

# The ldd lines that say "not found" for <binary>; an ldd that fails prints nothing (as install.sh).
missing_libs() {
  local out
  out="$(ldd "$1" 2>/dev/null)" || return 0
  printf '%s\n' "$out" | grep 'not found' || true
}

# fetch <url> [output file]: curl that fails on an HTTP error and cannot hang for good.
fetch() {
  local url="$1" out="${2:-}"
  if [ -n "$out" ]; then
    curl -fsSL --connect-timeout 20 --max-time "$DOWNLOAD_TIMEOUT" -o "$out" "$url"
  else
    curl -fsSL --connect-timeout 20 --max-time "$DOWNLOAD_TIMEOUT" "$url"
  fi
}

# refresh_scripts: install.sh and self-update.sh of the install dir become the ones of the
# archive. Temporary file + mv: this script is running from one of them, and a write in place
# would change what bash reads next.
refresh_scripts() {
  local f
  for f in install.sh self-update.sh; do
    cp "$EX/$f" "$INSTALL_DIR/.$f.new" || return 1
    chmod 755 "$INSTALL_DIR/.$f.new" || return 1
    mv -f "$INSTALL_DIR/.$f.new" "$INSTALL_DIR/$f" || return 1
  done
}

# update_release <owner/name>
update_release() {
  local repo="$1" t missing="" api base tag latest_body m arch name want got listing libs f scripts_note=""
  ID_FIELD=webVersion
  ID_CHARS='0-9A-Za-z.+-'
  [[ "$repo" =~ $REPO_RE ]] || die "$repo is not a usable owner/name"
  for t in curl tar sha256sum systemctl; do
    command -v "$t" >/dev/null 2>&1 || missing="$missing $t"
  done
  [ -z "$missing" ] || die "missing tools:$missing"

  INSTALL_DIR="${CC_SWITCH_WEB_INSTALL_DIR:-}"
  [ -n "$INSTALL_DIR" ] || die "CC_SWITCH_WEB_INSTALL_DIR is not set"
  case "$INSTALL_DIR" in
    /*) ;;
    *) die "CC_SWITCH_WEB_INSTALL_DIR must be an absolute path (got $INSTALL_DIR)" ;;
  esac
  [ -d "$INSTALL_DIR" ] || die "CC_SWITCH_WEB_INSTALL_DIR $INSTALL_DIR is not a directory"
  set_layout "$INSTALL_DIR/dist-web"
  validate_settings
  DOWNLOAD_TIMEOUT="${DOWNLOAD_TIMEOUT:-600}"
  case "$DOWNLOAD_TIMEOUT" in
    '' | 0 | *[!0-9]*) die "DOWNLOAD_TIMEOUT must be a positive whole number of seconds (got $DOWNLOAD_TIMEOUT)" ;;
  esac
  [ -f "$BIN" ] || die "no installed binary at $BIN; nothing to roll back to, run install.sh first"
  [ -d "$WEB_DIST" ] || die "no dist-web at $WEB_DIST; nothing to roll back to, run install.sh first"

  # What the service runs now: the version to compare with, and what a rollback confirms.
  FROM_VERSION="$(running_id)"
  RUNNING_ID="$FROM_VERSION"
  [ -n "$FROM_VERSION" ] || die "the service does not report its version on http://$HEALTH_HOST:$PORT/api/capabilities; is it running?"

  api="${CC_SWITCH_WEB_UPDATE_API:-https://api.github.com}"
  api="${api%/}"
  base="${CC_SWITCH_WEB_DOWNLOAD_BASE:-https://github.com/$repo/releases/download}"
  base="${base%/}"
  latest_body="$(fetch "$api/repos/$repo/releases/latest")" || die "could not read the latest release of $repo from $api"
  tag="$(printf '%s\n' "$latest_body" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/;T;p;q')"
  [ -n "$tag" ] || die "the latest release of $repo has no tag"
  [[ "$tag" =~ $TAG_RE ]] || die "the latest release tag '$tag' is not a cc-switch-web release (expected v<x.y.z>-web.<n>)"
  TO_VERSION="${tag#v}"
  [[ "$FROM_VERSION" =~ $VERSION_RE ]] || die "the running version '$FROM_VERSION' is not a cc-switch-web release version, so it cannot be compared with $TO_VERSION"

  if [ "$TO_VERSION" = "$FROM_VERSION" ]; then
    TERMINAL=1
    write_status "done" true "already up to date at $FROM_VERSION"
    exit 0
  fi
  version_newer "$TO_VERSION" "$FROM_VERSION" || die "the latest release $TO_VERSION is not newer than $FROM_VERSION"

  # --- downloading
  write_status downloading true "downloading $TO_VERSION"
  rm -rf "$INSTALL_DIR"/.update.*
  WORK="$(mktemp -d "$INSTALL_DIR/.update.XXXXXX")" || die "cannot create a download directory in $INSTALL_DIR"
  m="$(uname -m)"
  case "$m" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "unsupported architecture $m (release archives exist for x86_64 and aarch64 only)" ;;
  esac
  name="cc-switch-web-$TO_VERSION-linux-$arch"
  fetch "$base/$tag/SHA256SUMS" "$WORK/SHA256SUMS" || die "could not download $base/$tag/SHA256SUMS"
  want="$(awk -v n="$name.tar.gz" '$2 == n || $2 == "*" n { print $1; exit }' "$WORK/SHA256SUMS")"
  [ -n "$want" ] || die "no archive for $arch ($name.tar.gz) in $tag; nothing was replaced"
  fetch "$base/$tag/$name.tar.gz" "$WORK/$name.tar.gz" || die "could not download $base/$tag/$name.tar.gz"

  # --- verifying
  write_status verifying true "verifying $name.tar.gz"
  got="$(sha256sum "$WORK/$name.tar.gz" | awk '{ print $1 }')"
  [ "$got" = "$want" ] || die "checksum mismatch for $name.tar.gz (expected $want, got $got); nothing was replaced"

  # --- installing: unpack, check, then swap
  write_status installing true "unpacking $name.tar.gz"
  listing="$(tar -tzf "$WORK/$name.tar.gz")" || die "could not read $name.tar.gz; nothing was replaced"
  if grep -qE '^/|(^|/)\.\.(/|$)' <<<"$listing"; then
    die "the archive has unsafe paths; nothing was replaced"
  fi
  mkdir "$WORK/x"
  tar -xzf "$WORK/$name.tar.gz" -C "$WORK/x" || die "could not unpack $name.tar.gz; nothing was replaced"
  EX="$WORK/x/$name"
  for f in cc-switch-web install.sh self-update.sh; do
    [ -f "$EX/$f" ] || die "the archive is incomplete (no $f); nothing was replaced"
  done
  [ -f "$EX/dist-web/index.html" ] || die "the archive is incomplete (no dist-web/index.html); nothing was replaced"
  libs="$(missing_libs "$EX/cc-switch-web")"
  if [ -n "$libs" ]; then
    libs="$(printf '%s\n' "$libs" | sed 's/^[[:space:]]*//' | tr '\n' ',' | sed 's/,$//')"
    die "system libraries are missing for $TO_VERSION: $libs. Nothing was replaced. Run install.sh to install them: curl -fsSL https://github.com/$repo/releases/latest/download/install.sh | bash"
  fi
  rm -rf "$NEW_DIST"
  rm -f "$BIN_NEW"
  run "copying dist-web" cp -R "$EX/dist-web" "$NEW_DIST"
  run "copying the binary" cp "$EX/cc-switch-web" "$BIN_NEW"
  run "chmod" chmod 755 "$BIN_NEW"
  swap_in

  # --- restarting
  write_status restarting true "restarting $SERVICE"
  systemctl --user daemon-reload
  RESTARTED=1
  systemctl --user restart "$SERVICE"
  wait_healthy "$TO_VERSION" || die "the service did not report version $TO_VERSION with a working /api/health within ${HEALTH_TIMEOUT}s"

  PHASE=committed
  refresh_scripts || scripts_note=" WARNING: install.sh and self-update.sh in $INSTALL_DIR could not be refreshed; run the installer again to refresh them."
  TERMINAL=1
  write_status "done" true "updated $FROM_VERSION -> $TO_VERSION. Previous binary and dist-web kept as $BIN_PREV and $PREV_DIST.$scripts_note"
}

# --- step 1: arguments, tools, checkout ----------------------------------------

if [ "$#" -eq 2 ] && [ "$1" = --release ]; then MODE=release; fi

if [ "$MODE" = release ]; then
  write_status checking true "checking the installed release"
else
  write_status checking true "checking the checkout"
fi

if [ -n "${CC_SWITCH_WEB_BUILD_PATH:-}" ]; then
  PATH="$CC_SWITCH_WEB_BUILD_PATH:$PATH"
  export PATH
fi

if [ "$MODE" = release ]; then
  update_release "$2"
  exit 0
fi

[ "$#" -eq 2 ] || die "usage: self-update.sh <source_dir> <branch> | --release <repo>"
SRC_ARG="$1"
BRANCH="$2"

missing=""
for tool in git pnpm node cargo curl systemctl timeout; do
  command -v "$tool" >/dev/null 2>&1 || missing="$missing $tool"
done
[ -z "$missing" ] || die "missing build tools:$missing (set CC_SWITCH_WEB_BUILD_PATH to the directories that hold them)"

export GIT_TERMINAL_PROMPT=0

SRC="$(cd "$SRC_ARG" 2>/dev/null && pwd -P)" || die "source dir $SRC_ARG does not exist"
top="$(git -C "$SRC" rev-parse --show-toplevel 2>/dev/null)" || die "$SRC is not a git checkout"
[ "$top" = "$SRC" ] || die "$SRC is not the root of a git checkout"

git check-ref-format --branch "$BRANCH" >/dev/null 2>&1 || die "invalid branch name $BRANCH"
current="$(git -C "$SRC" symbolic-ref --short -q HEAD)" || current="(detached HEAD)"
[ "$current" = "$BRANCH" ] || die "checkout $SRC is on $current, not on $BRANCH"

# tracked changes only, the same rule as resolve_build_sha: an untracked .scratch/ is fine
FROM_SHA="$(git -C "$SRC" rev-parse HEAD)"
git -C "$SRC" diff --quiet HEAD 2>/dev/null || die "tracked files in $SRC differ from HEAD; commit or revert them first"

set_layout "$SRC/dist-web"
validate_settings
case "$FETCH_TIMEOUT" in
  '' | 0 | *[!0-9]*) die "FETCH_TIMEOUT must be a positive whole number of seconds (got $FETCH_TIMEOUT)" ;;
esac
[ -f "$BIN" ] || die "no installed binary at $BIN; nothing to roll back to, run scripts/install-server.sh first"
[ -d "$WEB_DIST" ] || die "no dist-web at $WEB_DIST; nothing to roll back to, run scripts/install-server.sh first"

# What the service runs now, to confirm a rollback (the checkout may be ahead of it).
RUNNING_ID="$(running_id)"

# --- step 2: fetch, fast-forward ------------------------------------------------

write_status fetching true "fetching origin"
# git's own output goes to the unit log, never into the status file: the status is served
# unauthenticated and a remote URL may carry a token.
rc=0
timeout -k 5 "$FETCH_TIMEOUT" git -C "$SRC" fetch origin || rc=$?
[ "$rc" -ne 124 ] || die "git fetch origin timed out after ${FETCH_TIMEOUT}s"
[ "$rc" -eq 0 ] || die "git fetch origin failed (exit $rc)"
TO_SHA="$(git -C "$SRC" rev-parse --verify -q "refs/remotes/origin/$BRANCH^{commit}")" || die "origin/$BRANCH does not exist"

# Up to date only when the live service itself reports this commit and works: a service that
# is down, in init-error mode, or runs a build without a sha is rebuilt.
if [ "$FROM_SHA" = "$TO_SHA" ] && [ "$RUNNING_ID" = "$TO_SHA" ] && probe "$TO_SHA"; then
  TERMINAL=1
  write_status "done" true "already up to date at $(short "$TO_SHA")"
  exit 0
fi

write_status merging true "fast-forwarding to origin/$BRANCH"
git -C "$SRC" merge --ff-only "origin/$BRANCH" || die "cannot fast-forward $BRANCH to origin/$BRANCH"
MERGED=1
[ "$(git -C "$SRC" rev-parse HEAD)" = "$TO_SHA" ] || die "checkout is not on $(short "$TO_SHA") after the merge"

unit_note=""
if git -C "$SRC" diff --quiet "$FROM_SHA" "$TO_SHA" -- scripts/cc-switch-web.service.template 2>/dev/null; then
  :
else
  unit_note=" WARNING: scripts/cc-switch-web.service.template changed in this update; run scripts/install-server.sh by hand to re-render the unit."
fi

# --- step 3: build next to the live files ---------------------------------------

write_status building true "building $(short "$TO_SHA")"
export CC_SWITCH_WEB_GIT_SHA="$TO_SHA"
rm -rf "$NEW_DIST"
rm -f "$BIN_NEW"
run_in "$SRC" "pnpm install --frozen-lockfile" pnpm install --frozen-lockfile
run_in "$SRC" "pnpm build:web" pnpm build:web --outDir "$NEW_DIST"
[ -f "$NEW_DIST/index.html" ] || die "pnpm build:web produced no $NEW_DIST/index.html"
run_in "$SRC/src-tauri" "cargo build --release --example server" cargo build --release --example server
run "install" install -m0755 "$SRC/src-tauri/target/release/examples/server" "$BIN_NEW"

# --- step 4: swap, restart, verify ------------------------------------------------

write_status installing true "replacing the binary and dist-web"
swap_in

write_status restarting true "restarting $SERVICE"
systemctl --user daemon-reload
RESTARTED=1
systemctl --user restart "$SERVICE"
wait_healthy "$TO_SHA" || die "the service did not report build $(short "$TO_SHA") with a working /api/health within ${HEALTH_TIMEOUT}s"

PHASE=committed
TERMINAL=1
write_status "done" true "updated $(short "$FROM_SHA") -> $(short "$TO_SHA"). Previous binary and dist-web kept as $BIN_PREV and $PREV_DIST.$unit_note"
