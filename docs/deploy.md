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
  or missing argument gives an unknown build. The expression
  `$(git diff --quiet HEAD && git rev-parse HEAD)` prints the commit only when no tracked
  file differs from `HEAD`, and nothing otherwise:

  ```bash
  docker build -f Dockerfile.web \
    --build-arg CC_SWITCH_WEB_GIT_SHA=$(git diff --quiet HEAD && git rev-parse HEAD) \
    -t cc-switch-web .
  ```

  The clean-tree guard of the Docker path is that expression in your command; an image
  built by another command (a bare `$(git rev-parse HEAD)` from a dirty tree, a CI
  pipeline) is unguarded and can report `up-to-date` for code the branch does not have.

`GET /api/update/check` asks the GitHub compare API how far the branch is ahead of that
commit (`status` is `up-to-date`, `behind` or `unknown`; `unknown` also covers a commit
that is not on the branch, such as a local build, and a build with commits the branch
lacks). The answer is cached in memory for one hour for every client, so open tabs do not
spend the 60 anonymous requests an hour; `?force=1` asks again. A rate-limit or network
failure is returned as an error and is not cached.

In the console the check runs once, a second after it opens, and never with `force`;
only the "Check for updates" button in Settings → About sends `?force=1`. A build that is
behind gets a dot on Settings and, in About, the number of missing commits, the newest of
them (with "showing N of M" when the list is shorter) and the update commands for systemd
and Docker with copy buttons. A failed check shows the server's error text. Unless
self-update is switched on (next section), the console does not install the update: the
operator runs the commands on the server host. For systemd that is `cd <checkout> && git pull --ff-only && scripts/install-server.sh && systemctl --user daemon-reload && systemctl --user restart cc-switch-web`
(`install-server.sh` installs the binary and rewrites the unit file but does not restart the
unit, so without the last two steps the old process keeps running and the console still says
"behind"). The source is
read from the environment of the server on every request:

| Variable                      | Default                  |
| ----------------------------- | ------------------------ |
| `CC_SWITCH_WEB_UPDATE_API`    | `https://api.github.com` |
| `CC_SWITCH_WEB_UPDATE_REPO`   | `alxwlw/cc-switch-web`   |
| `CC_SWITCH_WEB_UPDATE_BRANCH` | `main`                   |

## Self-update from the console (systemd user unit only, off by default)

When it is on, a build that is behind gets an "Update now" button in Settings → About.
The confirmation names the branch, lists the missing commits and warns that the database
may be migrated. After the click the page follows the stages, survives the restart of the
service and reloads itself when the new build answers.

**The risk, stated plainly.** The server has no authentication (see the top of this file)
and `POST /api/update/install` is an ordinary route of it: **anyone who can reach the
port, that is every member of the tailnet, can start a build on the host and make the
service restart.** They cannot choose what is built: the request body is ignored, the
script runs `git merge --ff-only` of `origin/<branch>` where the branch is only
`CC_SWITCH_WEB_UPDATE_BRANCH` (default `main`) from the server's own environment, and one
update runs at a time (a second request answers `409`). But whoever can push to that
branch of that remote gets their code built and run on the host with your user's rights,
and a build costs minutes of CPU and disk. That is why self-update is opt-in and is not
part of `install-server.sh`. Do not switch it on for a tailnet you do not trust, or for a
branch you do not control.

### Switch it on

Put the settings in a drop-in, not in `cc-switch-web.service`: `install-server.sh`
renders that file from the template on every run and would erase them.

```bash
mkdir -p ~/.config/systemd/user/cc-switch-web.service.d
cat > ~/.config/systemd/user/cc-switch-web.service.d/self-update.conf <<EOF
[Service]
Environment=CC_SWITCH_WEB_SELF_UPDATE=1
Environment=CC_SWITCH_WEB_SOURCE_DIR=$HOME/cc-switch-web
Environment=CC_SWITCH_WEB_BUILD_PATH=$(dirname "$(command -v node)"):$(dirname "$(command -v pnpm)"):$(dirname "$(command -v cargo)")
EOF
systemctl --user daemon-reload
systemctl --user restart cc-switch-web
```

- `CC_SWITCH_WEB_SELF_UPDATE=1` is the switch; any other value is off.
- `CC_SWITCH_WEB_SOURCE_DIR` is the absolute path of the git checkout the service was
  installed from (the one that holds `scripts/self-update.sh` and the `dist-web` the unit
  serves). It must be on the update branch with no changed tracked files, and `git fetch
  origin` must work without a prompt (an ssh key or a stored token).
