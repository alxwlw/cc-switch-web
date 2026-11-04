#!/usr/bin/env bash
# Safe self-update of the systemd deployment of cc-switch-web.
#
#   scripts/self-update.sh <source_dir> <branch>
#
# Fast-forwards the checkout in <source_dir> to origin/<branch>, builds the web
# console and the server next to the live files, swaps them in, restarts the
# service and waits until it reports the new commit. Anything that goes wrong
# after the swap starts restores the previous binary and dist-web from the
# `.prev` copies, restarts again and checks that the old build answers.
#
# Progress is written to ${XDG_STATE_HOME:-~/.local/state}/cc-switch-web/update-status.json
# (atomically: temporary file + mv) as
#   {stage, ok, message, fromSha, toSha, startedAt, finishedAt}
# Stages: checking, fetching, merging, building, installing, restarting,
# rolling-back, then one of done / failed / rolled-back / rollback-failed.
# Those four are final (finishedAt is set); `failed` also covers a SIGTERM.
# `ok` is false for every final stage but `done`.
#
# Env (all optional):
#   BIN_DIR (~/.local/bin)    installed binary is $BIN_DIR/cc-switch-web
#   WEB_DIST (<source_dir>/dist-web)   absolute; .new and .prev live next to it
#   HOST, PORT (18766)        health check polls http://$HOST:$PORT/api/capabilities;
#                             HOST empty or 0.0.0.0 -> 127.0.0.1
#   CC_SWITCH_WEB_BUILD_PATH  directories with pnpm/node/cargo, put in front of PATH
#                             (a systemd-run unit has the manager's PATH, not the login one)
#   HEALTH_TIMEOUT (60)       seconds to wait for the restarted service
#   HEALTH_INTERVAL (1)       seconds between probes (a number, may be fractional)
#   FETCH_TIMEOUT (120)       seconds `git fetch origin` may take before it is killed
#                             (GNU timeout: bounds any transport, ssh included)
#
# Build logs go to stdout/stderr, i.e. the journal of the transient unit:
#   journalctl --user -u cc-switch-web-update
# The checkout is never reset: after a failure past the merge it stays on the new commit.

set -euo pipefail

SERVICE=cc-switch-web
JOURNAL_HINT="journalctl --user -u cc-switch-web-update"

STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/cc-switch-web"
STATUS_FILE="$STATE_DIR/update-status.json"
STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

STAGE=checking
FROM_SHA=""
TO_SHA=""
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
  printf '{"stage":%s,"ok":%s,"message":%s,"fromSha":%s,"toSha":%s,"startedAt":%s,"finishedAt":%s}\n' \
    "$(json_str "$stage")" "$ok" "$(json_str "$message")" \
    "$(json_str_or_null "$FROM_SHA")" "$(json_str_or_null "$TO_SHA")" \
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
  if [ "$TERMINAL" -eq 0 ]; then
    [ "$rc" -ne 0 ] || rc=1
    local why="${FAIL_MSG:-update interrupted (exit $rc) during stage $STAGE}"
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

# probe <wanted sha or "">: the service answers /api/capabilities and, when a sha is
# wanted, reports exactly that buildSha.
probe() {
  local want="$1" body got
  body="$(curl -fsS --max-time 3 "http://$HEALTH_HOST:$PORT/api/capabilities" 2>/dev/null)" || return 1
  [ -n "$want" ] || return 0
  got="$(printf '%s' "$body" | sed -n 's/.*"buildSha"[[:space:]]*:[[:space:]]*"\([0-9a-fA-F]*\)".*/\1/p' | head -n 1)"
  [ "$got" = "$want" ]
}

# running_sha: buildSha of the live service, empty when it is down or has no sha.
running_sha() {
  curl -fsS --max-time 3 "http://$HEALTH_HOST:$PORT/api/capabilities" 2>/dev/null |
    sed -n 's/.*"buildSha"[[:space:]]*:[[:space:]]*"\([0-9a-fA-F]*\)".*/\1/p' | head -n 1 || true
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
    wait_healthy "$RUNNING_SHA"; then
    write_status rolled-back false "$why. The previous binary and dist-web are back (copies kept as $BIN_PREV and $PREV_DIST) and the service answers.$(checkout_note) Log: $JOURNAL_HINT"
    return 0
  fi
  [ ! -e "$BIN_PREV" ] || left="$left $BIN_PREV"
  [ ! -d "$PREV_DIST" ] || left="$left $PREV_DIST"
  write_status rollback-failed false "$why. ROLLBACK FAILED: the service does not answer with the previous build. Previous files kept at:${left:- (none)}. Restore them by hand, then: systemctl --user restart $SERVICE.$(checkout_note) Log: $JOURNAL_HINT"
  return 1
}

