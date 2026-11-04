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

Env overrides: `HOST`, `PORT`, `BIN_DIR`, `UNIT_DIR`. `CC_SWITCH_CONFIG_DIR` is read
by the script only for its closing message: the unit comes from
`scripts/cc-switch-web.service.template`, which always carries
`CC_SWITCH_CONFIG_DIR=%h/.cc-switch-web` (and `Restart=always`, `RestartSec=3`). Every
re-run of `install-server.sh` renders the unit from the template again, so an edit that
removed that line is undone (see "Data directory" below).

## Build commit

The console compares the running build with the fork's branch, so the binary carries the
commit it was built from (`buildSha` in `GET /api/capabilities`, `null` when unknown). It
is the compile-time env `CC_SWITCH_WEB_GIT_SHA`:

- `scripts/install-server.sh` sets it to `git rev-parse HEAD` of its checkout only when
  that is a git checkout root and no tracked file differs from `HEAD` (untracked files do
  not count). Otherwise it unsets the variable, prints a warning for tracked changes, and
  the build reports an unknown commit. Commit before installing to get a known build.
- Docker: the builder stage has no git, so pass the commit as a build argument. An empty
  or missing argument gives an unknown build:

  ```bash
  docker build -f Dockerfile.web \
    --build-arg CC_SWITCH_WEB_GIT_SHA=$(git rev-parse HEAD) \
    -t cc-switch-web .
  ```

`GET /api/update/check` asks the GitHub compare API how far the branch is ahead of that
commit (`status` is `up-to-date`, `behind` or `unknown`; `unknown` also covers a commit
that is not on the branch, such as a local build, and a build with commits the branch
lacks). The answer is cached in memory for one hour for every client, so open tabs do not
spend the 60 anonymous requests an hour; `?force=1` asks again. A rate-limit or network
failure is returned as an error and is not cached. The source is read from the
environment of the server on every request:

| Variable                      | Default                  |
| ----------------------------- | ------------------------ |
| `CC_SWITCH_WEB_UPDATE_API`    | `https://api.github.com` |
| `CC_SWITCH_WEB_UPDATE_REPO`   | `alxwlw/cc-switch-web`   |
| `CC_SWITCH_WEB_UPDATE_BRANCH` | `main`                   |

## Data isolation

The fork does not keep its database in `~/.cc-switch` unless you point it there, but a few
files of the device always live there (see the second list below):

- default data dir of the unit: `~/.cc-switch-web` (`CC_SWITCH_CONFIG_DIR`)
- live tool configs (`~/.claude/settings.json`, `~/.codex/config.toml`, …)
  are the real ones — switching providers rewrites them, same as upstream.

What follows the data directory (`<data dir>` below; `config::get_app_config_dir()`):

- `cc-switch.db`, `model-pricing.json`, `config.json`
- the server log (`<data dir>/logs`)
- `backups/` (database and OpenClaw / Hermes config backups), `skills/` and
  `skill-backups/` (when skills are stored in the cc-switch directory), `agy/`
- the OAuth account stores (`codex_oauth_auth.json`, the Kimi account file) and `codex_history_migration` backups

What stays in `~/.cc-switch` of the server's home whatever the data directory is (device
state of upstream, not moved on purpose):

- `settings.json`
- `live-state.json` (the mode / takeover state of each client)
- device backups: `backups/live-first-write/`, `backups/proxy-live-backup/` and the
  environment-variable backups of the env manager, all under `~/.cc-switch/backups/`
- the Codex device files `codex-*.json` (catalog and login history, client
  acknowledgements, the official-models cache and the login stash)

So `~/.cc-switch` can appear on a host that runs only the fork, and a desktop cc-switch
on the same account shares those files with it.

## Data directory

`GET /api/app-config-dir` (and Settings → Data in the console) shows the directory the
running process uses (`effective`), where it comes from (`source`) and the value that will
apply after the next start (`override`). The order is fixed: the environment variable
`CC_SWITCH_CONFIG_DIR`, then the override file, then `~/.cc-switch`.

**Env-locked (the default).** The unit and `Dockerfile.web` set `CC_SWITCH_CONFIG_DIR`.
The console shows the directory and the source `env` read-only (no browse, no reset) and
`PUT /api/app-config-dir` answers `409`. Nothing about the directory changes from the UI.

**Override.** Without the variable, Settings → Data takes a text path (`~/…` or absolute;
the browse button does not exist on the server). Saving writes
`~/.config/cc-switch-web/app_paths.json` (`$XDG_CONFIG_HOME/cc-switch-web/` when that is
set), key `app_config_dir_override`, atomically. The running process keeps its directory
until it restarts: the console then offers "Restart". The server refuses a path (`400`,
the reason goes into the toast) that is relative, is `~user`, is a file, cannot be written
to, or holds a `cc-switch.db` that is not SQLite or has a newer schema than this build. A
missing directory is created. An empty path removes the override (back to
`~/.cc-switch` after the restart).

