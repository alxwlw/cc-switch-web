#!/usr/bin/env bash
# Tests `scripts/self-update.sh --release o/r` (release mode) against a fake release on disk
# (scripts/tests/lib/release-fixture.sh, file:// URLs) with stub systemctl/curl/ldd/uname in
# PATH and logging wrappers around tar and sha256sum. The stubs record their calls and the
# status stage at call time in a log, and play a service whose reported webVersion follows
# the binary installed at the last restart.
# Needs only what a stock Linux runner has: bash, curl, tar, gzip, sha256sum, sed, awk, grep.
# Run: bash scripts/tests/self-update-release.test.sh   (CI runs every scripts/tests/*.test.sh)
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../self-update.sh"
# shellcheck source=scripts/tests/lib/release-fixture.sh
. "$HERE/lib/release-fixture.sh"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

passes=0
fails=0
T_NAME=""
T_BAD=0

begin() {
  T_NAME="$1"
  T_BAD=0
}

check() { # check <what> <got> <want>
  if [ "$2" != "$3" ]; then
    echo "     $T_NAME: $1: got '$2', want '$3'"
    T_BAD=1
  fi
}

check_has() { # check_has <what> <haystack> <needle>
  if [[ "$2" != *"$3"* ]]; then
    echo "     $T_NAME: $1: '$2' lacks '$3'"
    T_BAD=1
  fi
}

end() {
  if [ "$T_BAD" -eq 0 ]; then
    echo "ok   $T_NAME"
    passes=$((passes + 1))
  else
    echo "FAIL $T_NAME"
    fails=$((fails + 1))
  fi
}

# --- stubs --------------------------------------------------------------------------

STUBS="$TMP/stubs"
mkdir -p "$STUBS"
REAL_CURL="$(command -v curl)"
REAL_TAR="$(command -v tar)"
REAL_SHA="$(command -v sha256sum)"
REAL_UNAME="$(command -v uname)"

# shellcheck disable=SC2016 # expanded by the stubs, not here
common='
log() { printf "%s\n" "$*" >>"$STUB_LOG"; }
stage() { sed -n "s/.*\"stage\":\"\([a-z-]*\)\".*/\1/p" "$STATUS_FILE" 2>/dev/null; }
'

cat >"$STUBS/systemctl" <<EOF
#!/usr/bin/env bash
$common
log "systemctl \$* stage=\$(stage)"
if [ "\$1 \$2" = "--user restart" ]; then
  v="\$(sed -n 's/^# fake cc-switch-web //p' "\$BIN_DIR/cc-switch-web")"
  case " \${STUB_DOWN_VERSION:-} " in
    *" \$v "*) echo down >"\$STUB_SVC" ;;
    *) echo "\$v" >"\$STUB_SVC" ;;
  esac
fi
exit 0
EOF

cat >"$STUBS/curl" <<EOF
#!/usr/bin/env bash
$common
for last; do :; done
case "\$last" in
  http://*/api/capabilities | http://*/api/health)
    log "curl \$last stage=\$(stage)"
    state="\$(cat "\$STUB_SVC")"
    if [ "\$state" = down ]; then exit 7; fi
    case "\$last" in
      */api/health) echo '{"ok":true}' ;;
      *) echo "{\"web\":true, \"webVersion\" : \"\$state\"}" ;;
    esac
    exit 0
    ;;
esac
log "curl \$last stage=\$(stage)"
exec "$REAL_CURL" "\$@"
EOF

cat >"$STUBS/tar" <<EOF
#!/usr/bin/env bash
$common
log "tar \$* stage=\$(stage)"
exec "$REAL_TAR" "\$@"
EOF

cat >"$STUBS/sha256sum" <<EOF
#!/usr/bin/env bash
$common
log "sha256sum \$* stage=\$(stage)"
exec "$REAL_SHA" "\$@"
EOF

cat >"$STUBS/ldd" <<'EOF'
#!/usr/bin/env bash
if [ -n "${STUB_LDD_MISSING:-}" ]; then printf '\t%s => not found\n' "$STUB_LDD_MISSING"; fi
exit 0
EOF

