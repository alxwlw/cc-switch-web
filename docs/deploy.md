# Deploy (Tailscale-only)

Server has no in-app auth by design. Reachable only inside your Tailnet.

## Install on the host

```bash
./scripts/install-server.sh
systemctl --user daemon-reload
systemctl --user enable --now cc-switch-web
```

`install-server.sh` builds the console (`pnpm build:web`), the release
server binary, installs it to `~/.local/bin/cc-switch-web` and writes a
systemd user unit bound to the current `tailscale ip -4` on port `18766`.

Env overrides: `HOST`, `PORT`, `BIN_DIR`, `UNIT_DIR`, `CC_SWITCH_CONFIG_DIR`.

## Data isolation

The fork never touches `~/.cc-switch` unless you point it there:

- default data dir: `~/.cc-switch-web` (`CC_SWITCH_CONFIG_DIR`)
- live tool configs (`~/.claude/settings.json`, `~/.codex/config.toml`, …)
  are the real ones — switching providers rewrites them, same as upstream.

## Check

```bash
curl http://$(tailscale ip -4):18766/api/health
curl http://$(tailscale ip -4):18766/api/capabilities
systemctl --user status cc-switch-web
```

Console: `http://<tailscale-ip>:18766/` in the browser (no login).

## Sync upstream

See `docs/upstream.lock`. `upstream-main` is a fast-forward-only mirror of
`farion1231/cc-switch`; product work lives on `main` behind adapters
(`src-tauri/src/web_api.rs`, `examples/server.rs`, `src/lib/api/adapter.ts`,
`src/lib/web-shims/`, `vite.config.web.mts`, `scripts/*`).
