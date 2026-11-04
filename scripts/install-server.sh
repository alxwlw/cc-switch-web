#!/usr/bin/env bash
# Install cc-switch-web headless server (Tailscale-only, no in-app auth).
#
# Env overrides:
#   HOST (default: `tailscale ip -4`)  PORT (default: 18766)
#   BIN_DIR (default: ~/.local/bin)    UNIT_DIR (default: ~/.config/systemd/user)
#   CC_SWITCH_CONFIG_DIR (default: ~/.cc-switch-web)
#
# The build embeds CC_SWITCH_WEB_GIT_SHA (the checkout's HEAD) so the console can
# compare the running server with the fork's branch. It is set only for a clean
# checkout (tracked files; untracked ones do not count) whose root is this
# directory; otherwise it is unset and the server reports an unknown build.

# Prints the SHA to embed in the build, or nothing (with a warning when the
# checkout has tracked changes). Usage: resolve_build_sha <checkout root>
resolve_build_sha() {
  local root top sha
  root="$(cd "$1" && pwd -P)" || return 0
  # a plain directory (tarball, copy) inside some other repository is not a checkout of its own
  top="$(git -C "$root" rev-parse --show-toplevel 2>/dev/null)" || return 0
  [ "$top" = "$root" ] || return 0
  sha="$(git -C "$root" rev-parse HEAD 2>/dev/null)" || return 0
  if ! git -C "$root" diff --quiet HEAD 2>/dev/null; then
    echo "warning: tracked files differ from HEAD; building without CC_SWITCH_WEB_GIT_SHA (the console will show an unknown build)" >&2
    return 0
  fi
  printf '%s\n' "$sha"
}

# Sourced by a test: only the function above is wanted.
if [ "${BASH_SOURCE[0]}" != "$0" ]; then
  return 0
fi

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"
UNIT_DIR="${UNIT_DIR:-$HOME/.config/systemd/user}"
PORT="${PORT:-18766}"
HOST="${HOST:-$(tailscale ip -4 2>/dev/null | head -1)}"
export CC_SWITCH_CONFIG_DIR="${CC_SWITCH_CONFIG_DIR:-$HOME/.cc-switch-web}"

if [ -z "$HOST" ]; then
  echo "error: cannot determine Tailscale IP (is tailscaled running?)" >&2
  exit 1
fi

build_sha="$(resolve_build_sha "$ROOT")"
if [ -n "$build_sha" ]; then
  export CC_SWITCH_WEB_GIT_SHA="$build_sha"
  echo "==> build commit $build_sha"
else
  unset CC_SWITCH_WEB_GIT_SHA
  echo "==> build commit unknown (CC_SWITCH_WEB_GIT_SHA not set)"
fi

echo "==> frontend (pnpm build:web)"
(cd "$ROOT" && pnpm install && pnpm build:web)

echo "==> server (cargo build --release --example server)"
(cd "$ROOT/src-tauri" && cargo build --release --example server)

echo "==> install binary to $BIN_DIR/cc-switch-web"
mkdir -p "$BIN_DIR"
install -m0755 "$ROOT/src-tauri/target/release/examples/server" "$BIN_DIR/cc-switch-web"

echo "==> systemd user unit to $UNIT_DIR/cc-switch-web.service"
mkdir -p "$UNIT_DIR"
sed -e "s/@TAILSCALE_IP@/$HOST/" -e "s/@PORT@/$PORT/" \
  "$ROOT/scripts/cc-switch-web.service.template" > "$UNIT_DIR/cc-switch-web.service"

echo "done."
echo "enable with: systemctl --user daemon-reload && systemctl --user enable --now cc-switch-web"
echo "console:     http://$HOST:$PORT/  (tailscale only, no login)"
echo "data dir:    $CC_SWITCH_CONFIG_DIR (isolated from ~/.cc-switch)"
