#!/usr/bin/env bash
# Tests scripts/install.sh against a fake release on disk (scripts/tests/lib/release-fixture.sh,
# file:// URLs) with stub systemctl/loginctl/tailscale/sudo/apt-get/ldd/uname/id/curl in PATH.
# The stubs log their calls; the curl stub answers /api/capabilities with the version of the
# installed binary, like a service that follows the last restart.
# Run: bash scripts/tests/install.test.sh   (local gate; CI runs every scripts/tests/*.test.sh)
set -u

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$HERE/../install.sh"
# shellcheck source=scripts/tests/lib/release-fixture.sh
. "$HERE/lib/release-fixture.sh"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

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
REAL_CURL="$(command -v curl)"
REAL_UNAME="$(command -v uname)"

cat >"$STUBS/systemctl" <<'EOF'
#!/usr/bin/env bash
echo "systemctl $*" >>"$STUB_LOG"
if [ "$1 $2" = "--user show-environment" ] && [ -n "${STUB_SYSTEMD_FAIL:-}" ]; then exit 1; fi
exit 0
EOF

cat >"$STUBS/loginctl" <<'EOF'
#!/usr/bin/env bash
echo "loginctl $*" >>"$STUB_LOG"
case "$1" in
  show-user) echo "Linger=${STUB_LINGER:-no}" ;;
  enable-linger) if [ -n "${STUB_LINGER_FAIL:-}" ]; then exit 1; fi ;;
esac
exit 0
EOF

cat >"$STUBS/tailscale" <<'EOF'
#!/usr/bin/env bash
if [ "$*" = "ip -4" ]; then echo 100.64.0.7; echo 100.64.0.8; exit 0; fi
exit 1
EOF

cat >"$STUBS/sudo" <<'EOF'
#!/usr/bin/env bash
echo "sudo $*" >>"$STUB_LOG"
exec "$@"
EOF

cat >"$STUBS/apt-get" <<'EOF'
#!/usr/bin/env bash
echo "apt-get $*" >>"$STUB_LOG"
exit 0
EOF

cat >"$STUBS/ldd" <<'EOF'
#!/usr/bin/env bash
# the library is "missing" until an apt-get install ran
if [ -n "${STUB_LDD_MISSING:-}" ] && ! grep -q 'apt-get install' "$STUB_LOG"; then
  printf '\t%s => not found\n' "$STUB_LDD_MISSING"
fi
exit 0
EOF

cat >"$STUBS/id" <<'EOF'
#!/usr/bin/env bash
case "$1" in
  -u) echo "${STUB_UID:-1000}" ;;
  -un) echo tester ;;
esac
EOF

cat >"$STUBS/uname" <<EOF
#!/usr/bin/env bash
if [ "\$1" = -m ]; then echo "\${STUB_ARCH:-x86_64}"; exit 0; fi
exec "$REAL_UNAME" "\$@"
EOF

cat >"$STUBS/curl" <<EOF
#!/usr/bin/env bash
for last; do :; done
case "\$last" in
  http://*/api/capabilities)
    echo "curl \$last" >>"\$STUB_LOG"
    if [ -n "\${STUB_CAP_VERSION:-}" ]; then
      v="\$STUB_CAP_VERSION"
    else
      v="\$(sed -n 's/^# fake cc-switch-web //p' "\$HOME/.local/bin/cc-switch-web" 2>/dev/null)"
    fi
    if [ -z "\$v" ]; then exit 7; fi
    echo "{\"web\":true,\"webVersion\":\"\$v\"}"
    exit 0
    ;;
