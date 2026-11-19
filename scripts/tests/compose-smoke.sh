#!/usr/bin/env bash
# Smoke test of deploy/compose.yml with a built image, as the current user. Not a *.test.sh: it needs
# Docker and the host port 18766, so run-all.sh does not pick it up. CI runs it in the `docker` job.
#   bash scripts/tests/compose-smoke.sh <image> [<version>]
# <version> is the fork version the image was built with (CC_SWITCH_WEB_VERSION); without it the
# version of src-tauri/Cargo.toml plus "-web.0" is expected, which is what a non-release build gets.
# Two passes. The default config: the console answers on 127.0.0.1:18766, serves the UI and reports
# the version, nothing but 127.0.0.1 is bound on the port, the container runs as your UID with no
# /etc/passwd entry and created ~/.cc-switch-web owned by you, and Docker calls it healthy. The
# Tailscale variant of compose.yml (HOST an address of this machine that is not loopback, plus
# TRUSTED_TAILNET=1): the same, listening on that address only, and the image's HEALTHCHECK still
# reaches healthy. The address is the host's own first non-loopback IPv4, so a runner has one too.
set -euo pipefail

IMAGE="${1:?usage: compose-smoke.sh <image> [<version>]}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VERSION="${2:-}"
if [ -z "$VERSION" ]; then
  VERSION="$(sed -nE 's/^version *= *"([^"]+)".*/\1/p' "$ROOT/src-tauri/Cargo.toml" | head -n 1)-web.0"
fi
PORT=18766
PROJECT=ccsw-compose-smoke

TMP=""
OVERRIDE=""
started=0

compose() {
  local files=(-f "$ROOT/deploy/compose.yml")
  if [ -n "$OVERRIDE" ]; then files+=(-f "$OVERRIDE"); fi
  HOME="$TMP" CC_SWITCH_WEB_IMAGE="$IMAGE" \
    docker compose -p "$PROJECT" "${files[@]}" --env-file "$TMP/.env" "$@"
}

# Takes down what the current pass started and removes its directory.
end_pass() {
  if [ "$started" -eq 1 ]; then
    compose down --volumes --timeout 10 >&2 || true
    started=0
  fi
  if [ -n "$TMP" ]; then rm -rf "$TMP"; fi
  TMP=""
  OVERRIDE=""
}

cleanup() {
  local rc=$?
  if [ "$rc" -ne 0 ] && [ "$started" -eq 1 ]; then
    echo "--- container logs" >&2
    compose logs --no-color >&2 || true
  fi
  end_pass
  exit "$rc"
}
trap cleanup EXIT

fail() {
  echo "COMPOSE SMOKE FAIL: $*" >&2
  exit 1
}

# The Local Address column of every listener on the port, one per line.
listeners() {
  ss -ltn "sport = :$PORT" | awk 'NR > 1 { print $4 }'
}

# smoke_pass <name> <address> <trusted>: one compose up with the console on <address>.
# <trusted> = 1 adds TRUSTED_TAILNET=1 and puts HOST=<address> into the compose environment.
smoke_pass() {
  local name="$1" addr="$2" trusted="$3" base="http://$2:$PORT"
  echo "--- pass: $name ($addr)" >&2
  TMP="$(mktemp -d)"
  # The container runs as this user and must be able to write to the directory.
  chmod 755 "$TMP"
  printf 'CCS_UID=%s\nCCS_GID=%s\n' "$(id -u)" "$(id -g)" >"$TMP/.env"
  if [ "$trusted" = 1 ]; then
    OVERRIDE="$TMP/override.yml"
    printf 'services:\n  cc-switch-web:\n    environment:\n      HOST: "%s"\n      TRUSTED_TAILNET: "1"\n' "$addr" >"$OVERRIDE"
  fi

  # Anything already listening on the port would make every check below pass for the wrong server,
  # or make the container fail to bind.
  if [ -n "$(listeners)" ]; then
    fail "something already listens on port $PORT ($(listeners | tr '\n' ' ')); stop it first"
  fi

  started=1
  compose up -d >&2 || fail "docker compose up failed"

  local up=0 _
  for _ in $(seq 1 60); do
    if curl -fsS -o /dev/null --max-time 2 "$base/api/health" 2>/dev/null; then
      up=1
      break
    fi
    sleep 1
  done
  [ "$up" -eq 1 ] || fail "$name: no answer from $base/api/health within 60 s"

  local caps page owner
  caps="$(curl -fsS --max-time 5 "$base/api/capabilities")" || fail "$name: GET /api/capabilities failed"
  case "$caps" in
    *"\"webVersion\":\"$VERSION\""*) ;;
    *) fail "$name: capabilities do not report webVersion $VERSION: $caps" ;;
  esac

  page="$(curl -fsS --max-time 5 "$base/")" || fail "$name: GET / failed"
  case "$page" in
    *'<div id="root"'*) ;;
    *) fail "$name: GET / is not the console (no <div id=\"root\"): ${page:0:200}" ;;
  esac

  # Only the address the compose file names is bound, never the wildcard.
  local bound
  bound="$(listeners | sort -u | tr '\n' ' ')"
  [ "$bound" = "$addr:$PORT " ] || fail "$name: port $PORT is bound on '$bound', expected only $addr:$PORT"

  [ -d "$TMP/.cc-switch-web" ] || fail "$name: $TMP/.cc-switch-web was not created"
  owner="$(stat -c %u "$TMP/.cc-switch-web")"
  [ "$owner" = "$(id -u)" ] || fail "$name: $TMP/.cc-switch-web is owned by uid $owner, expected $(id -u)"

  # The image's HEALTHCHECK probes the address the server listens on (first check after 15 s).
  local cid health=""
  cid="$(compose ps -q cc-switch-web)"
  [ -n "$cid" ] || fail "$name: no container id from compose ps"
  for _ in $(seq 1 60); do
    health="$(docker inspect -f '{{.State.Health.Status}}' "$cid")"
    [ "$health" != healthy ] || break
    sleep 2
  done
  [ "$health" = healthy ] || fail "$name: the container is '$health' after 120 s, expected healthy (HEALTHCHECK probes the wrong address?)"

  echo "compose smoke ok ($name): $IMAGE, webVersion $VERSION, bound only on $addr:$PORT, healthy, config dir owned by uid $owner" >&2
  end_pass
}

smoke_pass "default" 127.0.0.1 0

# The first non-loopback IPv4 of this machine, as the Tailscale variant would name it.
host_ip="$(ip -4 -o addr show scope global 2>/dev/null | awk '{ split($4, a, "/"); print a[1]; exit }')"
if [ -z "$host_ip" ]; then
  if [ -n "${CI:-}" ]; then fail "no non-loopback IPv4 address on this machine"; fi
  echo "compose smoke: no non-loopback IPv4 address here, the tailnet variant is skipped" >&2
else
  smoke_pass "tailnet variant" "$host_ip" 1
fi
echo "compose smoke ok: $IMAGE, webVersion $VERSION"