# --- step 1: arguments, tools, checkout ----------------------------------------

write_status checking true "checking the checkout"

[ "$#" -eq 2 ] || die "usage: self-update.sh <source_dir> <branch>"
SRC_ARG="$1"
BRANCH="$2"

if [ -n "${CC_SWITCH_WEB_BUILD_PATH:-}" ]; then
  PATH="$CC_SWITCH_WEB_BUILD_PATH:$PATH"
  export PATH
fi

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

BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
BIN="$BIN_DIR/$SERVICE"
BIN_NEW="$BIN.new"
BIN_PREV="$BIN.prev"
BIN_RB="$BIN.rollback"
WEB_DIST="${WEB_DIST:-$SRC/dist-web}"
while [ "${#WEB_DIST}" -gt 1 ] && [ "${WEB_DIST%/}" != "$WEB_DIST" ]; do WEB_DIST="${WEB_DIST%/}"; done
NEW_DIST="$WEB_DIST.new"
PREV_DIST="$WEB_DIST.prev"

case "$WEB_DIST" in
  /*) ;;
  *) die "WEB_DIST must be an absolute path (got $WEB_DIST)" ;;
esac
case "$PORT" in
  '' | *[!0-9]*) die "PORT must be a number (got $PORT)" ;;
esac
case "$HEALTH_TIMEOUT" in
  '' | *[!0-9]*) die "HEALTH_TIMEOUT must be a whole number of seconds (got $HEALTH_TIMEOUT)" ;;
esac
case "$FETCH_TIMEOUT" in
  '' | 0 | *[!0-9]*) die "FETCH_TIMEOUT must be a positive whole number of seconds (got $FETCH_TIMEOUT)" ;;
esac
[[ "$HEALTH_INTERVAL" =~ ^[0-9]+(\.[0-9]+)?$ ]] || die "HEALTH_INTERVAL must be a number of seconds (got $HEALTH_INTERVAL)"
[ -f "$BIN" ] || die "no installed binary at $BIN; nothing to roll back to, run scripts/install-server.sh first"
[ -d "$WEB_DIST" ] || die "no dist-web at $WEB_DIST; nothing to roll back to, run scripts/install-server.sh first"

# What the service runs now, to confirm a rollback (the checkout may be ahead of it).
RUNNING_SHA="$(running_sha)"

# --- step 2: fetch, fast-forward ------------------------------------------------

write_status fetching true "fetching origin"
# git's own output goes to the unit log, never into the status file: the status is served
# unauthenticated and a remote URL may carry a token.
rc=0
timeout -k 5 "$FETCH_TIMEOUT" git -C "$SRC" fetch origin || rc=$?
[ "$rc" -ne 124 ] || die "git fetch origin timed out after ${FETCH_TIMEOUT}s"
[ "$rc" -eq 0 ] || die "git fetch origin failed (exit $rc)"
TO_SHA="$(git -C "$SRC" rev-parse --verify -q "refs/remotes/origin/$BRANCH^{commit}")" || die "origin/$BRANCH does not exist"

# Up to date only when the live service itself reports this commit: a service that is down
# or runs a build without a sha is rebuilt.
if [ "$FROM_SHA" = "$TO_SHA" ] && [ "$RUNNING_SHA" = "$TO_SHA" ]; then
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
rm -rf "$PREV_DIST"
rm -f "$BIN_PREV"
PHASE=swapping
mv "$BIN" "$BIN_PREV"
mv "$BIN_NEW" "$BIN"
mv "$WEB_DIST" "$PREV_DIST"
mv "$NEW_DIST" "$WEB_DIST"

write_status restarting true "restarting $SERVICE"
systemctl --user daemon-reload
RESTARTED=1
systemctl --user restart "$SERVICE"
wait_healthy "$TO_SHA" || die "the service did not report build $(short "$TO_SHA") within ${HEALTH_TIMEOUT}s"

PHASE=committed
TERMINAL=1
write_status "done" true "updated $(short "$FROM_SHA") -> $(short "$TO_SHA"). Previous binary and dist-web kept as $BIN_PREV and $PREV_DIST.$unit_note"
