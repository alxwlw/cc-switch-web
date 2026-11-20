# Changelog

Changes of cc-switch-web. Changes of the cc-switch desktop app it is based on are in the upstream
[CHANGELOG](https://github.com/farion1231/cc-switch/blob/main/CHANGELOG.md).

## v4.0.35-web.1 — 2026-11-20

- First release of the fork:
  - The console in the browser: the cc-switch desktop app on a Linux server without a desktop, at parity with the desktop app for everything that does not need a desktop session.
  - Install with `install.sh` (a systemd user service from a release archive) or with Docker Compose.
  - Access through an SSH tunnel or Tailscale; the server listens on `127.0.0.1` by default.
  - Update check against releases and self-update from Settings → About.