cat >"$STUBS/uname" <<EOF
#!/usr/bin/env bash
if [ "\$1" = -m ]; then echo "\${STUB_ARCH:-x86_64}"; exit 0; fi
exec "$REAL_UNAME" "\$@"
EOF
chmod +x "$STUBS"/*

# Only these real tools are visible to the script, next to the stubs.
BASE_TOOLS="bash env sh cat mkdir mv rm date tr sed head tail sleep dirname chmod cp touch grep awk basename mktemp wc sort ln readlink gzip"

# --- cases --------------------------------------------------------------------------

V1=4.0.35-web.1
V2=4.0.35-web.2

# setup_case <name> [installed version]: an install of that version (V1 by default) in
# C/inst and C/bin, a service reporting it, and a fake release V2 (x86_64) as the latest.
setup_case() {
  local ver="${2:-$V1}"
  C="$TMP/$1"
  INST="$C/inst"
  BIN_DIR="$C/bin"
  STATE="$C/state"
  STATUS="$STATE/cc-switch-web/update-status.json"
  REL="$C/rel"
  LOG="$C/calls.log"
  SVC="$C/svc"
  mkdir -p "$INST/dist-web" "$BIN_DIR" "$STATE" "$REL"
  printf '#!/bin/sh\n# fake cc-switch-web %s\n' "$ver" >"$BIN_DIR/cc-switch-web"
  chmod +x "$BIN_DIR/cc-switch-web"
  printf 'web-%s\n' "$ver" >"$INST/dist-web/index.html"
  printf '#!/bin/sh\n# old install.sh %s\n' "$ver" >"$INST/install.sh"
  printf '#!/bin/sh\n# old self-update.sh %s\n' "$ver" >"$INST/self-update.sh"
  chmod +x "$INST/install.sh" "$INST/self-update.sh"
  echo "$ver" >"$SVC"
  : >"$LOG"
  make_release "$REL" "$V2" x86_64
  set_latest "$REL" "$V2"
}

# run_update [VAR=value ...]: runs the script in release mode with a clean environment;
# sets RC and OUT. The VARs come last, so they override the defaults.
run_update() {
  local d="$C/path" t
  rm -rf "$d"
  mkdir -p "$d"
  for t in $BASE_TOOLS; do
    if [ -n "$(command -v "$t")" ]; then ln -s "$(command -v "$t")" "$d/$t"; fi
  done
  for t in curl systemctl tar sha256sum ldd uname; do
    if [[ " ${NO_TOOL:-} " != *" $t "* ]]; then ln -s "$STUBS/$t" "$d/$t"; fi
  done
  OUT="$C/out.txt"
  timeout 60 env -i PATH="$d" HOME="$C/home" XDG_STATE_HOME="$STATE" BIN_DIR="$BIN_DIR" \
    CC_SWITCH_WEB_INSTALL_DIR="$INST" HOST=100.64.0.7 PORT=18766 HEALTH_TIMEOUT=0 HEALTH_INTERVAL=0 \
    CC_SWITCH_WEB_UPDATE_API="file://$REL/api" CC_SWITCH_WEB_DOWNLOAD_BASE="file://$REL" \
    STUB_LOG="$LOG" STUB_SVC="$SVC" STATUS_FILE="$STATUS" \
    "$@" "$BASH" "$SCRIPT" --release o/r >"$OUT" 2>&1
  RC=$?
}

# field <name>: a status field; a string without its quotes, null/true/false as written.
field() {
  local v
  v="$(sed -n "s/.*\"$1\":\"\(\([^\"\\\\]\|\\\\.\)*\)\".*/\1/p" "$STATUS" 2>/dev/null)"
  if [ -z "$v" ]; then v="$(sed -n "s/.*\"$1\":\(null\|true\|false\).*/\1/p" "$STATUS" 2>/dev/null)"; fi
  printf '%s' "$v"
}

bin_version() { sed -n 's/^# fake cc-switch-web //p' "$BIN_DIR/cc-switch-web"; }
web_text() { cat "$INST/dist-web/index.html"; }
log_text() { cat "$LOG"; }
log_count() { grep -c -F -- "$1" "$LOG" || true; }
leftovers() { # leftovers: replacement files that must not outlive a run
  find "$BIN_DIR" "$INST" -maxdepth 1 \( -name '*.new' -o -name '*.prev' -o -name '.update.*' -o -name '*.rollback' \) | wc -l | tr -d ' '
}
# downloads: how many requests went to the release download base
downloads() { grep -c -F -- "curl file://$REL/v" "$LOG" || true; }
# stages_seen: the stages the stubs saw, in order, then the final one from the status file
stages_seen() {
  { grep -o 'stage=[a-z-]*' "$LOG" | sed 's/^stage=//'; field stage; } | awk '$0 != prev { printf "%s%s", sep, $0; sep = ","; prev = $0 }'
}