- `CC_SWITCH_WEB_BUILD_PATH` is a colon-separated list of directories put in front of
  `PATH` for the build. The update runs in a transient unit (`systemd-run --user`), which
  gets the user manager's `PATH`, not your login shell's, so `pnpm`, `node` and `cargo`
  are usually not found there. The command above takes them from your current shell. With
  mise the directories are the real install ones (`dirname "$(mise which node)"`,
  `dirname "$(mise which pnpm)"`), not the shim directory. `cargo` from rustup lives in
  `~/.cargo/bin`. Missing tools are reported by name before anything is touched.
- The unit already carries `HOST`, `PORT` and `WEB_DIST`; the server forwards those, the
  build path and `XDG_STATE_HOME` to the update unit. The health check polls
  `http://$HOST:$PORT/api/capabilities` (the new `buildSha` must show up) and
  `http://$HOST:$PORT/api/health` (must answer 2xx); `HOST` empty or `0.0.0.0` means
  `127.0.0.1`.
  `BIN_DIR` is **not** forwarded: the script always installs to
  `~/.local/bin/cc-switch-web`, where `install-server.sh` puts it by default. A service
  installed to another `BIN_DIR`, or one that runs another binary path, cannot be updated
  this way: its binary would not change, the health check would time out and the update
  would roll back.
- **User unit only.** `INVOCATION_ID` is also set in a system unit, but the update is
  started with `systemd-run --user`, which a system service cannot reach. In Docker (no
  `INVOCATION_ID`) the install route answers `409` and the console shows the Docker
  commands of the update check: rebuild the image with the build argument, as above.

`GET /api/capabilities` reports `selfUpdate: true` only when every condition holds; the
console shows the button from that flag. With self-update off, no button appears, the
manual commands stay in About and `POST /api/update/install` answers `409` with the
reason.

### What `scripts/self-update.sh` does

The server starts it as the transient unit `cc-switch-web-update` and it writes its
progress to `${XDG_STATE_HOME:-~/.local/state}/cc-switch-web/update-status.json`
(`GET /api/update/status`; the console polls it every 2 s because the event stream does
not survive the restart). Build output goes to the journal:
`journalctl --user -u cc-switch-web-update`.

1. `checking`: the tools exist, the checkout is on the update branch and no tracked file
   differs from `HEAD` (untracked files such as `.scratch/` are fine), the installed
   binary and `dist-web` exist. A failure here touches nothing.
2. `fetching`: `git fetch origin`, killed after 120 s (`FETCH_TIMEOUT`). If the checkout
   is already at `origin/<branch>` and the running service reports that commit, the run
   ends `done` with "already up to date".
3. `merging`: `git merge --ff-only origin/<branch>`. A branch that cannot fast-forward is
   refused. The checkout is **never reset**: after any later failure it stays on the new
   commit, and the message says so.
4. `building`: `pnpm install --frozen-lockfile`, `pnpm build:web` into `dist-web.new`,
   `cargo build --release --example server` with `CC_SWITCH_WEB_GIT_SHA` set to the new
   commit, `install -m0755` to `cc-switch-web.new`. The live binary and `dist-web` are not
   touched until all of that has succeeded, so a failed compile or a full disk leaves the
   running service as it was (status `failed`, the service is not restarted). The
   half-built `dist-web.new` stays on disk after such a failure until the next run deletes it.
5. `installing`: the old `.prev` copies are deleted, then (all renames inside one
   directory) `cc-switch-web` becomes `cc-switch-web.prev` and the new binary takes its
   place; `dist-web` becomes `dist-web.prev` and `dist-web.new` takes its place.
6. `restarting`: `systemctl --user daemon-reload`, `systemctl --user restart
   cc-switch-web`, then up to 60 s (`HEALTH_TIMEOUT`) of polling until `/api/capabilities`
   reports the new `buildSha` **and** `/api/health` answers 2xx. Both are needed: a server
   whose startup failed (a database newer than the binary) serves in init-error mode,
   where `/api/capabilities` still answers with the build's own sha but `/api/health`,
   like every other `/api` route, is `503`. Only when both hold does the run end `done`.
7. A failure after step 5 (the process exits at start, the sha never shows up, SIGTERM)
   starts `rolling-back`: the `.prev` files are copied back (the copies stay), the
   service restarts and must report the previous `buildSha` with a working `/api/health`.
   Then `rolled-back`, with the reason; or `rollback-failed` when the old build does not
   work either. The message of `rollback-failed` names the `.prev` files and points at the
   database section below as one possible cause.

Final stages are `done`, `failed`, `rolled-back` and `rollback-failed`. `failed` also
covers an interrupted run (SIGTERM). About shows the outcome of an earlier `failed`,
`rolled-back` or `rollback-failed` run when it opens.

Only one `.prev` generation exists: the next update replaces it, so after a second update
`.prev` holds the build before it, not the original one.