**The database is not copied.** The new directory starts with what is in it: an empty
directory gives an empty database (and the old directory stays untouched). To move your
data, stop the service and copy the files first (`sqlite3 <old>/cc-switch.db ".backup
'<new>/cc-switch.db'"`, plus `model-pricing.json`), or point the override at the
directory you already use.

**Moving a unit from env-locked to override without landing on `~/.cc-switch`.** With the
variable set the UI cannot write the file, and with the variable removed and no file the
server falls back to `~/.cc-switch`, which on a shared account may be the desktop
app's database. So the order is:

1. Create the directory you want and put the data in it. To keep the current data, use
   `~/.cc-switch-web` itself.
2. Write the file by hand (`~/…` also works in it; an absolute path is clearer):

   ```bash
   mkdir -p ~/.config/cc-switch-web
   printf '{"app_config_dir_override": "%s"}\n' "$HOME/.cc-switch-web" \
     > ~/.config/cc-switch-web/app_paths.json
   ```

3. Remove the `Environment=CC_SWITCH_CONFIG_DIR=…` line from
   `~/.config/systemd/user/cc-switch-web.service`, then
   `systemctl --user daemon-reload && systemctl --user restart cc-switch-web`.
4. Check `GET /api/app-config-dir`: `source` is `override`, `effective` is your path.

A directory that was deleted after the file was written is ignored at start with a warning
in the log, and the server uses `~/.cc-switch`: look at `effective` before you trust
the data. `./scripts/install-server.sh` puts the variable back into the unit every time it
runs (the template carries it), which returns the unit to env-locked mode.

**Docker.** `Dockerfile.web` sets `CC_SWITCH_CONFIG_DIR=/home/ccswitch/.cc-switch-web`, so
the container is env-locked. To use the override, drop that variable from your image or
`docker run` and mount `/home/ccswitch/.config/cc-switch-web` as a volume, or the file is
lost with the container. The image does not set `CC_SWITCH_WEB_SUPERVISED`.

**Bad override.** The PUT refuses a directory whose database is newer than this build,
but a hand-edited file, or a database upgraded by another app after the PUT, can still
point at one. The server then starts in init-error mode: the console shows the error
screen ("Database version is too new" with the file path) and `/api/health` answers
`503`. The way out is to delete `~/.config/cc-switch-web/app_paths.json` (or fix the
file) and restart the unit by hand (`systemctl --user restart cc-switch-web`; the
console has no restart button in this mode); the server returns to `~/.cc-switch`.

## Restart from the console

`POST /api/restart` (the "Restart" button) stops the server through the same path as
`SIGTERM` and exits with code **75** (`EX_TEMPFAIL`):

1. Every client in takeover mode (Claude Code, Codex, …) is pointed back at its direct
   provider config and the local proxy stops (bounded to 8 s). The mode itself is kept in
   `~/.cc-switch/live-state.json`, so the new process attaches the clients to the proxy
   again at start. The restored file is rebuilt from the direct provider, not copied back
   from a snapshot taken before the takeover: keys the file has besides the provider's are
   kept, keys the takeover removed come back at the end (seen:
   `ANTHROPIC_MODEL` of `~/.claude/settings.json` ends up last), and a value that differs
   from the provider's (a hand edit included) is replaced by the provider's. This state
   lasts only while the server is down: after a restart the new process attaches the
   clients again; after a plain stop (`SIGTERM`, `systemctl stop`) it stays. Restart and
   `SIGTERM` behave the same, and this is not new: `main` does the same.
2. Open connections get up to 5 s to drain. A console tab keeps its event stream open, so
   with a browser open the process exits about 5 s after the click, then `RestartSec=3`
   of the unit: expect about 8-10 s.

The process cannot start itself again, so it only restarts when something supervises it:

- systemd: detected through `INVOCATION_ID`, which systemd sets for every unit. The unit
  has `Restart=always`; code 75 would also restart under `Restart=on-failure`, which
  plain exit 0 (a clean stop) would not. That is why the code is 75 and not 0.
- anything else (Docker, a runit script, …): set `CC_SWITCH_WEB_SUPERVISED=1` **only
  together with a restart policy** (`restart: unless-stopped` / `on-failure`); the
  variable is your promise. Without a restart policy the button turns the server off.

Without a supervisor the route answers `409` ("restart the server manually") and nothing
happens. The route needs a JSON content type (`415` otherwise), and a second click while
the shutdown runs answers `202` again without a second shutdown. After the `202` the
console polls `/api/health` once a second, up to 60 s, and reloads the page when the
`instanceId` changed or the new process answers `503` (init-error); after 60 s it shows
a toast and does not reload.

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