# assert_untouched <what> [version]: the installed version still runs and nothing was replaced.
assert_untouched() {
  local ver="${2:-$V1}"
  check "$1: binary untouched" "$(bin_version)" "$ver"
  check "$1: dist-web untouched" "$(web_text)" "web-$ver"
  check "$1: install.sh untouched" "$(cat "$INST/install.sh")" "$(printf '#!/bin/sh\n# old install.sh %s' "$ver")"
  check "$1: self-update.sh untouched" "$(cat "$INST/self-update.sh")" "$(printf '#!/bin/sh\n# old self-update.sh %s' "$ver")"
  check "$1: no replacement or download files left" "$(leftovers)" "0"
  check "$1: service never restarted" "$(log_count 'systemctl --user restart')" "0"
  check "$1: final stage is failed" "$(field stage)" "failed"
  check "$1: ok is false" "$(field ok)" "false"
}

# resign <version> <arch>: SHA256SUMS of a one-archive release, after the archive was changed.
resign() {
  local d="$REL/v$1" n="cc-switch-web-$1-linux-$2.tar.gz"
  (cd "$d" && sha256sum "$n" >SHA256SUMS)
}

# --- 1 -------------------------------------------------------------------------------

begin "update to the latest release swaps binary and dist-web and ends done"
setup_case swap
bin_inode="$(stat -c %i "$BIN_DIR/cc-switch-web")"
ln "$BIN_DIR/cc-switch-web" "$C/old-binary-link"       # the running file's inode, kept
ln "$INST/self-update.sh" "$C/old-self-update-link"     # the running script's inode, kept
run_update
check "exit code" "$RC" "0"
check "stages in order" "$(stages_seen)" "checking,downloading,verifying,installing,restarting,done"
check "stage" "$(field stage)" "done"
check "ok" "$(field ok)" "true"
check "fromVersion" "$(field fromVersion)" "$V1"
check "toVersion" "$(field toVersion)" "$V2"
check "fromSha is null" "$(field fromSha)" "null"
check "toSha is null" "$(field toSha)" "null"
check "finishedAt is a UTC timestamp" "$(field finishedAt | grep -cE '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z$')" "1"
check "new binary installed" "$(bin_version)" "$V2"
check "installed binary is executable" "$(stat -c %a "$BIN_DIR/cc-switch-web")" "755"
check "new dist-web" "$(web_text)" "web-$V2"
check "previous binary kept as .prev" "$(sed -n 's/^# fake cc-switch-web //p' "$BIN_DIR/cc-switch-web.prev")" "$V1"
check "previous dist-web kept as .prev" "$(cat "$INST/dist-web.prev/index.html")" "web-$V1"
check "the old binary file was renamed, not overwritten" "$(stat -c %i "$BIN_DIR/cc-switch-web.prev")" "$bin_inode"
check "the old binary's inode still holds the old build" "$(sed -n 's/^# fake cc-switch-web //p' "$C/old-binary-link")" "$V1"
check "install.sh is the one of the archive" "$(sed -n 's/^# fake install.sh //p' "$INST/install.sh")" "$V2"
check "self-update.sh is the one of the archive" "$(sed -n 's/^# fake self-update.sh //p' "$INST/self-update.sh")" "$V2"
check "self-update.sh was replaced by a new file, not written in place" "$(sed -n 's/^# old self-update.sh //p' "$C/old-self-update-link")" "$V1"
check "self-update.sh is executable" "$(stat -c %a "$INST/self-update.sh")" "755"
check "no temporary files left" "$(find "$BIN_DIR" "$INST" -maxdepth 1 \( -name '*.new' -o -name '.update.*' \) | wc -l | tr -d ' ')" "0"
check "message names the version change" "$([[ "$(field message)" == *"$V1 -> $V2"* ]] && echo yes)" "yes"
log="$(log_text)"
check_has "archive of this architecture downloaded" "$log" "curl file://$REL/v$V2/cc-switch-web-$V2-linux-x86_64.tar.gz stage=downloading"
check_has "checksum verified in its own stage" "$log" "sha256sum $C/inst/.update."
check_has "unit reloaded while restarting" "$log" "systemctl --user daemon-reload stage=restarting"
check_has "service restarted" "$log" "systemctl --user restart cc-switch-web stage=restarting"
check "daemon-reload comes before restart" \
  "$(grep -n -F -e 'daemon-reload' -e '--user restart' "$LOG" | cut -d: -f2- | sed 's/ stage=[a-z-]*//' | tr '\n' '|')" \
  "systemctl --user daemon-reload|systemctl --user restart cc-switch-web|"