If `scripts/cc-switch-web.service.template` changed in the update, the final message of
`done` carries a warning. The script only runs `daemon-reload`; it does not render the
unit. Run `scripts/install-server.sh` by hand (then `systemctl --user daemon-reload &&
systemctl --user restart cc-switch-web`). The drop-in survives that.

### The database and an update

This is what the code does (`src-tauri/src/database`), not a promise of the script. The
script makes no backup of its own.

- **A backup is made before a migration, by the new binary at its start.**
  `Database::init` (`database/mod.rs:128-141`) takes a copy when the database exists and
  its `user_version` is above 0 and below the build's `SCHEMA_VERSION` (23 today). The copy
  is `<data dir>/backups/db_backup_YYYYMMDD_HHMMSS.db` (`~/.cc-switch-web/backups/` for the
  unit), made with the SQLite backup API and checked with an integrity check
  (`database/backup.rs:478-564`). Only then do the migrations run. An update without a
  schema change creates no such file. If the copy fails, the server logs a warning and
  migrates anyway. The newest `backup_retain_count` files (10 by default, the "Backup Retention"
  setting next to the backup list) are kept, older `.db` files in that directory are deleted
  (`database/backup.rs:604`). The same directory holds the other database backups, so
  copy the file you need somewhere safe soon after an update.
- **An older binary does not open a newer database.**
  `Database::stored_user_version_exceeds_supported` (`database/mod.rs:175`) finds a
  `user_version` above the binary's `SCHEMA_VERSION`; the bootstrap
  (`bootstrap.rs:164`) then refuses to touch the file and the server starts in
  init-error mode (`examples/server.rs:209`): the console shows "database version is too
  new", `/api/init-status` reports it and every other `/api/*` route answers `503`. There
  are no downgrade migrations.
- **A rollback after a migration does not give you back a working service, and the script
  says so.** If the new build migrated the database and then crashed, or came up in
  init-error mode, the health check (which includes `/api/health`) fails. The rollback puts
  the previous binary back; it cannot open the migrated database and starts in init-error
  mode too, so its health check fails as well and the status is `rollback-failed`, with a
  message that names the database as a possible cause. The same goes for a new build that
  starts in init-error mode on its own: it is a failed update, never `done`. The way out is
  the database restore below. (The same check also stops "already up to date" from being
  reported for a service that is in init-error mode.)
- **Check for a migration before you click**: `git fetch origin && git diff HEAD
  origin/<branch> -- src-tauri/src/database` shows a changed `SCHEMA_VERSION` or
  schema; if so, take your own backup first:
  `sqlite3 ~/.cc-switch-web/cc-switch.db ".backup ~/cc-switch-before-update.db"`.
- **Restore the database** (service stopped; the file to use is the newest
  `db_backup_*.db` from before the update, or your own copy):

  ```bash
  systemctl --user stop cc-switch-web
  cd ~/.cc-switch-web
  mv cc-switch.db cc-switch.db.too-new
  rm -f cc-switch.db-wal cc-switch.db-shm
  cp -p backups/db_backup_<stamp>.db cc-switch.db
  ```

  Then put the previous binary and `dist-web` back with the recipe below. Everything
  written between the backup and the stop is lost; `cc-switch.db.too-new` keeps it.

### Go back by hand

`.prev` holds the build that was running before the last update (the binary
`cc-switch-web.prev` next to the installed one, `dist-web.prev` next to `dist-web`). To
return to it, after a `done` you regret, or after `rollback-failed`:

```bash
BIN="$HOME/.local/bin/cc-switch-web"
DIST="$HOME/cc-switch-web/dist-web"   # WEB_DIST of the unit
STATUS="${XDG_STATE_HOME:-$HOME/.local/state}/cc-switch-web/update-status.json"

cp -p "$BIN.prev" "$BIN.restore" && mv -f "$BIN.restore" "$BIN"
rm -rf "$DIST.new" && mv "$DIST" "$DIST.new" && cp -a "$DIST.prev" "$DIST" && rm -rf "$DIST.new"
rm -f "$STATUS"
systemctl --user restart cc-switch-web
```

The last two lines matter after `rollback-failed`: the console keeps showing that
outcome, and hides the update button, for as long as the status file exists (a started
update clears it). Removing it brings the button back. `systemctl --user restart` is
enough; if the unit file itself changed, run `daemon-reload` first. If `.prev` is
damaged or missing, rebuild the old commit instead: `git -C <checkout> checkout
<fromSha>` (the sha is in the status message), `scripts/install-server.sh`, restart, and
`git checkout <branch>` afterwards, because the next update insists on the update branch.

The checkout is not touched by this recipe: it stays on the commit of the update. The
console will say "behind" again, because the running build is older than the branch; a
click then rebuilds that same commit, so fix a bad commit on the branch first.

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
