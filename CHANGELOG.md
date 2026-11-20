# Changelog

Changes of cc-switch-web. Changes of the cc-switch desktop app it is based on are in the upstream
[CHANGELOG](https://github.com/farion1231/cc-switch/blob/main/CHANGELOG.md).

## v4.0.36-web.1 — 2026-11-20

- Based on cc-switch [v4.0.36](https://github.com/farion1231/cc-switch/releases/tag/v4.0.36).
- The new Plugins page works in the console: install, enable, update and uninstall plugins of Claude Code, Codex, Grok Build, MiniMax Code, OpenClaw and Pi, and manage their marketplaces. Each action runs that agent's own CLI on the server, so the CLI has to be installed there.
- The README warns users of zuoliangyu/cc-switch-web, a different project with the same name and the same data directory `~/.cc-switch-web`.

## v4.0.35-web.2 — 2026-11-20

- About now points at this fork, not at the upstream app:
  - Release Notes opens the release of the running version on the fork's GitHub page, or the releases list for a build from source.
  - The GitHub button is always shown and opens the fork; the "Star on GitHub" banner and the Official Website button are gone from the web console.
  - The server line reads "Based on CC Switch v…" instead of "Server version", and the raw runtime-boundaries text is no longer printed (it stays as the tooltip of the host notice).
- The browser tab is titled `cc-switch-web` instead of the upstream Chinese title.

## v4.0.35-web.1 — 2026-11-20

- First release of the fork:
  - The console in the browser: the cc-switch desktop app on a Linux server without a desktop, at parity with the desktop app for everything that does not need a desktop session.
  - Install with `install.sh` (a systemd user service from a release archive) or with Docker Compose.
  - Access through an SSH tunnel or Tailscale; the server listens on `127.0.0.1` by default.
  - Update check against releases and self-update from Settings → About.