check_has "the new version is probed after the restart" "$(sed -n '/--user restart/,$p' "$LOG")" "/api/capabilities"
check_has "health is probed after the restart" "$(sed -n '/--user restart/,$p' "$LOG")" "/api/health"
check "the status keys" \
  "$(sed -n 's/^{//;s/}$//;p' "$STATUS" | tr ',' '\n' | sed -n 's/^"\([A-Za-z]*\)":.*/\1/p' | sort | tr '\n' ',')" \
  "finishedAt,fromSha,fromVersion,message,ok,stage,startedAt,toSha,toVersion,"
end

# --- 2 -------------------------------------------------------------------------------

begin "already on the latest release is done without touching anything"
setup_case uptodate "$V2"
bin_inode="$(stat -c %i "$BIN_DIR/cc-switch-web")"
run_update
check "exit code" "$RC" "0"
check "stage" "$(field stage)" "done"
check "ok" "$(field ok)" "true"
check "message" "$([[ "$(field message)" == *"already up to date"* ]] && echo yes)" "yes"
check "binary inode unchanged" "$(stat -c %i "$BIN_DIR/cc-switch-web")" "$bin_inode"
check "binary version" "$(bin_version)" "$V2"
check "dist-web" "$(web_text)" "web-$V2"
check "no replacement or download files left" "$(leftovers)" "0"
check "nothing downloaded" "$(downloads)" "0"
check "service never restarted" "$(log_count 'systemctl --user restart')" "0"
check "fromVersion" "$(field fromVersion)" "$V2"
check "toVersion" "$(field toVersion)" "$V2"
end

# --- 3 -------------------------------------------------------------------------------

begin "a latest release that is not newer fails before downloading"
setup_case older "$V2"
make_release "$REL" "$V1" x86_64
set_latest "$REL" "$V1"
run_update
check "exit code" "$RC" "1"
check "nothing downloaded" "$(downloads)" "0"
check "message" "$(field message | grep -c "the latest release $V1 is not newer than $V2")" "1"
assert_untouched "older release" "$V2"
end

# --- 4 -------------------------------------------------------------------------------

begin "no archive for this architecture fails before the swap"
setup_case noarch
run_update STUB_ARCH=aarch64
check "exit code" "$RC" "1"
check "message names the architecture" "$([[ "$(field message)" == *"no archive for aarch64"* ]] && echo yes)" "yes"
assert_untouched "no aarch64 archive"
end

begin "an architecture without release archives fails before downloading the archive"
setup_case oddarch
run_update STUB_ARCH=riscv64
check "exit code" "$RC" "1"
check "message names the architecture" "$([[ "$(field message)" == *"unsupported architecture riscv64"* ]] && echo yes)" "yes"
check "no archive requested" "$(log_count '.tar.gz')" "0"
assert_untouched "riscv64"
end

# --- 5 -------------------------------------------------------------------------------

begin "a checksum mismatch fails before the swap"
setup_case badsum
printf 'x' >>"$REL/v$V2/cc-switch-web-$V2-linux-x86_64.tar.gz"
run_update
check "exit code" "$RC" "1"
check "message" "$([[ "$(field message)" == *"checksum mismatch"* ]] && echo yes)" "yes"
check "the archive was never unpacked" "$(log_count 'tar ')" "0"
assert_untouched "checksum mismatch"
end

# --- 6 -------------------------------------------------------------------------------

begin "missing system libraries fail before the swap and name install.sh"
setup_case nolibs
run_update STUB_LDD_MISSING=libwebkit2gtk-4.1.so.0
check "exit code" "$RC" "1"
check "message lists the library" "$([[ "$(field message)" == *"libwebkit2gtk-4.1.so.0"* ]] && echo yes)" "yes"
check "message names install.sh" "$([[ "$(field message)" == *"install.sh"* ]] && echo yes)" "yes"
assert_untouched "missing libraries"
end

