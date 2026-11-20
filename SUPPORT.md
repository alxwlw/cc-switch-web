# Support

Questions and bug reports go to the issues of this repository:
<https://github.com/alxwlw/cc-switch-web/issues>. Search the existing ones, closed included,
before you open a new one.

- **A bug**: [open a bug report](https://github.com/alxwlw/cc-switch-web/issues/new?template=bug_report.yml).
- **A feature request**: [open a feature request](https://github.com/alxwlw/cc-switch-web/issues/new?template=feature_request.yml).
- **A question** about installing or using the console: [open an issue](https://github.com/alxwlw/cc-switch-web/issues/new).
- **A security problem**: not in a public issue; see [SECURITY.md](SECURITY.md).
- **A bug that also happens in the cc-switch desktop app**: report it to
  [upstream](https://github.com/farion1231/cc-switch/issues).

## What to include

- the version, from Settings → About;
- how you installed it (release installer or Docker Compose) and your Linux distribution and
  architecture;
- what you did, what you expected and what happened;
- the log: `journalctl --user -u cc-switch-web -n 100` for a release install, or
  `docker compose logs` for Docker. Remove keys and tokens before you paste it.

This is a small project maintained in spare time; there is no guaranteed response time.