esac
exec "$REAL_CURL" "\$@"
EOF
chmod +x "$STUBS"/*

# Only these real tools are visible to the script, next to the stubs.
BASE_TOOLS="timeout bash env sh cat mkdir mv rm date tr sed head tail install sleep dirname chmod cp touch grep awk basename mktemp wc sort ln readlink tar gzip sha256sum hostname stat find cmp ls"

# --- cases --------------------------------------------------------------------------

# shellcheck disable=SC2088 # the installer prints the tilde literally for the user's shell
FG_BIN='~/.local/bin/cc-switch-web'
V1=4.0.35-web.1
V2=4.0.35-web.2

# setup_case <name> [fixture arch...]: home, a fake release V1 for the given arches
# (default x86_64) as the latest one. Sets C HOME_D REL LOG.
setup_case() {
  C="$TMP/$1"
  shift
  HOME_D="$C/home"
  REL="$C/rel"
  LOG="$C/calls.log"
  mkdir -p "$HOME_D" "$REL"
  : >"$LOG"
  if [ $# -eq 0 ]; then set -- x86_64; fi
  make_release "$REL" "$V1" "$@"
  set_latest "$REL" "$V1"
}

# run_install [VAR=value ...] -- [install.sh args]: sets RC OUT ERR. The VARs come last
# in the environment, so they override the defaults. SCRIPT_UNDER_TEST and PIPED tune it.
run_install() {
  local d="$C/path" t envs=()
  rm -rf "$d"
  mkdir -p "$d"
  for t in $BASE_TOOLS; do
    if [ -n "$(command -v "$t")" ]; then ln -s "$(command -v "$t")" "$d/$t"; fi
  done
  for t in curl systemctl loginctl tailscale sudo apt-get ldd id uname; do
    if [[ " ${NO_TOOL:-} " != *" $t "* ]]; then ln -s "$STUBS/$t" "$d/$t"; fi
  done
  while [ $# -gt 0 ] && [ "$1" != -- ]; do
    envs+=("$1")
    shift
  done
  shift
  OUT="$C/out.txt"
  ERR="$C/err.txt"
  local script="${SCRIPT_UNDER_TEST:-$SCRIPT}"
  if [ -n "${PIPED:-}" ]; then
    timeout 60 env -i PATH="$d" HOME="$HOME_D" USER=tester STUB_LOG="$LOG" \
      CC_SWITCH_WEB_REPO=o/r CC_SWITCH_WEB_DOWNLOAD_BASE="file://$REL" CC_SWITCH_WEB_UPDATE_API="file://$REL/api" \
      "${envs[@]}" "$BASH" -s -- "$@" <"$script" >"$OUT" 2>"$ERR"
  else
    timeout 60 env -i PATH="$d" HOME="$HOME_D" USER=tester STUB_LOG="$LOG" \
      CC_SWITCH_WEB_REPO=o/r CC_SWITCH_WEB_DOWNLOAD_BASE="file://$REL" CC_SWITCH_WEB_UPDATE_API="file://$REL/api" \
      "${envs[@]}" "$BASH" "$script" "$@" >"$OUT" 2>"$ERR"
  fi
  RC=$?
}

bin_path() { echo "$HOME_D/.local/bin/cc-switch-web"; }
unit_path() { echo "$HOME_D/.config/systemd/user/cc-switch-web.service"; }
inst_path() { echo "$HOME_D/.local/share/cc-switch-web"; }
unit_text() { cat "$(unit_path)" 2>/dev/null; }
out_text() { cat "$OUT" "$ERR"; }
log_text() { cat "$LOG"; }
exists() { [ -e "$1" ] && echo yes || echo no; }

# 1: piped into bash, as `curl | bash` does
setup_case fresh
PIPED=1 run_install --
check "fresh install lays out files and enables the unit: exit code" "$RC" "0"
for p in "$(bin_path)" "$(inst_path)/dist-web/index.html" "$(inst_path)/install.sh" "$(inst_path)/self-update.sh" "$(unit_path)" "$HOME_D/.cc-switch-web"; do
  check "fresh: ${p#"$HOME_D"/} exists" "$(exists "$p")" "yes"
done
check "fresh: binary is executable" "$([ -x "$(bin_path)" ] && echo yes || echo no)" "yes"
check "fresh: binary is the release one" "$(sed -n 2p "$(bin_path)")" "# fake cc-switch-web $V1"
check "fresh: data dir mode" "$(stat -c %a "$HOME_D/.cc-switch-web")" "700"
check_has "fresh: unit HOST" "$(unit_text)" "Environment=HOST=127.0.0.1"
check_has "fresh: unit PORT" "$(unit_text)" "Environment=PORT=18766"
check_has "fresh: unit self-update" "$(unit_text)" "Environment=CC_SWITCH_WEB_SELF_UPDATE=1"
check_has "fresh: unit repo" "$(unit_text)" "Environment=CC_SWITCH_WEB_UPDATE_REPO=o/r"
check_lacks "fresh: unit has no TRUSTED_TAILNET" "$(unit_text)" "TRUSTED_TAILNET"
check_has "fresh: daemon-reload" "$(log_text)" "systemctl --user daemon-reload"
check_has "fresh: enable" "$(log_text)" "systemctl --user enable cc-switch-web"
check_has "fresh: restart" "$(log_text)" "systemctl --user restart cc-switch-web"
check_has "fresh: ssh hint" "$(out_text)" "ssh -L 18766:localhost:18766"

# 2
setup_case tailscale
run_install -- --tailscale
check "--tailscale binds the tailnet address with TRUSTED_TAILNET: exit code" "$RC" "0"
check_has "tailscale: HOST" "$(unit_text)" "Environment=HOST=100.64.0.7"
check_lacks "tailscale: only the first address" "$(unit_text)" "100.64.0.8"
check_has "tailscale: TRUSTED_TAILNET" "$(unit_text)" "Environment=TRUSTED_TAILNET=1"
check_has "tailscale: url" "$(out_text)" "http://100.64.0.7:18766"

# 3
setup_case rerun
run_install -- --tailscale --port 18800 --no-self-update
run_install --
check "a re-run without flags keeps address, port and self-update: exit code" "$RC" "0"
check_has "rerun: HOST kept" "$(unit_text)" "Environment=HOST=100.64.0.7"
check_has "rerun: TRUSTED kept" "$(unit_text)" "Environment=TRUSTED_TAILNET=1"
check_has "rerun: PORT kept" "$(unit_text)" "Environment=PORT=18800"
check_has "rerun: self-update kept" "$(unit_text)" "Environment=CC_SWITCH_WEB_SELF_UPDATE=0"

# 4
setup_case loopback
run_install -- --tailscale --port 18800
run_install -- --loopback
check "--loopback on a re-run switches back: exit code" "$RC" "0"
check_has "loopback: HOST" "$(unit_text)" "Environment=HOST=127.0.0.1"
check_lacks "loopback: no TRUSTED_TAILNET" "$(unit_text)" "TRUSTED_TAILNET"
check_has "loopback: PORT kept" "$(unit_text)" "Environment=PORT=18800"

# 5
setup_case checksum
run_install --
old_bin="$(cat "$(bin_path)")"
old_web="$(cat "$(inst_path)/dist-web/index.html")"
make_release "$REL" "$V2" x86_64
set_latest "$REL" "$V2"
echo corrupt >>"$REL/v$V2/cc-switch-web-$V2-linux-x86_64.tar.gz"
run_install --
check "a checksum mismatch installs nothing: exit code" "$RC" "1"
check_has "checksum: stderr" "$(cat "$ERR")" "checksum"
check "checksum: binary untouched" "$(cat "$(bin_path)")" "$old_bin"
check "checksum: dist-web untouched" "$(cat "$(inst_path)/dist-web/index.html")" "$old_web"

# 6
setup_case arch
run_install STUB_ARCH=riscv64 --
check "an unsupported architecture is an error naming it: exit code" "$RC" "1"
check_has "arch riscv64: names it" "$(cat "$ERR")" "riscv64"
check_has "arch riscv64: docker" "$(cat "$ERR")" "docker"
run_install STUB_ARCH=aarch64 --
check "a missing architecture is an error naming it: exit code" "$RC" "1"
check_has "arch aarch64: no archive for it" "$(cat "$ERR")" "no archive for aarch64"
check_has "arch aarch64: docker" "$(cat "$ERR")" "docker"
check "arch: nothing installed" "$(exists "$(bin_path)")" "no"

# 7
setup_case apt
run_install STUB_LDD_MISSING=libwebkit2gtk-4.1.so.0 --
check "missing libraries are installed with apt through sudo: exit code" "$RC" "0"
check "apt: update then install through sudo" "$(grep '^sudo ' "$LOG")" "sudo apt-get update
sudo apt-get install -y --no-install-recommends libssl3 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 libwebkit2gtk-4.1-0"
check "apt: installed" "$([ -x "$(bin_path)" ] && echo yes || echo no)" "yes"

# 8
setup_case noapt
NO_TOOL=apt-get run_install STUB_LDD_MISSING=libwebkit2gtk-4.1.so.0 --
check "missing libraries without apt stop before anything is installed: exit code" "$RC" "1"
check_has "no apt: names the library" "$(cat "$ERR")" "libwebkit2gtk-4.1.so.0"
check_has "no apt: docker" "$(cat "$ERR")" "docker"
check "no apt: nothing installed" "$(exists "$(bin_path)")" "no"

# 9
setup_case inode
run_install --
inode1="$(stat -c %i "$(bin_path)")"
run_install --
check "the binary is replaced by rename, not rewritten in place: second run" "$RC" "0"
inode2="$(stat -c %i "$(bin_path)")"
check "inode: binary replaced by rename" "$([ "$inode1" != "$inode2" ] && echo different || echo same)" "different"

# 10
setup_case nosystemd
run_install STUB_SYSTEMD_FAIL=1 --
check "without a systemd user session files are installed and a command is printed: exit code" "$RC" "0"
check "no systemd: no enable" "$(grep -c 'enable' "$LOG" || true)" "0"
check "no systemd: no unit" "$(exists "$(unit_path)")" "no"
check "no systemd: binary installed" "$([ -x "$(bin_path)" ] && echo yes || echo no)" "yes"
check_has "no systemd: foreground WEB_DIST" "$(out_text)" "WEB_DIST="
check_has "no systemd: foreground command" "$(out_text)" "$FG_BIN"
setup_case nosystemctl
NO_TOOL=systemctl run_install --
check "no systemctl binary: exit code" "$RC" "0"
check_has "no systemctl binary: foreground command" "$(out_text)" "$FG_BIN"

# 11
setup_case linger
run_install STUB_LINGER_FAIL=1 --
check "a refused linger prints the sudo command: exit code" "$RC" "0"
check_has "linger: sudo command" "$(out_text)" "sudo loginctl enable-linger"

# 12
setup_case uninstall
run_install --
echo keep >"$HOME_D/.cc-switch-web/note"
run_install -- --uninstall
check "--uninstall removes the install and keeps the data: exit code" "$RC" "0"
check "uninstall: data kept" "$(cat "$HOME_D/.cc-switch-web/note")" "keep"
check "uninstall: binary gone" "$(exists "$(bin_path)")" "no"
check "uninstall: unit gone" "$(exists "$(unit_path)")" "no"
check "uninstall: install dir gone" "$(exists "$(inst_path)")" "no"
check_has "uninstall: disable" "$(log_text)" "systemctl --user disable --now cc-switch-web"

# 13
setup_case baked
make_release "$REL" "$V2" x86_64
set_latest "$REL" "$V2"
sed "s/__RELEASE_TAG__/v$V1/" "$SCRIPT" >"$C/install-baked.sh"
SCRIPT_UNDER_TEST="$C/install-baked.sh" run_install --
check "the baked tag wins over the latest release: exit code" "$RC" "0"
check "baked tag: old version installed" "$(sed -n 2p "$(bin_path)")" "# fake cc-switch-web $V1"
HOME_D="$C/home-placeholder"
mkdir -p "$HOME_D"
run_install --
check "placeholder: latest installed" "$(sed -n 2p "$(bin_path)")" "# fake cc-switch-web $V2"

# 14
setup_case journal
run_install STUB_CAP_VERSION=9.9.9-web.9 CC_SWITCH_WEB_HEALTH_TIMEOUT=2 --
check "a service that does not come up names the journal command: exit code" "$RC" "1"
check_has "no health: journal command" "$(cat "$ERR")" "journalctl --user -u cc-switch-web"

# 15-18: broken archives whose SHA256SUMS line is right still install nothing
# repack <mode>: replaces the V2 archive by a broken one and fixes its sum
repack() {
  local dir="$REL/v$V2" n="cc-switch-web-$V2-linux-x86_64" w="$C/repack"
  local a="$dir/$n.tar.gz"
  mkdir -p "$w"
  tar -xzf "$a" -C "$w"
  case "$1" in
    no-install) mv "$w/$n/install.sh" "$w/gone" ;;
    no-dist) mv "$w/$n/dist-web" "$w/gone" ;;
    unsafe)
      echo x >"$w/evil"
      tar -czf "$a" -C "$w" --transform 's|^evil$|../x|' "$n" evil
      ;;
    truncated) head -c 100 "$a" >"$w/cut" && mv "$w/cut" "$a" ;;
  esac
  if [ "$1" != unsafe ] && [ "$1" != truncated ]; then tar -czf "$a" -C "$w" "$n"; fi
  (cd "$dir" && sha256sum "$n.tar.gz") >"$dir/SHA256SUMS"
}
for mode_want in "no-install:incomplete (no install.sh)" "no-dist:incomplete (no dist-web)" "unsafe:unsafe paths" "truncated:could not read"; do
  mode="${mode_want%%:*}"
  want_text="${mode_want#*:}"
  setup_case "broken-$mode"
  run_install --
  old_bin="$(cat "$(bin_path)")"
  old_web="$(cat "$(inst_path)/dist-web/index.html")"
  make_release "$REL" "$V2" x86_64
  set_latest "$REL" "$V2"
  repack "$mode"
  run_install --
  check "a broken archive ($mode) installs nothing: exit code" "$RC" "1"
  check_has "broken archive ($mode): stderr" "$(cat "$ERR")" "$want_text"
  check "broken archive ($mode): binary untouched" "$(cat "$(bin_path)")" "$old_bin"
  check "broken archive ($mode): dist-web untouched" "$(cat "$(inst_path)/dist-web/index.html")" "$old_web"
done

# a releases/latest answer larger than the pipe buffer, the tag first (DP-252)
setup_case biglatest
{
  printf '{\n  "tag_name": "v%s",\n  "body": "' "$V1"
  head -c 102400 /dev/zero | tr '\0' x
  printf '"\n}\n'
} >"$REL/api/repos/o/r/releases/latest"
run_install --
check "a 100 KiB latest release answer still gives the tag: exit code" "$RC" "0"
check "big answer: latest installed" "$(sed -n 2p "$(bin_path)")" "# fake cc-switch-web $V1"

echo
echo "$passes passed, $fails failed"
[ "$fails" -eq 0 ]
