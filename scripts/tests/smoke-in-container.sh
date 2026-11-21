#!/usr/bin/env bash
# Smoke test of a release archive in a clean distro container. Not a test suite: it needs root
# inside a container and the archive on disk.
#   docker run --rm -v "$PWD:/w" debian:12 bash /w/scripts/tests/smoke-in-container.sh <tag>
# Expects /w/artifacts/<tag>/ with cc-switch-web-<version>-linux-<arch>.tar.gz and SHA256SUMS.
# Installs with scripts/install.sh as an unprivileged user with passwordless sudo, starts the
# server from the command the installer prints (no systemd in a container) and probes it.
set -euo pipefail

TAG="${1:?usage: smoke-in-container.sh <tag>}"
VERSION="${TAG#v}"
PORT=18766
LOG=/tmp/server.log
INSTALL_LOG=/tmp/install.log

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends curl ca-certificates sudo >/dev/null

useradd --create-home --shell /bin/bash tester
echo 'tester ALL=(ALL) NOPASSWD:ALL' >/etc/sudoers.d/tester
chmod 440 /etc/sudoers.d/tester

fail() {
  echo "SMOKE FAIL: $*" >&2
  if [ -f "$INSTALL_LOG" ]; then
    echo "--- install log" >&2
    cat "$INSTALL_LOG" >&2
  fi
  if [ -f "$LOG" ]; then
    echo "--- server log" >&2
    cat "$LOG" >&2
  fi
  exit 1
}

# 1. install as tester; the installer reads the archive from the artifacts directory
# shellcheck disable=SC2024 # root writes the log on purpose; tester only runs the installer
sudo -u tester -H env CC_SWITCH_WEB_DOWNLOAD_BASE=file:///w/artifacts \
  bash /w/scripts/install.sh --version "$TAG" >"$INSTALL_LOG" 2>&1 || fail "install.sh exited non-zero"
cat "$INSTALL_LOG"

# 2. the installer prints the foreground command (HOST=... PORT=... cc-switch-web) when there is no systemd
cmd="$(grep -E '^  HOST=' "$INSTALL_LOG" | head -n 1 | sed 's/^  //')" || true
[ -n "$cmd" ] || fail "install.sh printed no foreground command"

# 3. start the server in the background as tester, with the printed variables
: >"$LOG"
chown tester "$LOG"
sudo -u tester -H bash -c "$cmd >>'$LOG' 2>&1 &"

# 4. wait up to 60 s for the version we installed
ok=0
for _ in $(seq 1 60); do
  if curl -fsS --max-time 2 "http://127.0.0.1:$PORT/api/health" 2>/dev/null | grep -q '"status":"ok"'; then
    ok=1
    break
  fi
  sleep 1
done
[ "$ok" = 1 ] || fail "/api/health did not report ok within 60 s"

health="$(curl -fsS --max-time 5 "http://127.0.0.1:$PORT/api/health")" || fail "/api/health request failed"
caps="$(curl -fsS --max-time 5 "http://127.0.0.1:$PORT/api/capabilities")" || fail "/api/capabilities request failed"
root="$(curl -fsS --max-time 5 "http://127.0.0.1:$PORT/")" || fail "GET / failed"

grep -q '"status":"ok"' <<<"$health" || fail "/api/health lacks \"status\":\"ok\": $health"
grep -qE "\"webVersion\"[[:space:]]*:[[:space:]]*\"${VERSION//./\\.}\"" <<<"$caps" ||
  fail "/api/capabilities lacks \"webVersion\":\"$VERSION\": $caps"
grep -q '<div id="root"' <<<"$root" || fail "GET / lacks <div id=\"root\""

echo "SMOKE OK: $TAG on $(. /etc/os-release && echo "$PRETTY_NAME") ($(uname -m))"