# --- 7 -------------------------------------------------------------------------------

begin "a new build that does not come up is rolled back"
setup_case rollback
ln "$BIN_DIR/cc-switch-web" "$C/old-binary-link"
run_update STUB_DOWN_VERSION="$V2"
check "exit code" "$RC" "1"
check "stage" "$(field stage)" "rolled-back"
check "ok" "$(field ok)" "false"
check "stages in order" "$(stages_seen)" "checking,downloading,verifying,installing,restarting,rolling-back,rolled-back"
check "binary is the old one again" "$(bin_version)" "$V1"
check "dist-web is the old one again" "$(web_text)" "web-$V1"
check "install.sh of the install dir is the old one" "$(cat "$INST/install.sh")" "$(printf '#!/bin/sh\n# old install.sh %s' "$V1")"
check "self-update.sh of the install dir is the old one" "$(cat "$INST/self-update.sh")" "$(printf '#!/bin/sh\n# old self-update.sh %s' "$V1")"
check "the service runs the old version again" "$(cat "$SVC")" "$V1"
check "service restarted twice" "$(log_count 'systemctl --user restart')" "2"
check "the .prev copies are kept" "$(sed -n 's/^# fake cc-switch-web //p' "$BIN_DIR/cc-switch-web.prev")" "$V1"
check "message names the failure" "$([[ "$(field message)" == *"did not report version $V2"* ]] && echo yes)" "yes"
check "fromVersion" "$(field fromVersion)" "$V1"
check "toVersion" "$(field toVersion)" "$V2"
check "no download files left" "$(find "$INST" -maxdepth 1 -name '.update.*' | wc -l | tr -d ' ')" "0"
end

begin "a rollback that cannot bring the old build up is rollback-failed"
setup_case rollback-failed
run_update STUB_DOWN_VERSION="$V2 $V1"
check "exit code" "$RC" "1"
check "stage" "$(field stage)" "rollback-failed"
check "ok" "$(field ok)" "false"
check "message" "$([[ "$(field message)" == *"ROLLBACK FAILED"* ]] && echo yes)" "yes"
check "the previous copies are named in the message" "$([[ "$(field message)" == *"cc-switch-web.prev"* ]] && echo yes)" "yes"
check "the old binary was put back" "$(bin_version)" "$V1"
check "the old dist-web was put back" "$(web_text)" "web-$V1"
check "the previous copies are kept for the operator" "$(sed -n 's/^# fake cc-switch-web //p' "$BIN_DIR/cc-switch-web.prev")" "$V1"
end

# --- 8 -------------------------------------------------------------------------------

begin "version_newer compares numbers"
# shellcheck disable=SC1090 # the function is cut out of the script under test
eval "$(sed -n '/^VERSION_RE=/p;/^version_newer() {/,/^}/p' "$SCRIPT")"
check "the function was found" "$(type -t version_newer)" "function"
newer() { if version_newer "$1" "$2"; then echo newer; else echo not; fi; }
check "web.2 > web.1" "$(newer 4.0.35-web.2 4.0.35-web.1)" "newer"
check "4.0.36-web.1 > 4.0.35-web.9" "$(newer 4.0.36-web.1 4.0.35-web.9)" "newer"
check "web.10 > web.9" "$(newer 4.0.35-web.10 4.0.35-web.9)" "newer"
check "4.1.0-web.1 > 4.0.99-web.99" "$(newer 4.1.0-web.1 4.0.99-web.99)" "newer"
check "10.0.0-web.1 > 9.9.9-web.9" "$(newer 10.0.0-web.1 9.9.9-web.9)" "newer"
check "equal is not newer" "$(newer 4.0.35-web.2 4.0.35-web.2)" "not"
check "web.9 is not newer than web.10" "$(newer 4.0.35-web.9 4.0.35-web.10)" "not"
check "older minor is not newer" "$(newer 4.0.35-web.1 4.0.36-web.1)" "not"
check "leading zeros do not matter" "$(newer 4.0.35-web.010 4.0.35-web.9)" "newer"
check "a number past 64 bits still compares" "$(newer 4.0.35-web.99999999999999999999 4.0.35-web.9)" "newer"
check "not a fork version is not newer" "$(newer nightly 4.0.35-web.1)" "not"
check "nothing is newer than not a fork version" "$(newer 4.0.35-web.1 nightly)" "not"
check "an upstream version without web is not a fork version" "$(newer 4.0.36 4.0.35-web.1)" "not"
end

