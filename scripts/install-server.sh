#!/usr/bin/env bash
# Install cc-switch-web headless server (Tailscale-only, no in-app auth).
#
# Env overrides:
#   HOST (default: `tailscale ip -4`)  PORT (default: 18766)
#   BIN_DIR (default: ~/.local/bin)    UNIT_DIR (default: ~/.config/systemd/user)
#   CC_SWITCH_CONFIG_DIR (default: ~/.cc-switch-web)
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
