#!/usr/bin/env bash
# Tests scripts/self-update.sh against real temporary git repositories (a bare origin
# and a clone) with stub cargo/pnpm/node/systemctl/curl in PATH. The stubs record their
# calls and the status stage at call time in a log, and play a service whose reported
# buildSha follows the binary installed at the last restart.
# Run: bash scripts/tests/self-update.test.sh   (local gate; CI does not run it)
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../self-update.sh"

TMP="$(mktemp -d)"
LISTENER=""
cleanup() {
  if [ -n "$LISTENER" ]; then kill "$LISTENER" 2>/dev/null; fi
  rm -rf "$TMP"
}
trap cleanup EXIT
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
export GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t

command -v python3 >/dev/null || {
  echo "python3 is needed to parse the status file"
  exit 2
}

fails=0
passes=0

check() { # check <name> <got> <want>
  if [ "$2" = "$3" ]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: got '$2', want '$3'"
    fails=$((fails + 1))
  fi
}

check_has() { # check_has <name> <haystack> <needle>
  if [[ "$2" == *"$3"* ]]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: '$2' lacks '$3'"
    fails=$((fails + 1))
  fi
}

check_lacks() { # check_lacks <name> <haystack> <needle>
  if [[ "$2" != *"$3"* ]]; then
    echo "ok   $1"
    passes=$((passes + 1))
  else
    echo "FAIL $1: '$2' has '$3'"
    fails=$((fails + 1))
  fi
}

# --- stubs --------------------------------------------------------------------------

STUBS="$TMP/stubs"
mkdir -p "$STUBS"

# shellcheck disable=SC2016 # expanded by the stubs, not here
common='
log() { printf "%s\n" "$*" >>"$STUB_LOG"; }
stage() { sed -n "s/.*\"stage\":\"\([a-z-]*\)\".*/\1/p" "$STATUS_FILE" 2>/dev/null; }
'

cat >"$STUBS/node" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF

cat >"$STUBS/pnpm" <<EOF
#!/usr/bin/env bash
$common
log "pnpm \$* cwd=\$PWD stage=\$(stage)"
ln -f "\$STATUS_FILE" "\$STATUS_FILE.snap"
case "\$1" in
  install) exit 0 ;;
  build:web)
    if [ -n "\${STUB_FAIL_PNPM_BUILD:-}" ]; then echo "build exploded" >&2; exit 3; fi
    out=""
    while [ \$# -gt 0 ]; do
      if [ "\$1" = --outDir ]; then out="\$2"; fi
      shift
    done
    if [ -z "\$out" ]; then exit 4; fi
    mkdir -p "\$out"
    if [ -z "\${STUB_NO_INDEX:-}" ]; then echo "web-\$(git rev-parse HEAD)" >"\$out/index.html"; fi
    if [ -n "\${STUB_BLOCK_BUILD:-}" ]; then echo go >"\$STUB_BLOCK_BUILD"; sleep 30; fi
    exit 0
    ;;
esac
exit 5
EOF

cat >"$STUBS/cargo" <<EOF
#!/usr/bin/env bash
$common
log "cargo \$* cwd=\$PWD sha=\${CC_SWITCH_WEB_GIT_SHA:-} stage=\$(stage)"
if [ -n "\${STUB_FAIL_CARGO:-}" ]; then echo "cargo exploded" >&2; exit 101; fi
mkdir -p target/release/examples
echo "\${STUB_BIN:-bin-\$(git rev-parse HEAD)}" >target/release/examples/server
exit 0
EOF

cat >"$STUBS/systemctl" <<EOF
#!/usr/bin/env bash
$common
log "systemctl \$* stage=\$(stage)"
if [ "\$STATUS_FILE" -ef "\$STATUS_FILE.snap" ]; then log snap-same; else log snap-replaced; fi
if [ "\$1 \$2" = "--user restart" ]; then
  if [ -n "\${STUB_BLOCK_RESTART:-}" ] && [ ! -e "\$STUB_SVC.blocked" ]; then
    : >"\$STUB_SVC.blocked"
    echo go >"\$STUB_BLOCK_RESTART"
    sleep 30
  fi
  content="\$(cat "\$BIN_DIR/cc-switch-web")"
  if [ -e "\$STUB_SVC.always-down" ] || [ "\$content" = bin-down ]; then
    echo down >"\$STUB_SVC"
  else
    echo "\${content#bin-}" >"\$STUB_SVC"
  fi
  if [ -n "\${STUB_WARMUP:-}" ]; then echo "\$STUB_WARMUP" >"\$STUB_SVC.warm"; fi
fi
exit 0
EOF

cat >"$STUBS/curl" <<EOF
#!/usr/bin/env bash
$common
for last; do :; done
log "curl \$last"
if [ -e "\$STUB_SVC.warm" ] && [ "\$(cat "\$STUB_SVC.warm")" -gt 0 ]; then
  echo \$(( \$(cat "\$STUB_SVC.warm") - 1 )) >"\$STUB_SVC.warm"
  exit 7
fi
state="\$(cat "\$STUB_SVC")"
if [ "\$state" = down ]; then exit 7; fi
if [ "\$state" = none ]; then
  echo '{"buildSha":null}'
else
  echo "{\"web\":true, \"buildSha\" : \"\$state\"}"
fi
EOF
chmod +x "$STUBS"/*

# Only these real tools are visible to the script, next to the stubs: a missing build
# tool is then really missing whatever the host has installed.
BASE_TOOLS="timeout bash env git sh cat mkdir mv rm date tr sed head tail install sleep dirname chmod cp touch grep basename mktemp wc sort ln readlink"

# --- repositories -------------------------------------------------------------------

commit_in() { # commit_in <dir> <file> <text>
  mkdir -p "$(dirname "$1/$2")"
  printf '%s\n' "$3" >"$1/$2"
  git -C "$1" add "$2"
  git -C "$1" commit -q -m "$3"
}

# setup_case <name> [src dir name]: origin at A in SRC's clone, B pushed to origin from a
# second clone (B_FILE/B_TEXT choose what B changes), a "deployed" binary and dist-web
# built from A and a service reporting A. Sets C SRC A B and the paths below.
setup_case() {
  C="$TMP/$1"
  SRC="$C/${2:-src}"
  ORIGIN="$C/origin.git"
  PUSHER="$C/pusher"
  mkdir -p "$C/home" "$SRC"
  git init -q --bare -b main "$ORIGIN"
  git -C "$SRC" init -q -b main
  git -C "$SRC" remote add origin "$ORIGIN"
  commit_in "$SRC" f one
  commit_in "$SRC" scripts/cc-switch-web.service.template unit-one
  commit_in "$SRC" src-tauri/Cargo.toml manifest
  git -C "$SRC" push -q origin main
  A="$(git -C "$SRC" rev-parse HEAD)"
  git clone -q "$ORIGIN" "$PUSHER" 2>/dev/null
  commit_in "$PUSHER" "${B_FILE:-f}" "${B_TEXT:-two}"
  git -C "$PUSHER" push -q origin main
  B="$(git -C "$PUSHER" rev-parse HEAD)"
  SRC="$(cd "$SRC" && pwd -P)"
  BIN_DIR="$C/bin"
  WEB="$C/web"
  STATE="$C/state"
  STATUS="$STATE/cc-switch-web/update-status.json"
  LOG="$C/calls.log"
  SVC="$C/svc"
  mkdir -p "$BIN_DIR" "$WEB" "$STATE"
  echo "bin-$A" >"$BIN_DIR/cc-switch-web"
  echo "web-$A" >"$WEB/index.html"
  echo "$A" >"$SVC"
  : >"$LOG"
}

# run_update [VAR=value ...]: runs the script with a clean environment; sets RC and OUT.
# The VARs come last, so they override the defaults.
run_update() {
  local d="$C/path" t
  rm -rf "$d"
  mkdir -p "$d"
  for t in $BASE_TOOLS; do
    if [ -n "$(command -v "$t")" ]; then ln -s "$(command -v "$t")" "$d/$t"; fi
  done
  for t in pnpm node cargo curl systemctl; do
    if [[ " ${NO_TOOL:-} " != *" $t "* ]]; then ln -s "$STUBS/$t" "$d/$t"; fi
  done
  OUT="$C/out.txt"
  # SRC_ARG / BRANCH_ARG override the arguments, ONE_ARG drops the branch. The whole run is
  # bounded, so a script that hangs shows up as a failed case (exit 124), not a stuck suite.
  local args=("${SRC_ARG:-$SRC}")
  if [ -z "${ONE_ARG:-}" ]; then args+=("${BRANCH_ARG:-main}"); fi
  timeout 30 env -i PATH="$d" HOME="$C/home" XDG_STATE_HOME="$STATE" BIN_DIR="$BIN_DIR" WEB_DIST="$WEB" \
    HOST=100.64.0.7 PORT=18766 HEALTH_TIMEOUT=0 HEALTH_INTERVAL=0 \
    STUB_LOG="$LOG" STUB_SVC="$SVC" STATUS_FILE="$STATUS" \
    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null \
    GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t \
    "$@" "$BASH" "$SCRIPT" "${args[@]}" >"$OUT" 2>&1
  RC=$?
}

field() { # field <name>: a status field; strings raw, the rest as JSON
  python3 -I -c '
import json, sys
v = json.load(open(sys.argv[1]))[sys.argv[2]]
print(v if isinstance(v, str) else json.dumps(v))' "$STATUS" "$1" 2>/dev/null
}

bin_text() { cat "$BIN_DIR/cc-switch-web"; }
web_text() { cat "$WEB/index.html"; }
log_text() { cat "$LOG"; }
count_named() { find "${@:2}" -maxdepth 1 -name "$1" | wc -l | tr -d ' '; }
log_count() { grep -c -F -- "$1" "$LOG" || true; }

# assert_untouched <name>: nothing installed was changed, no replacement files left.
assert_untouched() {
  check "$1: binary untouched" "$(bin_text)" "bin-$A"
  check "$1: dist-web untouched" "$(web_text)" "web-$A"
  check "$1: no .prev left" "$(count_named '*.prev' "$BIN_DIR" "$C")" "0"
  check "$1: service never restarted" "$(log_count 'systemctl --user restart')" "0"
}

# ==== success ==========================================================================

setup_case success
mkdir -p "$SRC/.scratch" && echo x >"$SRC/.scratch/note" # untracked: not a dirty tree
default_dist="$SRC/dist-web"
mkdir -p "$default_dist" && echo "web-$A" >"$default_dist/index.html"
run_update WEB_DIST=
check "success: exit code" "$RC" "0"
check "success: stage" "$(field stage)" "done"
check "success: ok" "$(field ok)" "true"
check "success: fromSha" "$(field fromSha)" "$A"
check "success: toSha" "$(field toSha)" "$B"
check "success: startedAt is a UTC timestamp" "$(field startedAt | grep -cE '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$')" "1"
check "success: finishedAt is a UTC timestamp" "$(field finishedAt | grep -cE '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$')" "1"
check "success: checkout fast-forwarded" "$(git -C "$SRC" rev-parse HEAD)" "$B"
check "success: new binary installed" "$(bin_text)" "bin-$B"
check "success: new dist-web installed at the default WEB_DIST" "$(cat "$default_dist/index.html")" "web-$B"
check "success: previous binary kept" "$(cat "$BIN_DIR/cc-switch-web.prev")" "bin-$A"
check "success: previous dist-web kept" "$(cat "$default_dist.prev/index.html")" "web-$A"
check "success: no .new left" "$(count_named '*.new' "$BIN_DIR" "$SRC")" "0"
check "success: message names the .prev copies" "$(field message | grep -c '\.prev')" "1"
log="$(log_text)"
check_has "success: frozen lockfile install in the checkout" "$log" "pnpm install --frozen-lockfile cwd=$SRC stage=building"
check_has "success: frontend built into dist-web.new" "$log" "pnpm build:web --outDir $default_dist.new cwd=$SRC stage=building"
check_has "success: server built in src-tauri with the new HEAD" "$log" "cargo build --release --example server cwd=$SRC/src-tauri sha=$B stage=building"
check_has "success: unit reloaded while installing is done" "$log" "systemctl --user daemon-reload stage=restarting"
check_has "success: service restarted" "$log" "systemctl --user restart cc-switch-web stage=restarting"
check "success: daemon-reload comes before restart" \
  "$(grep -n -F -e 'daemon-reload' -e '--user restart' "$LOG" | cut -d: -f2- | tr '\n' '|' | sed 's/ stage=[a-z-]*//g')" \
  "systemctl --user daemon-reload|systemctl --user restart cc-switch-web|"
check "success: status is replaced by rename, not rewritten in place" \
  "$(grep -m1 '^snap-' "$LOG")" "snap-replaced"
check_lacks "success: unchanged unit template gives no warning" "$(field message)" "install-server.sh"

# ==== health check address ==============================================================

n=0
for host in "100.64.0.7|http://100.64.0.7:18766/api/capabilities" \
  "|http://127.0.0.1:18766/api/capabilities" \
  "0.0.0.0|http://127.0.0.1:18766/api/capabilities" \
  "fd7a::1|http://[fd7a::1]:18766/api/capabilities"; do
  n=$((n + 1))
  setup_case "health-$n"
  run_update HOST="${host%%|*}"
  check "health host '${host%%|*}': done" "$(field stage)" "done"
  check_has "health host '${host%%|*}': probed ${host#*|}" "$(log_text)" "curl ${host#*|}"
  check "health host '${host%%|*}': only that URL was probed" \
    "$(grep -F 'curl ' "$LOG" | grep -vc -F "curl ${host#*|}" || true)" "0"
done

setup_case health-port
run_update PORT=19001
check_has "health check uses PORT" "$(log_text)" "curl http://100.64.0.7:19001/api/capabilities"
check "health check uses PORT: no probe went to the default port" \
  "$(grep -F 'curl ' "$LOG" | grep -vc -F 'curl http://100.64.0.7:19001/api/capabilities' || true)" "0"

setup_case warmup
run_update STUB_WARMUP=3 HEALTH_TIMEOUT=30 HEALTH_INTERVAL=0.05
check "slow start: done after retries" "$(field stage)" "done"
check "slow start: probed more than once after the restart" "$([ "$(log_count 'curl ')" -ge 4 ] && echo yes)" "yes"

# ==== up to date ========================================================================

setup_case uptodate
git -C "$SRC" fetch -q origin
git -C "$SRC" merge -q --ff-only origin/main
echo "$B" >"$SVC"
echo "bin-$B" >"$BIN_DIR/cc-switch-web"
run_update
check "up to date: exit code" "$RC" "0"
check "up to date: stage" "$(field stage)" "done"
check_has "up to date: message" "$(field message)" "already up to date"
check "up to date: nothing built" "$(log_count 'cargo ')$(log_count 'pnpm ')" "00"
check "up to date: binary untouched" "$(bin_text)" "bin-$B"

setup_case uptodate-service-down
git -C "$SRC" fetch -q origin
git -C "$SRC" merge -q --ff-only origin/main
echo down >"$SVC" # nothing answers: HEAD == origin is not proof that the right build runs
run_update
check "checkout == origin but the service is down: rebuilt" "$(field stage)" "done"
check_lacks "checkout == origin but the service is down: not the shortcut" "$(field message)" "already up to date"
check "checkout == origin but the service is down: new binary" "$(bin_text)" "bin-$B"
check "checkout == origin but the service is down: built" "$(log_count 'cargo ')" "1"

setup_case uptodate-no-sha
git -C "$SRC" fetch -q origin
git -C "$SRC" merge -q --ff-only origin/main
echo none >"$SVC" # answers, but the build carries no sha
run_update
check "checkout == origin but the build has no sha: rebuilt" "$(field stage)" "done"
check "checkout == origin but the build has no sha: new binary" "$(bin_text)" "bin-$B"

setup_case ahead-of-running
git -C "$SRC" fetch -q origin
git -C "$SRC" merge -q --ff-only origin/main # checkout at B, the service still runs A
run_update
check "checkout ahead of the running build: rebuilt" "$(field stage)" "done"
check "checkout ahead of the running build: new binary" "$(bin_text)" "bin-$B"
check "checkout ahead of the running build: fromSha is the checkout HEAD" "$(field fromSha)" "$B"

# ==== refusals before anything is touched ==================================================

setup_case dirty
echo changed >>"$SRC/f"
run_update
check "dirty tree: exit code" "$RC" "1"
check "dirty tree: stage" "$(field stage)" "failed"
check "dirty tree: ok" "$(field ok)" "false"
check_has "dirty tree: message" "$(field message)" "differ from HEAD"
check_lacks "dirty tree: no checkout note before the merge" "$(field message)" "checkout stays"
check "dirty tree: finishedAt set" "$(field finishedAt | grep -c '^20')" "1"
check "dirty tree: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"
check "dirty tree: checkout stays" "$(git -C "$SRC" rev-parse HEAD)" "$A"
check "dirty tree: nothing built" "$(log_count 'cargo ')$(log_count 'pnpm ')" "00"
assert_untouched "dirty tree"

setup_case staged
echo changed >>"$SRC/f"
git -C "$SRC" add f
run_update
check "staged change counts as dirty" "$(field stage)" "failed"

setup_case non-ff
commit_in "$SRC" local-only local # local commit on main that origin lacks
LOCAL="$(git -C "$SRC" rev-parse HEAD)"
run_update
check "non-ff: exit code" "$RC" "1"
check "non-ff: stage" "$(field stage)" "failed"
check_has "non-ff: message" "$(field message)" "cannot fast-forward"
check "non-ff: checkout not moved" "$(git -C "$SRC" rev-parse HEAD)" "$LOCAL"
check "non-ff: nothing built" "$(log_count 'cargo ')$(log_count 'pnpm ')" "00"
assert_untouched "non-ff"

setup_case wrong-branch
git -C "$SRC" checkout -q -b other
run_update
check "wrong branch: stage" "$(field stage)" "failed"
check_has "wrong branch: message names both branches" "$(field message)" "is on other, not on main"
check "wrong branch: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"
assert_untouched "wrong branch"

setup_case detached
git -C "$SRC" checkout -q --detach
run_update
check "detached HEAD: stage" "$(field stage)" "failed"
check_has "detached HEAD: message" "$(field message)" "detached HEAD"

setup_case bad-branch
BRANCH_ARG='a..b' run_update
check "invalid branch name: exit code" "$RC" "1"
check_has "invalid branch name: message" "$(field message)" "invalid branch name"
check "invalid branch name: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"

setup_case missing-tool
NO_TOOL=pnpm run_update
check "missing tool: exit code" "$RC" "1"
check "missing tool: stage" "$(field stage)" "failed"
check_has "missing tool: message names it" "$(field message)" "missing build tools: pnpm"
check_lacks "missing tool: does not blame an installed tool" "$(field message)" "cargo"
check "missing tool: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"
check "missing tool: no tool ran" "$(log_count 'cargo ')$(log_count 'curl ')$(log_count 'systemctl ')" "000"
assert_untouched "missing tool"

setup_case several-missing
NO_TOOL="cargo curl" run_update
check_has "several missing tools: all named" "$(field message)" "missing build tools: cargo curl"

setup_case build-path
rm -rf "$TMP/bp" && mkdir -p "$TMP/bp" && ln -s "$STUBS/pnpm" "$TMP/bp/pnpm"
NO_TOOL=pnpm run_update CC_SWITCH_WEB_BUILD_PATH="$TMP/bp"
check "CC_SWITCH_WEB_BUILD_PATH supplies a tool missing from PATH" "$(field stage)" "done"

setup_case no-install
rm -f "$BIN_DIR/cc-switch-web"
run_update
check "no installed binary: stage" "$(field stage)" "failed"
check_has "no installed binary: message" "$(field message)" "no installed binary"
check "no installed binary: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"

setup_case relative-dist
run_update WEB_DIST=relative/dist
check "relative WEB_DIST: stage" "$(field stage)" "failed"
check_has "relative WEB_DIST: message" "$(field message)" "must be an absolute path"

setup_case subdir-source
SRC_ARG="$SRC/src-tauri" run_update
check "source dir inside a checkout: stage" "$(field stage)" "failed"
check_has "source dir inside a checkout: message" "$(field message)" "is not the root of a git checkout"
check "source dir inside a checkout: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"
assert_untouched "source dir inside a checkout"

setup_case branch-not-on-origin
git -C "$SRC" checkout -q -b nosuch
BRANCH_ARG=nosuch run_update
check "branch missing on origin: stage" "$(field stage)" "failed"
check_has "branch missing on origin: message" "$(field message)" "origin/nosuch does not exist"
check "branch missing on origin: checkout not moved" "$(git -C "$SRC" rev-parse HEAD)" "$A"
assert_untouched "branch missing on origin"

setup_case port-not-numeric
run_update PORT=18x66
check "PORT=18x66: stage" "$(field stage)" "failed"
check_has "PORT=18x66: message" "$(field message)" "PORT must be a number"
check "PORT=18x66: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"
assert_untouched "PORT=18x66"

for bad in HEALTH_TIMEOUT=abc HEALTH_TIMEOUT=1.5 HEALTH_TIMEOUT=08 HEALTH_TIMEOUT=010 HEALTH_INTERVAL=abc HEALTH_INTERVAL=1. FETCH_TIMEOUT=abc FETCH_TIMEOUT=0; do
  setup_case "bad-${bad//[^a-zA-Z0-9]/_}"
  run_update "$bad"
  check "$bad: stage" "$(field stage)" "failed"
  check_has "$bad: message names the variable" "$(field message)" "${bad%%=*} must be"
  check "$bad: nothing fetched" "$(git -C "$SRC" rev-parse origin/main)" "$A"
  assert_untouched "$bad"
done

setup_case no-source-dir
SRC_ARG="$C/does-not-exist" run_update
check "missing source dir: stage" "$(field stage)" "failed"
check_has "missing source dir: message" "$(field message)" "does not exist"
assert_untouched "missing source dir"

setup_case usage
ONE_ARG=1 run_update
check "one argument: exit code" "$RC" "1"
check "one argument: stage" "$(field stage)" "failed"
check_has "one argument: message" "$(field message)" "usage: self-update.sh <source_dir> <branch>"
assert_untouched "one argument"

# ==== fetch ======================================================================================

# an origin that accepts the connection and never answers; the fifo hands the port over
setup_case fetch-hangs
mkfifo "$C/port"
python3 -I -c '
import socket, sys
s = socket.socket()
s.bind(("127.0.0.1", 0))
s.listen(5)
open(sys.argv[1], "w").write(str(s.getsockname()[1]))
held = []
while True:
    held.append(s.accept()[0])' "$C/port" &
LISTENER=$!
read -r -t 10 port <"$C/port" || [ -n "$port" ] || echo "the stub origin never started" # no newline: read returns 1 with the value set
git -C "$SRC" remote set-url origin "http://127.0.0.1:$port/repo.git"
run_update FETCH_TIMEOUT=1
kill "$LISTENER" 2>/dev/null
wait "$LISTENER" 2>/dev/null
LISTENER=""
check "fetch against a silent origin: exit code" "$RC" "1"
check "fetch against a silent origin: stage" "$(field stage)" "failed"
check_has "fetch against a silent origin: message names fetch and the bound" "$(field message)" "git fetch origin timed out after 1s"
check "fetch against a silent origin: checkout not moved" "$(git -C "$SRC" rev-parse HEAD)" "$A"
assert_untouched "fetch against a silent origin"

setup_case fetch-secret
git -C "$SRC" remote set-url origin "file://$C/user:secret@nowhere/repo.git"
run_update
check "failing fetch: stage" "$(field stage)" "failed"
check_has "failing fetch: message says what failed" "$(field message)" "git fetch origin failed (exit "
check_has "failing fetch: git's output is in the unit log" "$(cat "$OUT")" "secret"
check_lacks "failing fetch: no git output in the status file" "$(cat "$STATUS")" "secret"
check_has "failing fetch: message points at the journal" "$(field message)" "journalctl --user -u cc-switch-web-update"

setup_case merge-output
commit_in "$SRC" local-only secret-local
run_update
check "failing merge: stage" "$(field stage)" "failed"
check_lacks "failing merge: no git output in the status file" "$(cat "$STATUS")" "hint:"
check_has "failing merge: git's output is in the unit log" "$(cat "$OUT")" "fatal"

# ==== build failures leave the installation alone =============================================

setup_case pnpm-build-fails
run_update STUB_FAIL_PNPM_BUILD=1
check "pnpm build fails: exit code" "$RC" "1"
check "pnpm build fails: stage" "$(field stage)" "failed"
check_has "pnpm build fails: message names the command" "$(field message)" "pnpm build:web failed (exit 3)"
check_has "pnpm build fails: message points at the journal" "$(field message)" "journalctl --user -u cc-switch-web-update"
check_has "pnpm build fails: message says the checkout stays on the new commit" "$(field message)" "The checkout stays on ${B:0:7}"
check "pnpm build fails: checkout is on the new commit" "$(git -C "$SRC" rev-parse HEAD)" "$B"
check "pnpm build fails: cargo not started" "$(log_count 'cargo ')" "0"
assert_untouched "pnpm build fails"

setup_case cargo-fails
run_update STUB_FAIL_CARGO=1
check "cargo fails: stage" "$(field stage)" "failed"
check_has "cargo fails: message names the command" "$(field message)" "cargo build --release --example server failed (exit 101)"
assert_untouched "cargo fails"
check "cargo fails: toSha recorded" "$(field toSha)" "$B"

setup_case no-index
run_update STUB_NO_INDEX=1
check "frontend build without index.html: stage" "$(field stage)" "failed"
check_has "frontend build without index.html: message" "$(field message)" "produced no"
assert_untouched "frontend build without index.html"

# ==== rollback ==================================================================================

setup_case health-fails
run_update STUB_BIN=bin-down
check "health fails: exit code" "$RC" "1"
check "health fails: stage" "$(field stage)" "rolled-back"
check "health fails: ok" "$(field ok)" "false"
check "health fails: finishedAt set" "$(field finishedAt | grep -c '^20')" "1"
check "health fails: binary restored" "$(bin_text)" "bin-$A"
check "health fails: dist-web restored" "$(web_text)" "web-$A"
check "health fails: .prev copies kept" "$(cat "$BIN_DIR/cc-switch-web.prev")$(cat "$WEB.prev/index.html")" "bin-$A""web-$A"
check "health fails: failed dist-web kept aside as .new" "$(cat "$WEB.new/index.html")" "web-$B"
check "health fails: service restarted twice" "$(log_count 'systemctl --user restart')" "2"
check "health fails: the restored build is running" "$(cat "$SVC")" "$A"
check_has "health fails: message says the checkout stays on the new commit" "$(field message)" "The checkout stays on ${B:0:7}"
check "health fails: checkout stays on the new commit" "$(git -C "$SRC" rev-parse HEAD)" "$B"
check_has "health fails: message gives the reason" "$(field message)" "did not report build ${B:0:7}"

setup_case old-sha-still-reported
run_update STUB_BIN="bin-$A" # curl answers, but with the old buildSha
check "old buildSha after restart: stage" "$(field stage)" "rolled-back"
check "old buildSha after restart: binary restored" "$(bin_text)" "bin-$A"
check "old buildSha after restart: dist-web restored" "$(web_text)" "web-$A"

setup_case null-sha
run_update STUB_BIN="bin-none"
check "service reports no buildSha: stage" "$(field stage)" "rolled-back"

setup_case rollback-fails
: >"$SVC.always-down"
echo none >"$SVC" # the service runs now with an unknown build, then never comes back
run_update
check "rollback fails: exit code" "$RC" "1"
check "rollback fails: stage" "$(field stage)" "rollback-failed"
check "rollback fails: ok" "$(field ok)" "false"
check_has "rollback fails: message names the bin .prev" "$(field message)" "$BIN_DIR/cc-switch-web.prev"
check_has "rollback fails: message names the dist-web .prev" "$(field message)" "$WEB.prev"
check "rollback fails: .prev copies still there" "$(cat "$BIN_DIR/cc-switch-web.prev")$(cat "$WEB.prev/index.html")" "bin-$A""web-$A"
check "rollback fails: finishedAt set" "$(field finishedAt | grep -c '^20')" "1"

setup_case rollback-needs-the-running-sha
# the service ran a build older than the checkout's HEAD (A); rolling back must be
# confirmed against what it ran (R), not against fromSha
R="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
echo "$R" >"$SVC"
echo "bin-$R" >"$BIN_DIR/cc-switch-web"
run_update STUB_BIN=bin-down
check "rollback confirmed against the build that was running: stage" "$(field stage)" "rolled-back"
check "rollback confirmed against the build that was running: binary" "$(bin_text)" "bin-$R"
check "rollback confirmed against the build that was running: running" "$(cat "$SVC")" "$R"

# ==== signals ===============================================================================

# run_update_blocked <VAR>: starts the script in its own process group with a stub that
# blocks on the fifo named by VAR (STUB_BLOCK_BUILD or STUB_BLOCK_RESTART); returns once the
# stub is there, with PID the group to signal.
SETSID="$(command -v setsid)"
run_update_blocked() {
  mkfifo "$C/fifo"
  (
    local d="$C/path" t
    rm -rf "$d"
    mkdir -p "$d"
    for t in $BASE_TOOLS; do if [ -n "$(command -v "$t")" ]; then ln -s "$(command -v "$t")" "$d/$t"; fi; done
    for t in pnpm node cargo curl systemctl; do ln -s "$STUBS/$t" "$d/$t"; done
    exec env -i PATH="$d" HOME="$C/home" XDG_STATE_HOME="$STATE" BIN_DIR="$BIN_DIR" WEB_DIST="$WEB" \
      HOST=100.64.0.7 PORT=18766 HEALTH_TIMEOUT=0 HEALTH_INTERVAL=0 STUB_LOG="$LOG" STUB_SVC="$SVC" \
      GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@t GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@t \
      STATUS_FILE="$STATUS" "$1=$C/fifo" "$SETSID" "$BASH" "$SCRIPT" "$SRC" main >"$C/out.txt" 2>&1
  ) &
  PID=$!
  read -r -t 10 _ <"$C/fifo" || echo "the stub never reached the fifo: $(cat "$C/out.txt")"
}

setup_case term-building
run_update_blocked STUB_BLOCK_BUILD
kill -TERM -- "-$PID"
wait "$PID"
check "SIGTERM while building: exit code" "$?" "143"
check "SIGTERM while building: stage" "$(field stage)" "failed"
check "SIGTERM while building: ok" "$(field ok)" "false"
check_has "SIGTERM while building: message" "$(field message)" "interrupted"
check "SIGTERM while building: finishedAt set" "$(field finishedAt | grep -c '^20')" "1"
assert_untouched "SIGTERM while building"

setup_case term-restarting
run_update_blocked STUB_BLOCK_RESTART
kill -TERM -- "-$PID"
wait "$PID"
check "SIGTERM after the swap: exit code" "$?" "143"
check "SIGTERM after the swap: rolled back" "$(field stage)" "rolled-back"
check "SIGTERM after the swap: binary restored" "$(bin_text)" "bin-$A"
check "SIGTERM after the swap: dist-web restored" "$(web_text)" "web-$A"
check "SIGTERM after the swap: restart repeated for the rollback" "$(log_count 'systemctl --user restart')" "2"

# ==== status file ===============================================================================

setup_case 'weird' 'we"ird\nlit\back
na	me'
git -C "$SRC" checkout -q -b other
run_update
check "quote, backslash, newline and tab in a message: file parses as JSON" "$(python3 -I -c 'import json,sys; json.load(open(sys.argv[1])); print("valid")' "$STATUS" 2>&1)" "valid"
check "quote, backslash, newline and tab in a message: round trip" \
  "$([[ "$(field message)" == *'we"ird\nlit\back'$'\n''na'$'\t''me is on other'* ]] && echo yes)" "yes"
check "no temporary status files left" "$(count_named '*tmp*' "$STATE/cc-switch-web")" "0"

setup_case final-json
run_update
check "success: every field present" \
  "$(python3 -I -c 'import json,sys; print(",".join(sorted(json.load(open(sys.argv[1])))))' "$STATUS")" \
  "finishedAt,fromSha,message,ok,stage,startedAt,toSha"

setup_case early-null
echo changed >>"$SRC/f"
run_update
check "fromSha is the checkout HEAD in a refusal for a dirty tree" "$(field fromSha)" "$A"
check "toSha is null before the fetch" "$(field toSha)" "null"

# ==== second consecutive update, modes, trailing slash =======================================

setup_case second-update
mkdir -p "$WEB.prev"
echo stale >"$WEB.prev/old"
echo stale-bin >"$BIN_DIR/cc-switch-web.prev"
run_update
check "second update: stage" "$(field stage)" "done"
check "second update: installed binary mode" "$(stat -c %a "$BIN_DIR/cc-switch-web")" "755"
check "second update: .prev is the build that was live" "$(cat "$WEB.prev/index.html")" "web-$A"
check "second update: stale .prev content is gone" "$([ -e "$WEB.prev/old" ] && echo present || echo gone)" "gone"
check "second update: live dist-web is not nested in .prev" "$([ -e "$WEB.prev/$(basename "$WEB")" ] && echo nested || echo flat)" "flat"
check "second update: previous binary" "$(cat "$BIN_DIR/cc-switch-web.prev")" "bin-$A"

setup_case second-update-rollback
mkdir -p "$WEB.prev"
echo stale >"$WEB.prev/old"
echo stale-bin >"$BIN_DIR/cc-switch-web.prev"
run_update STUB_BIN=bin-down
check "second update, failed: stage" "$(field stage)" "rolled-back"
check "second update, failed: rolled back to the build that was live" "$(web_text)" "web-$A"
check "second update, failed: binary" "$(bin_text)" "bin-$A"

setup_case trailing-slash
run_update WEB_DIST="$WEB/"
check "WEB_DIST with a trailing slash: stage" "$(field stage)" "done"
check "WEB_DIST with a trailing slash: new dist-web" "$(web_text)" "web-$B"
check "WEB_DIST with a trailing slash: .prev lies next to dist-web" "$(cat "$WEB.prev/index.html")" "web-$A"
check "WEB_DIST with a trailing slash: nothing inside the live dist-web" "$(find "$WEB" -mindepth 1 -maxdepth 1 -name '.*' | wc -l | tr -d ' ')" "0"

# ==== unit template ===============================================================================

B_FILE=scripts/cc-switch-web.service.template B_TEXT=unit-two setup_case unit-changed
run_update
check "changed unit template: update still succeeds" "$(field stage)" "done"
check_has "changed unit template: warning to run install-server.sh" "$(field message)" "run scripts/install-server.sh by hand"

echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
