# Security Policy

## Supported versions

Only the latest release of cc-switch-web receives security fixes. Update to it first
(see [docs/deploy.md](docs/deploy.md#updates)).

## Threat model

cc-switch-web is a web console for one user on a Linux server. It edits the agent configs of the
user it runs as (Claude Code, Codex, Gemini CLI and others) and runs a local proxy for them.

- **The server has no login.** By default it listens on `127.0.0.1`. A non-loopback address is
  accepted only with `TRUSTED_TAILNET=1`; without it the server refuses to start.
- **Whoever reaches the port has full control** of that user's agent configs and provider keys,
  and can start an update to the latest published release.
- So the port is never published to the internet or a shared network. Reach the console through
  an SSH tunnel, or through a tailnet whose ACLs allow only the people who may manage your
  agents. The installer, the Docker image and the compose file do not open the port to anything
  beyond what you set.
- The server runs with the rights of its user and has no privilege separation from it.

## Reporting a vulnerability

Use **Report a vulnerability** on the repository's Security tab, or open
<https://github.com/alxwlw/cc-switch-web/security/advisories/new>. Do not report it in a public
issue. Include the version (Settings → About), how it is installed, and steps to reproduce.

## Scope

In scope: the server (`/api/*`), the installer (`scripts/install.sh`), the self-update
(`scripts/self-update.sh` and its routes) and the container image.

Out of scope: vulnerabilities of the cc-switch desktop application itself, which belong to
[upstream](https://github.com/farion1231/cc-switch), and a deployment that publishes the port
against the advice above.
