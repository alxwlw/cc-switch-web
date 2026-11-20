# Contributing to cc-switch-web

cc-switch-web is a fork of [farion1231/cc-switch](https://github.com/farion1231/cc-switch) that
runs the app as a web console on a Linux server. Bug reports, fixes and ideas are welcome here.
Problems that also happen in the cc-switch desktop app belong to
[upstream](https://github.com/farion1231/cc-switch/issues).

For a bug or a feature request, open an issue with the
[bug report](https://github.com/alxwlw/cc-switch-web/issues/new?template=bug_report.yml) or
[feature request](https://github.com/alxwlw/cc-switch-web/issues/new?template=feature_request.yml)
form. For a larger change, discuss it in an issue before you write the code.

## Build and run from source

Prerequisites: Node.js 22.12 (`.node-version`), pnpm 10 (the version is pinned by the
`packageManager` field of `package.json`), the Rust channel of `rust-toolchain.toml`, and the
system libraries of Tauri on Linux:

```bash
sudo apt-get install build-essential pkg-config libssl-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev libwebkit2gtk-4.1-dev libsoup-3.0-dev
```

From the repository root:

```bash
pnpm install
pnpm build:web                       # the console, into dist-web/
mkdir -p dist                        # the Tauri build expects this directory to exist
cargo run --example server --manifest-path src-tauri/Cargo.toml
```

The server listens on `127.0.0.1:18766` and serves `dist-web/`. To keep a local build running as
a systemd user service, use `scripts/install-server.sh`; how it works and how its updates behave
is in [docs/deploy.md](docs/deploy.md#build-from-source).

## Tests

```bash
pnpm typecheck
pnpm format:check
pnpm test:unit                                        # frontend, vitest
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo nextest run --manifest-path src-tauri/Cargo.toml   # backend; `cargo test` also works
bash scripts/tests/run-all.sh                         # the shell suites: installer, updater, release, sync
```

`bash scripts/tests/<name>.test.sh` runs one suite. Some backend tests write to directories such
as `~/.cc-switch` and `~/.codex`; run them with `CC_SWITCH_TEST_HOME="$(mktemp -d)"` set (only
that variable, not `HOME`). CI runs the same commands.

## Where the fork's code lives

The fork's own code is the set of paths upstream does not have. This command lists them
(`upstream-main` is the clean mirror of upstream that `scripts/sync-upstream.sh` keeps; run the
script once, or `git fetch upstream` and use `upstream/main`, if it is not there yet):

```bash
git diff --name-only --diff-filter=A upstream-main...main
```

The main ones:

- the web adapter and its shims: `src/lib/api/adapter.ts`, `src/lib/web-shims/`,
  `src/components/web/`, `vite.config.web.mts`
- the server: `src-tauri/src/web_api*`, `src-tauri/src/services/web_update.rs`,
  `src-tauri/src/bootstrap.rs`, `src-tauri/src/events.rs`, `src-tauri/src/server_log.rs`,
  `src-tauri/examples/server.rs` and the `src-tauri/tests/web_api_*.rs` tests
- packaging and deployment: `Dockerfile.web`, `scripts/`, `deploy/`
- the web-specific parts of files upstream also has: the web branch of
  `src/components/settings/AboutSection.tsx`, `SelfUpdatePanel.tsx`, `WebCommitList.tsx`,
  `src/lib/updater.ts`, `src/lib/api/capabilities.ts`, `src/lib/api/selfUpdate.ts`,
  `src/hooks/useSelfUpdate.ts`, and the `web.*` keys of the locale files

Everything else is upstream's code. Do not edit it in passing (a formatting fix, a rename, a
refactor): every such line becomes a conflict in the next sync. If a change in upstream's code is
needed, keep it small and say so in the pull request, or propose it to upstream first.

The fork-owned documents (README, SECURITY, SUPPORT, CONTRIBUTING, CODE_OF_CONDUCT, CHANGELOG,
LICENSE, `docs/deploy.md`, `.github/`) are listed in `fork_owned` of `docs/upstream.lock`; a sync
keeps the fork's version of them.

## Synchronizing with upstream

The fork follows upstream every few weeks. `docs/upstream.lock` records the upstream tag and
commit, the branch policy and the `fork_owned` paths.

```bash
git remote add upstream https://github.com/farion1231/cc-switch.git   # once per clone
scripts/sync-upstream.sh [--ref <upstream-ref>]                       # default upstream/main
```

The script fetches upstream, fast-forwards `upstream-main` (a clean mirror), starts
`sync/upstream-<sha>` from `main`, merges without committing and puts the `fork_owned` paths back
to the fork's version. It exits `0` when the merge is prepared without conflicts, `2` when
conflicts in code are left to resolve by hand, and `1` when it refused or failed. It never
commits: review the result, run the tests, commit and open a pull request.

## Pull requests

- Work on a branch, for example `feat-cld/<name>` or `fix/<name>`.
- Open the pull request as a **draft** against `main`; mark it ready when the checks pass.
- Run the commands under "Tests" first. Add or update tests for what you change.
- Update `docs/deploy.md` when installation or deployment changes, and `CHANGELOG.md` for a
  user-visible change.
- Commit messages follow Conventional Commits (`feat:`, `fix:`, `docs:`, `chore:`).
- Leave the database version to maintainers: do not change `SCHEMA_VERSION`. A pull request from
  a non-collaborator that adds a `const SCHEMA_VERSION` line is closed automatically
  (`.github/workflows/schema-version.yml`). If your change needs a schema change, explain why in
  the pull request and mention the maintainer (`@alxwlw`); it is reopened if appropriate.
- A change that is not about the web console but about the desktop app belongs upstream.

By contributing you agree that your contribution is licensed under the [MIT License](LICENSE).
Be kind: see the [Code of Conduct](CODE_OF_CONDUCT.md).