# --- more: what a broken release must not do --------------------------------------------

begin "a latest tag that is not a fork version fails before downloading"
setup_case badtag
printf '{"tag_name": "nightly"}\n' >"$REL/api/repos/o/r/releases/latest"
run_update
check "exit code" "$RC" "1"
check "message" "$([[ "$(field message)" == *"'nightly' is not a cc-switch-web release"* ]] && echo yes)" "yes"
check "nothing downloaded" "$(downloads)" "0"
assert_untouched "bad tag"
end

begin "a latest release that cannot be read fails before downloading"
setup_case noapi
rm -f "$REL/api/repos/o/r/releases/latest"
run_update
check "exit code" "$RC" "1"
check "message" "$([[ "$(field message)" == *"could not read the latest release of o/r"* ]] && echo yes)" "yes"
check "nothing downloaded" "$(downloads)" "0"
assert_untouched "no release"
end

begin "a service that does not report its version fails before downloading"
setup_case svcdown
echo down >"$SVC"
run_update
check "exit code" "$RC" "1"
check "message" "$([[ "$(field message)" == *"does not report its version"* ]] && echo yes)" "yes"
check "nothing downloaded" "$(downloads)" "0"
assert_untouched "service down"
end

begin "an archive that is not a tarball fails before the swap"
setup_case garbage
n="cc-switch-web-$V2-linux-x86_64.tar.gz"
echo "this is not gzip" >"$REL/v$V2/$n"
resign "$V2" x86_64
run_update
check "exit code" "$RC" "1"
check "message" "$([[ "$(field message)" == *"could not read $n"* ]] && echo yes)" "yes"
assert_untouched "garbage archive"
end

begin "an archive with unsafe paths fails before the swap"
for evil in '../evil' '/evil'; do
  setup_case "unsafe"
  n="cc-switch-web-$V2-linux-x86_64"
  w="$(mktemp -d "$TMP/w.XXXXXX")"
  "$REAL_TAR" -xzf "$REL/v$V2/$n.tar.gz" -C "$w"
  echo evil >"$w/evil"
  "$REAL_TAR" -czf "$REL/v$V2/$n.tar.gz" -C "$w" --absolute-names --transform "s,^evil\$,$evil," "$n" evil
  resign "$V2" x86_64
  check "the fixture really has the unsafe member ($evil)" "$("$REAL_TAR" -tzf "$REL/v$V2/$n.tar.gz" | grep -cxF -- "$evil")" "1"
  run_update
  check "exit code ($evil)" "$RC" "1"
  check "message ($evil)" "$([[ "$(field message)" == *"unsafe paths"* ]] && echo yes)" "yes"
  assert_untouched "unsafe path $evil"
  rm -rf "$w"
done
end

begin "an incomplete archive fails before the swap"
for missing in self-update.sh cc-switch-web dist-web/index.html; do
  setup_case "incomplete"
  n="cc-switch-web-$V2-linux-x86_64"
  w="$(mktemp -d "$TMP/w.XXXXXX")"
  "$REAL_TAR" -xzf "$REL/v$V2/$n.tar.gz" -C "$w"
  rm -f "$w/$n/$missing"
  "$REAL_TAR" -czf "$REL/v$V2/$n.tar.gz" -C "$w" "$n"
  resign "$V2" x86_64
  run_update
  check "exit code (no $missing)" "$RC" "1"
  check "message (no $missing)" "$([[ "$(field message)" == *"incomplete (no $missing)"* ]] && echo yes)" "yes"
  assert_untouched "no $missing"
  rm -rf "$w"
done
end

begin "a missing required tool fails before anything is read"
setup_case notool
NO_TOOL=sha256sum run_update
check "exit code" "$RC" "1"
check "message" "$([[ "$(field message)" == *"missing tools: sha256sum"* ]] && echo yes)" "yes"
check "nothing downloaded" "$(downloads)" "0"
assert_untouched "no sha256sum"
end

echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
