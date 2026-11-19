#!/usr/bin/env bash
# Smoke test of deploy/compose.yml with a built image, as the current user. Not a *.test.sh: it needs
# Docker and the host port 18766, so run-all.sh does not pick it up. CI runs it in the `docker` job.
#   bash scripts/tests/compose-smoke.sh <image> [<version>]
# <version> is the fork version the image was built with (CC_SWITCH_WEB_VERSION); without it the
# version of src-tauri/Cargo.toml plus "-web.0" is expected, which is what a non-release build gets.
# Checks that the console answers on 127.0.0.1:18766, serves the UI and reports the version, and that
# the container, run as your UID with no /etc/passwd entry, created ~/.cc-switch-web owned by you.
set -euo pipefail

IMAGE="${1:?usage: compose-smoke.sh <image> [<version>]}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${2:-}"
if [ -z "$VERSION" ]; then
  VERSION="$(sed -nE 's/^version *= *"([^"]+)".*/\1/p' "$ROOT/src-tauri/Cargo.toml" | head -n 1)-web.0"
fi
BASE=http://127.0.0.1:18766
PROJECT=ccsw-compose-smoke

TMP="$(mktemp -d)"
# The container runs as this user and must be able to write to the directory.
chmod 755 "$TMP"
printf 'CCS_UID=%s\nCCS_GID=%s\n' "$(id -u)" "$(id -g)" >"$TMP/.env"

compose() {
  HOME="$TMP" CC_SWITCH_WEB_IMAGE="$IMAGE" \
    docker compose -p "$PROJECT" -f "$ROOT/deploy/compose.yml" --env-file "$TMP/.env" "$@"
}

started=0
cleanup() {
  local rc=$?
  if [ "$rc" -ne 0 ] && [ "$started" -eq 1 ]; then
    echo "--- container logs" >&2
    compose logs --no-color >&2 || true
  fi
  if [ "$started" -eq 1 ]; then
    compose down --volumes --timeout 10 >&2 || true
  fi
  rm -rf "$TMP"
  exit "$rc"
}
trap cleanup EXIT

fail() {
  echo "COMPOSE SMOKE FAIL: $*" >&2
  exit 1
}

# Anything already listening on the port would make every check below pass for the wrong server, or
# make the container fail to bind.
if (exec 3<>/dev/tcp/127.0.0.1/18766) 2>/dev/null; then
  fail "something already listens on 127.0.0.1:18766; stop it first"
fi

started=1
compose up -d >&2 || fail "docker compose up failed"

up=0
for _ in $(seq 1 60); do
  if curl -fsS -o /dev/null --max-time 2 "$BASE/api/health" 2>/dev/null; then
    up=1
    break
  fi
  sleep 1
done
[ "$up" -eq 1 ] || fail "no answer from $BASE/api/health within 60 s"

caps="$(curl -fsS --max-time 5 "$BASE/api/capabilities")" || fail "GET /api/capabilities failed"
case "$caps" in
  *"\"webVersion\":\"$VERSION\""*) ;;
  *) fail "capabilities do not report webVersion $VERSION: $caps" ;;
esac

page="$(curl -fsS --max-time 5 "$BASE/")" || fail "GET / failed"
case "$page" in
  *'<div id="root"'*) ;;
  *) fail "GET / is not the console (no <div id=\"root\"): ${page:0:200}" ;;
esac

[ -d "$TMP/.cc-switch-web" ] || fail "$TMP/.cc-switch-web was not created"
owner="$(stat -c %u "$TMP/.cc-switch-web")"
[ "$owner" = "$(id -u)" ] || fail "$TMP/.cc-switch-web is owned by uid $owner, expected $(id -u)"

echo "compose smoke ok: $IMAGE, webVersion $VERSION, config dir owned by uid $owner"
