# Phase 1 — версия форка, обновления по релизам, установщик, релизный workflow, Docker

PR: https://github.com/alxwlw/cc-switch-web/pull/43, ветка `feat-cld/oss-release-p1`

Цель: всё, что нужно для релиза, кроме консоли и лица проекта. После фазы бинарник знает свою версию форка; проверка обновлений и самообновление работают по релизам; `scripts/install.sh` ставит архив релиза; `release-web.yml` на PR собирает архивы и образ и прогоняет smoke-тесты, а по тегу публикует релиз; есть `deploy/compose.yml`. Тег в этой фазе не ставится: апстримовский `release.yml` тоже срабатывает на `v*`, он удаляется в фазе 2.

Спека: `docs/superpowers/specs/2026-11-19-oss-release-design.md`, разделы 1–5.

Scope: `src-tauri/src/services/web_update.rs`, `src-tauri/src/web_api/self_update.rs` (+ `self_update/tests.rs`), `src-tauri/src/web_api.rs` (только `capabilities_payload`, `capabilities`, `update_check`, `update_check_with`, `update_install`), `src-tauri/src/lib.rs` (re-export), `src-tauri/tests/web_api_update.rs`, `src-tauri/tests/web_api_update_install.rs`, `scripts/self-update.sh`, `scripts/install.sh` (новый), `scripts/tests/*` (новые), `.github/workflows/release-web.yml` (новый), `Dockerfile.web`, `deploy/compose.yml` (новый).

Факты разведки (`origin/main@b43896c1`):
- `web_update.rs:108` `pub fn build_sha() -> Option<&'static str>` = `normalize_sha(option_env!("CC_SWITCH_WEB_GIT_SHA"))`; в `build.rs` переменной нет — cargo сам пересобирает при смене значения `option_env!`.
- `web_api.rs:752` `fn capabilities_payload(build_sha: Option<&str>, self_update: bool) -> Value` — `json!` с ключами `mode, auth, version, buildSha, selfUpdate, platform, mvp_apps, runtime_boundaries`; обработчик `capabilities` (:768) смонтирован в полном роутере (:122) и в `init_error_router` (:656).
- `web_update.rs:211` `pub async fn check(cfg: &UpdateSourceConfig, build_sha: Option<&str>, force: bool) -> Result<UpdateCheck, AppError>`; re-export в `lib.rs:85`. `UpdateCheck` (:75, camelCase): `status, currentVersion, currentSha, latestSha, behindBy, commits, checkedAt, branch, instructions{systemd,docker}`. Кеш — одна запись в `tokio::sync::Mutex` (:176), ключ api_base+repo+branch+sha (:155), TTL 3600 с, ошибки не кешируются. HTTP — `crate::proxy::http_client::get_for_url(url)`, User-Agent `cc-switch-web`, таймаут 15 с; 403/429 → ошибка про rate limit.
- `web_api.rs:834` `update_check_with(cfg, build_sha, force)`; `?force=1|true`.
- Тесты проверки: `src-tauri/tests/web_api_update.rs` поднимает локальный axum-сервер как заглушку GitHub, `api_base` подставляется в `UpdateSourceConfig`; тесты `#[tokio::test] #[serial]`.
- `self_update.rs:260` `fn resolve(env: Env, branch: &str) -> Result<UpdateLaunch, String>`, `Env = &dyn Fn(&str) -> Option<OsString>`; `UpdateLaunch { unit, script, source_dir, branch, env, status_file }` (:52); `systemd_run_args` (:69) → `--user --unit=<unit> --collect --setenv=K=V… -- <script> <source_dir> <branch>`; `FORWARDED_ENV` (:34) = `HOST, PORT, WEB_DIST, XDG_STATE_HOME, CC_SWITCH_WEB_BUILD_PATH`; `UpdateRunner::new(Arc<dyn UpdateSpawner>)` (:322); `UpdateRunStatus { stage, ok, message, fromSha, toSha, startedAt, finishedAt }` (:416), stage — свободная строка. Тесты: `self_update/tests.rs` (`Rig` с tempdir и HashMap-окружением), маршруты — `tests/web_api_update_install.rs` (фейковый spawner через `router_with_update`).
- `scripts/self-update.sh <source_dir> <branch>`: стадии `checking → fetching → merging → building → installing → restarting → done`, при сбое `rolling-back → rolled-back | rollback-failed`, до замены — `failed`; `write_status` (:73) атомарно; `probe <want>` (:160) сверяет `buildSha` из `/api/capabilities` sed-ом и ждёт 2xx от `/api/health`; замена (:323-329) — `mv` в `.prev` и `.new` на место; откат (:189-211). Тест — `scripts/tests/self-update.test.sh` (заглушки в PATH, «N passed, M failed»), в CI не запускается.
- `examples/server.rs`: `HOST` по умолчанию `127.0.0.1`, `PORT` 18766, не-loopback без `TRUSTED_TAILNET=1` — отказ (:163); `WEB_DIST` без `index.html` → только `/api`. `/api/health` → `{"status":"ok","instanceId":…}`.
- Сравнения semver в крейте нет (приватные `parse_semver`/`compare_semver` в `commands/misc.rs:1424,1469` — апстримовские, не трогать).
- Пакеты для сборки на Linux — как в `ci.yml` job `backend` (GTK/webkit/soup); перед `cargo build` нужен `mkdir -p dist` (`tauri.conf.json` `frontendDist`).

### Task 1: версия форка в бинарнике

**Files:**
- Modify: `src-tauri/src/services/web_update.rs` (рядом с `build_sha`, :108)
- Modify: `src-tauri/src/lib.rs:85` (re-export `WebVersion`)
- Modify: `src-tauri/src/web_api.rs:752-775` (`capabilities_payload`, `capabilities`) и вызовы `capabilities_payload` в тестах web_api.rs (~:5218-5320)
- Modify: `Dockerfile.web` (ARG рядом с `CC_SWITCH_WEB_GIT_SHA`)

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)] pub struct WebVersion { pub upstream: [u64; 3], pub n: u64 }` — порядок полей задаёт сравнение: сначала апстрим, потом `n`.
  - `impl WebVersion { pub fn parse(s: &str) -> Option<Self> }` — принимает `4.0.35-web.1` и `v4.0.35-web.1`, ничего больше (без обрезки пробелов).
  - `impl std::fmt::Display for WebVersion` → `4.0.35-web.1` (без `v`).
  - `pub fn web_version() -> Option<WebVersion>` — `option_env!("CC_SWITCH_WEB_VERSION")`, пробелы по краям обрезаются, пустое или неразбираемое → `None`.
  - `fn capabilities_payload(build_sha: Option<&str>, web_version: Option<WebVersion>, self_update: bool) -> Value` — новый ключ `"webVersion"`: строка или `null`.

- [ ] **Step 1: тесты `WebVersion`** (в `mod tests` файла `web_update.rs`)

```rust
#[test]
fn web_version_parses_the_tag_and_the_bare_form() {
    assert_eq!(WebVersion::parse("v4.0.35-web.1"), Some(WebVersion { upstream: [4, 0, 35], n: 1 }));
    assert_eq!(WebVersion::parse("4.0.35-web.12").map(|v| v.n), Some(12));
}

#[test]
fn web_version_rejects_every_other_shape() {
    for s in ["", "4.0.35", "4.0.35-web.", "4.0.35-web.1x", "4.0-web.1", "v4.0.35-beta.1", " 4.0.35-web.1", "vv4.0.35-web.1"] {
        assert_eq!(WebVersion::parse(s), None, "{s:?}");
    }
}

#[test]
fn web_versions_order_by_upstream_then_n() {
    let p = |s| WebVersion::parse(s).unwrap();
    assert!(p("4.0.35-web.2") > p("4.0.35-web.1"));
    assert!(p("4.0.36-web.1") > p("4.0.35-web.9"));
    assert!(p("4.1.0-web.1") > p("4.0.99-web.9"));
}

#[test]
fn web_version_displays_without_the_v() {
    assert_eq!(WebVersion::parse("v4.0.35-web.1").unwrap().to_string(), "4.0.35-web.1");
}
```

- [ ] **Step 2:** `cargo test --manifest-path src-tauri/Cargo.toml --lib web_version` → FAIL (нет `WebVersion`).
- [ ] **Step 3:** реализовать `WebVersion`, `parse`, `Display`, `web_version()`; re-export в `lib.rs`.
- [ ] **Step 4:** тот же запуск → PASS.
- [ ] **Step 5: тест capabilities** (рядом с `capabilities_payload_carries_the_given_build_sha` в `web_api.rs`)

```rust
#[test]
fn capabilities_payload_carries_the_web_version() {
    let v = WebVersion::parse("4.0.35-web.1");
    assert_eq!(capabilities_payload(None, v, false)["webVersion"], "4.0.35-web.1");
    assert!(capabilities_payload(None, None, false)["webVersion"].is_null());
}
```

Существующие вызовы `capabilities_payload(x, y)` в тестах → `capabilities_payload(x, None, y)`.
- [ ] **Step 6:** `cargo test --manifest-path src-tauri/Cargo.toml --lib capabilities` → FAIL, затем добавить параметр и ключ; обработчик `capabilities` передаёт `web_update::web_version()` → PASS.
- [ ] **Step 7:** `Dockerfile.web`: `ARG CC_SWITCH_WEB_VERSION=` сразу после `ARG CC_SWITCH_WEB_GIT_SHA=` (после `COPY . .`, чтобы смена значения не сбрасывала кеш слоёв выше); комментарий над ARG — пустое значение = сборка не из релиза. Проверка: `docker build -f Dockerfile.web --build-arg CC_SWITCH_WEB_VERSION=4.0.20-web.0 -t ccsw-t1 . && docker run --rm -d --name ccsw-t1 ccsw-t1 && sleep 5 && docker exec ccsw-t1 curl -fsS http://127.0.0.1:18766/api/capabilities | grep -o '"webVersion":"4.0.20-web.0"'; docker rm -f ccsw-t1` → печатает `"webVersion":"4.0.20-web.0"`.
- [ ] **Step 8:** commit `feat(web): embed the fork version and report it as webVersion`.

### Task 2: проверка обновлений по релизам

**Files:**
- Modify: `src-tauri/src/services/web_update.rs` (`UpdateCheck` :75, `check` :211, ключ кеша :155, `instructions()` :92-103)
- Modify: `src-tauri/src/web_api.rs` (`update_check` :781, `update_check_with` :834)
- Test: `src-tauri/tests/web_api_update.rs` (заглушка GitHub получает маршрут `/repos/o/r/releases/latest`)

**Interfaces:**
- Consumes: `WebVersion`, `web_version()` (Task 1).
- Produces:
  - `#[serde(rename_all = "kebab-case")] pub enum UpdateMode { Release, Commit }` → `"release"` / `"commit"`.
  - В `UpdateCheck` новые поля (camelCase): `mode: UpdateMode`, `latest_version: Option<String>`, `release_url: Option<String>`, `published_at: Option<String>`.
  - `pub async fn check(cfg: &UpdateSourceConfig, build_sha: Option<&str>, web_version: Option<WebVersion>, force: bool) -> Result<UpdateCheck, AppError>`: `Some(v)` → режим релизов, compare API не вызывается; `None` → нынешняя логика по коммитам без изменений, `mode: "commit"`, новые поля `null`.
  - `update_check_with(cfg, build_sha, web_version, force)`; `update_check` передаёт `web_update::web_version()`.

Режим релизов:
- `GET {api_base}/repos/{repo}/releases/latest` с теми же заголовками, таймаутом и обработкой 403/429, что у compare.
- 404 → `unknown` (релизов ещё нет). `tag_name` не разбирается `WebVersion::parse` → `unknown`, `latestVersion: null`.
- последний > текущего → `behind`; равен → `up-to-date`; меньше → `unknown` (сборка новее последнего релиза).
- `currentVersion` = версия форка (`4.0.35-web.1`), `latestVersion` = версия последнего без `v`, `releaseUrl` = `html_url`, `publishedAt` = `published_at`, `behindBy: 0`, `commits: []`, `latestSha: null`, `currentSha` = build sha.
- Ключ кеша дополняется версией форка; TTL, `force` и «ошибки не кешируются» — как сейчас.
- `instructions` в этом режиме: `systemd` = `curl -fsSL https://github.com/{repo}/releases/latest/download/install.sh | bash`, `docker` = `docker compose pull && docker compose up -d`.

- [ ] **Step 1: тесты** в `tests/web_api_update.rs` (заглушка отдаёт `{"tag_name","html_url","published_at"}` или заданный статус и считает обращения к `releases/latest` и `compare`):
  - `a_newer_release_is_behind_with_its_version_and_notes_url` — текущая `4.0.35-web.1`, тег `v4.0.35-web.2`: `status == "behind"`, `mode == "release"`, `latestVersion == "4.0.35-web.2"`, `releaseUrl` = `html_url` заглушки, `currentVersion == "4.0.35-web.1"`, `behindBy == 0`, `commits` пуст.
  - `the_installed_release_is_up_to_date` — тег `v4.0.35-web.1` → `up-to-date`.
  - `a_build_newer_than_the_latest_release_is_unknown` — текущая `4.0.36-web.1`, тег `v4.0.35-web.3` → `unknown`.
  - `no_release_yet_is_unknown` — 404 → `unknown`, `latestVersion` null.
  - `a_latest_tag_that_is_not_a_fork_version_is_unknown` — тег `v4.0.36` → `unknown`, `latestVersion` null.
  - `a_release_build_never_asks_the_compare_api` — после проверки счётчик `compare` == 0.
  - `a_release_answer_is_cached_and_force_asks_again` — два вызова → 1 обращение; третий с `force` → 2.
  - `a_rate_limited_release_check_is_an_error_and_not_cached` — 403 → `Err`, текст содержит `rate limit`; следующий вызов снова идёт к заглушке.
  - `release_mode_instructions_rerun_the_installer_and_pull_the_image` — точные строки `instructions.systemd` (с `o/r`) и `instructions.docker` из списка выше.
  - в `three_commits_ahead_is_behind_with_three_newest_first` добавить `mode == "commit"` и `latestVersion` null.
- [ ] **Step 2:** `cargo test --manifest-path src-tauri/Cargo.toml --test web_api_update` → новые тесты FAIL (не компилируется сигнатура `check`).
- [ ] **Step 3:** реализовать режим релизов в `web_update.rs` и проброс версии в `web_api.rs`; существующие вызовы `check(...)` в тестах получают `None` четвёртым аргументом.
- [ ] **Step 4:** `cargo test --manifest-path src-tauri/Cargo.toml --test web_api_update` и `--lib web_update` → PASS, все старые тесты зелёные.
- [ ] **Step 5:** commit `feat(web): check for updates against the latest release in release builds`.

### Task 3: установщик `scripts/install.sh`

**Files:**
- Create: `scripts/install.sh` (bash, один самодостаточный файл: его качают `curl | bash`)
- Create: `scripts/tests/lib/release-fixture.sh` (общий с Task 4)
- Create: `scripts/tests/install.test.sh`

**Interfaces:**
- Consumes: `webVersion` в `/api/capabilities` (Task 1).
- Produces:
  - Вызов: `install.sh [--tailscale | --loopback] [--port N] [--self-update | --no-self-update] [--version vX.Y.Z-web.N] [--uninstall]`. Ошибки — в stderr с префиксом `install.sh: `, код 1.
  - Окружение: `CC_SWITCH_WEB_REPO` (по умолчанию `alxwlw/cc-switch-web`), `CC_SWITCH_WEB_UPDATE_API` (`https://api.github.com`), `CC_SWITCH_WEB_DOWNLOAD_BASE` (`https://github.com/$REPO/releases/download`), `CC_SWITCH_WEB_HEALTH_TIMEOUT` (30). Все загрузки — `curl -fsSL`, поэтому тесты подставляют `file://` URL.
  - Зашитый тег: строка `RELEASE_TAG="__RELEASE_TAG__"`; релизный workflow (Task 5) заменяет плейсхолдер на тег. Плейсхолдер и нет `--version` → тег из `GET $API/repos/$REPO/releases/latest` (`tag_name`, разбор без jq).
  - Архив: `$BASE/<tag>/cc-switch-web-<версия>-linux-<arch>.tar.gz`, внутри каталог `cc-switch-web-<версия>-linux-<arch>/` с `cc-switch-web`, `dist-web/`, `install.sh`, `self-update.sh`, `LICENSE`; рядом `$BASE/<tag>/SHA256SUMS`. `<arch>` = `uname -m`, только `x86_64` и `aarch64`.
  - Подключённый через `source` (как `install-server.sh`) файл только определяет функции: `detect_arch`, `missing_libs <binary>` (строки `not found` из `ldd`; `ldd` с ошибкой → пусто), `read_unit_env <unit-file> <KEY>`, `render_unit <host> <port> <trusted 0|1> <self_update 0|1> <repo>`.
  - `release-fixture.sh`: `make_release <root> <version> <arch>...` — архивы и `SHA256SUMS` в `<root>/v<version>/`, вместо бинарника — shell-скрипт со строкой `# fake cc-switch-web <version>`; `set_latest <root> <version>` — `<root>/api/repos/o/r/releases/latest` с `tag_name`.

Порядок работы установщика (ничего на месте установки не трогается до шага 5):
1. Проверить `curl`, `tar`, `sha256sum` — нет → ошибка с их именами. Под root — предупреждение, что конфиги агентов будут читаться из домашнего каталога root.
2. Тег, архитектура; скачать архив и `SHA256SUMS` во временный каталог (удаляется по `trap`); строки для архива в `SHA256SUMS` нет → ошибка «no archive for <arch> in <tag>» с советом про Docker; хеш не совпал → ошибка со словом `checksum`.
3. Распаковать во временный каталог; `missing_libs` по распакованному бинарнику.
4. Не хватает и есть `apt-get` → сообщение (пакеты, «a few hundred MB»), затем `$SUDO apt-get update` и `$SUDO apt-get install -y --no-install-recommends libssl3 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 libwebkit2gtk-4.1-0` (`$SUDO` пуст под root, иначе `sudo`; нет `sudo` → ошибка с командой для ручного запуска); проверить снова — не хватает → ошибка со списком. `apt-get` нет → ошибка со списком библиотек и советом про Docker.
5. Настройки: если unit уже есть — `HOST`, `PORT`, `TRUSTED_TAILNET`, `CC_SWITCH_WEB_SELF_UPDATE` берутся из него; флаги их перекрывают; без unit — `127.0.0.1`, `18766`, без `TRUSTED_TAILNET`, самообновление `1`. `--tailscale` → `HOST=$(tailscale ip -4 | head -1)` (пусто → ошибка) и `TRUSTED_TAILNET=1`; `--loopback` → `127.0.0.1` без `TRUSTED_TAILNET`.
6. Раскладка (Global Constraints). Каждый файл — во временное имя рядом и `mv -f` (бинарник, `install.sh`, `self-update.sh`); `dist-web` — `dist-web.new` → `mv` старого в `dist-web.old` → `mv` нового → удалить `.old`. Каталог данных — `mkdir -p` с правами 700, существующий не трогается.
7. Unit (`render_unit`), ровно эти строки:

```
[Unit]
Description=cc-switch-web console (no in-app login; listens on HOST only)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=%h/.local/bin/cc-switch-web
Environment=HOST=<host>
Environment=PORT=<port>
Environment=TRUSTED_TAILNET=1            # только при trusted=1
Environment=CC_SWITCH_CONFIG_DIR=%h/.cc-switch-web
Environment=WEB_DIST=%h/.local/share/cc-switch-web/dist-web
Environment=CC_SWITCH_WEB_INSTALL_DIR=%h/.local/share/cc-switch-web
Environment=CC_SWITCH_WEB_UPDATE_REPO=<repo>
Environment=CC_SWITCH_WEB_SELF_UPDATE=<1|0>
Environment=RUST_LOG=info
Restart=always
RestartSec=3

[Install]
WantedBy=default.target
```
   (комментария `# только при trusted=1` в файле нет.)
8. `systemctl --user show-environment` успешен → `daemon-reload`, `enable cc-switch-web`, `restart cc-switch-web`; `loginctl show-user "$USER" -p Linger` не `Linger=yes` → `loginctl enable-linger "$USER"`, отказ → напечатать `sudo loginctl enable-linger <user>`. Неуспешен → unit не пишется, печатается команда запуска в foreground со всеми переменными (`HOST=… PORT=… CC_SWITCH_CONFIG_DIR=… WEB_DIST=… ~/.local/bin/cc-switch-web`), выход 0.
9. До `CC_SWITCH_WEB_HEALTH_TIMEOUT` секунд опрашивать `http://<host>:<port>/api/capabilities` (`0.0.0.0` → `127.0.0.1`), пока в ответе не будет `"webVersion":"<версия>"`; не дождались → ошибка с `journalctl --user -u cc-switch-web -n 50`, файлы остаются.
10. Итог: версия; для loopback — `ssh -L <port>:localhost:<port> <user>@<hostname -f>` и `http://localhost:<port>`; для tailnet — `http://<host>:<port>`; каталог данных; команда удаления `curl -fsSL https://github.com/<repo>/releases/latest/download/install.sh | bash -s -- --uninstall`.

`--uninstall`: `systemctl --user disable --now cc-switch-web` (если есть systemd), удалить unit, бинарник, `~/.local/share/cc-switch-web`; напечатать, что данные остались в `~/.cc-switch-web`.

- [ ] **Step 1: `release-fixture.sh` и тесты** `scripts/tests/install.test.sh` — формат как у `self-update.test.sh` (каждый случай в своём временном `HOME`, заглушки в `PATH`: `systemctl` и `loginctl` пишут аргументы в лог, `tailscale ip -4` печатает `100.64.0.7`, `sudo` выполняет аргументы, `apt-get` пишет аргументы в лог, `ldd` и `uname` настраиваются переменными, `curl` отвечает за `http://…/api/capabilities` строкой с версией из установленного бинарника, остальное передаёт настоящему `curl`; итог «N passed, M failed», код ≠ 0 при падениях). Случаи:
  1. `fresh install lays out files and enables the unit` — все пути раскладки на месте, бинарник исполняемый; unit содержит `Environment=HOST=127.0.0.1`, `Environment=PORT=18766`, `Environment=CC_SWITCH_WEB_SELF_UPDATE=1`, не содержит `TRUSTED_TAILNET`; в логе `systemctl` — `--user daemon-reload`, `--user enable cc-switch-web`, `--user restart cc-switch-web`; вывод содержит `ssh -L 18766:localhost:18766`.
  2. `--tailscale binds the tailnet address with TRUSTED_TAILNET` — `HOST=100.64.0.7`, `TRUSTED_TAILNET=1`, вывод `http://100.64.0.7:18766`.
  3. `a re-run without flags keeps address, port and self-update` — сначала `--tailscale --port 18800 --no-self-update`, потом без флагов: в unit всё те же три значения.
  4. `--loopback on a re-run switches back` — `HOST=127.0.0.1`, строки `TRUSTED_TAILNET` нет.
  5. `a checksum mismatch installs nothing` — поверх установленной старой версии; архив новой испорчен после `SHA256SUMS`: код 1, stderr содержит `checksum`, бинарник и `dist-web` побайтно прежние.
  6. `an unsupported or missing architecture is an error naming it` — `uname -m` = `riscv64` → код 1, stderr содержит `riscv64` и `docker`; `aarch64` при фикстуре только с `x86_64` → код 1, stderr содержит `aarch64`.
  7. `missing libraries are installed with apt through sudo` — `ldd` сообщает `libwebkit2gtk-4.1.so.0 => not found`, пока `apt-get install` не выполнялся: в логе `apt-get` через `sudo` по порядку `update`, затем `install -y --no-install-recommends libssl3 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 libwebkit2gtk-4.1-0`; установка завершена.
  8. `missing libraries without apt stop before anything is installed` — `apt-get` нет в `PATH`: код 1, stderr называет `libwebkit2gtk-4.1.so.0` и `docker`, `~/.local/bin/cc-switch-web` не создан.
  9. `the binary is replaced by rename, not rewritten in place` — inode `~/.local/bin/cc-switch-web` (`stat -c %i`) после переустановки другой.
  10. `without a systemd user session files are installed and a foreground command is printed` — `systemctl --user show-environment` падает: вызовов `enable` нет, unit не создан, вывод содержит `WEB_DIST=` и `~/.local/bin/cc-switch-web`, код 0.
  11. `a refused linger prints the sudo command` — `loginctl enable-linger` падает: вывод содержит `sudo loginctl enable-linger`.
  12. `--uninstall removes the install and keeps the data` — файл в `~/.cc-switch-web` остался; бинарник, unit и `~/.local/share/cc-switch-web` удалены; в логе `systemctl` — `--user disable --now cc-switch-web`.
  13. `the baked tag wins over the latest release` — копия `install.sh` с плейсхолдером, заменённым на старый тег, ставит старую версию при `set_latest` на новую; копия с плейсхолдером ставит новую (`CC_SWITCH_WEB_UPDATE_API=file://<root>/api`).
  14. `a service that does not come up names the journal command` — `curl`-заглушка отдаёт чужую версию, `CC_SWITCH_WEB_HEALTH_TIMEOUT=2`: код 1, stderr содержит `journalctl --user -u cc-switch-web`.
- [ ] **Step 2:** `bash scripts/tests/install.test.sh` → FAIL (нет `scripts/install.sh`).
- [ ] **Step 3:** написать `scripts/install.sh`, `chmod +x`.
- [ ] **Step 4:** `bash scripts/tests/install.test.sh` → `14 passed, 0 failed`; `shellcheck scripts/install.sh` (если установлен) без ошибок.
- [ ] **Step 5:** commit `feat(web): install.sh installs a release with a systemd user unit`.

### Task 4: самообновление установки из релиза

**Files:**
- Modify: `src-tauri/src/web_api/self_update.rs` (`UpdateLaunch` :52, `systemd_run_args` :69, `FORWARDED_ENV` :34, `resolve` :260, `UpdateRunner` :322, `UpdateRunStatus` :416)
- Modify: `src-tauri/src/web_api.rs` (`update_install` :794 — тело ответа; место, где `resolve` получает ветку)
- Modify: `scripts/self-update.sh`
- Test: `src-tauri/src/web_api/self_update/tests.rs`, `src-tauri/tests/web_api_update_install.rs`, `scripts/tests/self-update-release.test.sh` (новый)

**Interfaces:**
- Consumes: `web_version()` (Task 1); раскладка и `CC_SWITCH_WEB_INSTALL_DIR` из unit (Task 3); `release-fixture.sh` (Task 3).
- Produces:
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum BuildMode { Checkout, Release }`, `BuildMode::of_this_build()` → `Release`, если `web_version().is_some()`.
  - `pub enum UpdateSource { Checkout { source_dir: PathBuf, branch: String }, Release { install_dir: PathBuf, repo: String } }`; в `UpdateLaunch` поля `source_dir` и `branch` заменяются на `source: UpdateSource`.
  - `systemd_run_args`: `Checkout` — как сейчас (`<script> <source_dir> <branch>`); `Release` — `… -- <install_dir>/self-update.sh --release <repo>`.
  - `fn resolve(env: Env, branch: &str, repo: &str, mode: BuildMode) -> Result<UpdateLaunch, String>`; вызывающий берёт `branch` и `repo` из `UpdateSourceConfig::from_env()`.
  - `UpdateRunner::new(spawner)` = `UpdateRunner::with_mode(spawner, BuildMode::of_this_build())`; `with_mode` — для тестов.
  - Ответ `POST /api/update/install` (202): `{unit, branch: string|null, repo: string|null}` — в `Checkout` `branch` задан, `repo` null; в `Release` наоборот.
  - `UpdateRunStatus` + `from_version: Option<String>`, `to_version: Option<String>` (`fromVersion`, `toVersion`).
  - `self-update.sh --release <repo>` — новый режим; режим `self-update.sh <source_dir> <branch>` не меняется.

Проверки `resolve` в режиме `Release`, по порядку, каждая со своим текстом:
1. `CC_SWITCH_WEB_SELF_UPDATE` = `1`, иначе «self-update is off: set CC_SWITCH_WEB_SELF_UPDATE=1 in the server's environment (install.sh --self-update), or update by hand with the commands of the update check».
2. `INVOCATION_ID` — текст как в режиме `Checkout`.
3. `CC_SWITCH_WEB_INSTALL_DIR` задан, абсолютный, каталог; в нём исполняемый `self-update.sh` (по тексту на каждый случай, по образцу `CC_SWITCH_WEB_SOURCE_DIR`).
4. `repo` соответствует `^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$`, иначе «CC_SWITCH_WEB_UPDATE_REPO is not a usable owner/name».
5. Путь файла статуса — как сейчас.
`CC_SWITCH_WEB_SOURCE_DIR` в этом режиме не нужен. Тексты режима `Checkout` не меняются.
`FORWARDED_ENV` дополняется: `CC_SWITCH_WEB_INSTALL_DIR`, `CC_SWITCH_WEB_UPDATE_API`, `CC_SWITCH_WEB_DOWNLOAD_BASE`, `BIN_DIR`.

Режим скрипта `--release <repo>` (окружение: `CC_SWITCH_WEB_INSTALL_DIR` обязателен, `BIN_DIR` = `~/.local/bin`, `WEB_DIST` = `$CC_SWITCH_WEB_INSTALL_DIR/dist-web`, `CC_SWITCH_WEB_UPDATE_API`, `CC_SWITCH_WEB_DOWNLOAD_BASE`, `HOST`, `PORT` — умолчания как у `install.sh`):
- `checking`: есть `curl`, `tar`, `sha256sum`, `systemctl`; текущая версия — `webVersion` из `/api/capabilities`; последний тег — из `releases/latest`. Последний равен текущему → `done`, «already up to date», ничего не тронуто. Не новее → `failed` «the latest release <x> is not newer than <y>».
- `downloading`: архив для `uname -m` и `SHA256SUMS` во временный каталог внутри `$CC_SWITCH_WEB_INSTALL_DIR`; нет строки для архитектуры → `failed` с её именем.
- `verifying`: `sha256sum`; не совпало → `failed` «checksum mismatch».
- `installing`: распаковать; не хватает системных библиотек (`ldd`) → `failed` со списком и советом запустить `install.sh`; иначе замена бинарника и `dist-web` с копиями `.prev` — тем же кодом, что в режиме `Checkout`.
- `restarting`: `daemon-reload`, `restart`, `wait_healthy` сверяет `webVersion` с новой версией (в режиме `Checkout` — `buildSha`, как сейчас).
- Успех → `install.sh` и `self-update.sh` каталога установки заменяются новыми (временный файл + `mv`: работающий процесс держит старый inode) → `done`. Сбой после начала замены → `rolling-back` → `rolled-back` | `rollback-failed`, проверка здоровья по старой версии; скрипты каталога установки не меняются.
- В статусе этого режима `fromVersion`/`toVersion` заданы, `fromSha`/`toSha` = null; в режиме `Checkout` наоборот.
- Сравнение версий — функция `version_newer <a> <b>` (числа, не строки: `4.0.35-web.10` новее `4.0.35-web.9`).

- [ ] **Step 1: Rust-тесты.** В `self_update/tests.rs`:
  - `a_release_build_resolves_to_the_installed_script_and_the_repo` — `systemd_run_args()` оканчивается на `--`, `<install_dir>/self-update.sh`, `--release`, `o/r`.
  - `a_release_build_does_not_need_a_source_dir` — без `CC_SWITCH_WEB_SOURCE_DIR` → `Ok`.
  - `a_release_build_refuses_without_the_install_dir_or_its_script` — каждый случай пункта 3 → свой текст.
  - `a_repo_that_is_not_owner_slash_name_is_refused` — для `""`, `"o"`, `"o/r/x"`, `"o r/x"`, `"o/r\n"` → текст пункта 4.
  - `the_release_variables_are_forwarded_when_set`.
  - `a_status_file_with_versions_is_read` — `fromVersion`/`toVersion` читаются, лишние поля не проходят.
  В `tests/web_api_update_install.rs`: `a_release_install_is_202_with_the_repo_and_no_branch` (`UpdateRunner::with_mode(fake, BuildMode::Release)`): тело `{"unit":"cc-switch-web-update","branch":null,"repo":"alxwlw/cc-switch-web"}`, spawner получил `UpdateSource::Release`. В `an_enabled_install_is_202_and_the_launcher_gets_the_script_the_branch_and_the_env` — `repo` null.
- [ ] **Step 2:** `cargo test --manifest-path src-tauri/Cargo.toml --lib self_update` и `--test web_api_update_install` → FAIL.
- [ ] **Step 3:** реализовать Rust-часть.
- [ ] **Step 4:** те же запуски → PASS, старые тесты зелёные.
- [ ] **Step 5: bash-тесты** `scripts/tests/self-update-release.test.sh` (фикстура из Task 3; заглушки `systemctl`, `curl` для `/api/*`, как в `self-update.test.sh`; стадия на момент каждого вызова заглушки пишется в лог):
  1. `update to the latest release swaps binary and dist-web and ends done` — стадии по порядку `checking, downloading, verifying, installing, restarting, done`; бинарник — новой версии, `.prev` — старой; `install.sh` и `self-update.sh` каталога установки — из нового архива; `fromVersion`/`toVersion` в статусе.
  2. `already on the latest release is done without touching anything` — `message` «already up to date», inode бинарника прежний.
  3. `a latest release that is not newer fails before downloading` — к архиву не обращались.
  4. `no archive for this architecture fails before the swap` — бинарник прежний, `failed`, `message` с архитектурой.
  5. `a checksum mismatch fails before the swap`.
  6. `missing system libraries fail before the swap and name install.sh`.
  7. `a new build that does not come up is rolled back` — `rolling-back` → `rolled-back`, бинарник и `dist-web` прежние, скрипты каталога установки прежние.
  8. `version_newer compares numbers` — `4.0.35-web.2 > 4.0.35-web.1`, `4.0.36-web.1 > 4.0.35-web.9`, `4.0.35-web.10 > 4.0.35-web.9`, равные — не новее.
- [ ] **Step 6:** `bash scripts/tests/self-update-release.test.sh` → FAIL; реализовать режим в `scripts/self-update.sh`; → `8 passed, 0 failed`; `bash scripts/tests/self-update.test.sh` — всё ещё зелёный (режим `Checkout` не сломан).
- [ ] **Step 7:** commit `feat(web): self-update a release install from the latest release`.

### Task 5: релизный workflow — архивы, smoke-тесты, публикация

**Files:**
- Create: `scripts/package-release.sh`
- Create: `scripts/tests/package-release.test.sh`
- Create: `scripts/tests/smoke-in-container.sh` (запускается внутри контейнера дистрибутива, не набор тестов: имя без `.test.sh`)
- Create: `.github/workflows/release-web.yml`
- Create: `.github/workflows/scripts.yml` — `name: Scripts`, на каждый `pull_request` в `main` и `push` в `main`, без фильтра путей: один job на `ubuntu-latest`, все `scripts/tests/*.test.sh`. Лёгкий, поэтому ловит и PR, которые меняют только документы (проверка следов апстрима из фазы 2 — тоже `*.test.sh`).

**Interfaces:**
- Consumes: `scripts/install.sh` и плейсхолдер `__RELEASE_TAG__` (Task 3), `scripts/self-update.sh` (Task 4), `CC_SWITCH_WEB_VERSION` (Task 1).
- Produces:
  - `package-release.sh <tag> <arch> <binary> <dist-web-dir> <out-dir>` → `<out-dir>/cc-switch-web-<версия>-linux-<arch>.tar.gz` с раскладкой из Task 3 (`cc-switch-web` 0755, `dist-web/`, `install.sh` с зашитым `<tag>`, `self-update.sh`, `LICENSE`).
  - `package-release.sh --installer <tag> <out-file>` → копия `scripts/install.sh` с зашитым `<tag>` (отдельный ассет релиза).
  - Workflow `release-web.yml` (`name: Release (web)`), job `publish` — точка, к которой Task 6 добавляет зависимость от образа.

Workflow:
- Триггеры: `push` тегов `v*-web.*`; `pull_request` в `main` с путями `scripts/**`, `deploy/**`, `Dockerfile.web`, `Dockerfile.web.dockerignore`, `.github/workflows/release-web.yml`, `src-tauri/examples/server.rs`, `src-tauri/src/services/web_update.rs`, `src-tauri/src/web_api/**`; `workflow_dispatch`. Права по умолчанию `contents: read`; `concurrency: release-web-${{ github.ref }}`.
- `scripts` (`ubuntu-latest`): все `scripts/tests/*.test.sh`, падение любого → job красный (тот же цикл, что в `scripts.yml`: на теге `publish` должен от него зависеть).
- `meta`: выходы `tag`, `version` (без `v`), `publish`. На теге: тег проходит регулярку из Global Constraints, его апстрим-часть равна `version` из `src-tauri/Cargo.toml`, в `CHANGELOG.md` есть строка, начинающаяся с `## v<версия>`; иначе ошибка с причиной. Не тег: `version` = `<Cargo version>-web.0`, `publish=false`.
- `build`, матрица `{ubuntu-22.04, x86_64}`, `{ubuntu-22.04-arm, aarch64}`: системные пакеты и Rust-кеш — как job `backend` в `ci.yml`; Node из `.node-version`, `pnpm` из `packageManager` (`npm install -g`, как в `Dockerfile.web`); `pnpm install --frozen-lockfile && pnpm build:web`; `mkdir -p dist`; `cargo build --release --example server --manifest-path src-tauri/Cargo.toml` с `CC_SWITCH_WEB_GIT_SHA=${{ github.sha }}` и `CC_SWITCH_WEB_VERSION=<version>`; `package-release.sh`; артефакт `release-<arch>`.
- `smoke`, нужен `build`, матрица: `debian:12`, `ubuntu:22.04`, `ubuntu:24.04` на `ubuntu-latest` (x86_64) и `ubuntu:24.04` на `ubuntu-22.04-arm` (aarch64). Каталог `artifacts/<tag>/` с архивом и `SHA256SUMS`; `docker run --rm -v "$PWD:/w" <image> bash /w/scripts/tests/smoke-in-container.sh <tag>`. Скрипт: `apt-get update`, ставит только `curl ca-certificates sudo`; создаёт пользователя `tester` с `sudo` без пароля; от него — `CC_SWITCH_WEB_DOWNLOAD_BASE=file:///w/artifacts bash /w/scripts/install.sh --version <tag>` (systemd в контейнере нет — установщик ставит системные пакеты через sudo и печатает команду запуска); запускает сервер в фоне с этими переменными; до 60 с ждёт, затем проверяет: `/api/health` содержит `"status":"ok"`, `/api/capabilities` содержит `"webVersion":"<версия>"`, `/` содержит `<div id="root"`. Любой сбой → вывести лог сервера и выйти ≠ 0.
- `publish` (только `publish=true`; нужны `meta`, `scripts`, `build`, `smoke`; права `contents: write`): скачать артефакты; `SHA256SUMS` по архивам; `package-release.sh --installer <tag> install.sh`; заметки — раздел `CHANGELOG.md` от `## v<версия>` до следующего `## `; `gh release create <tag> --verify-tag --title <tag> --notes-file notes.md --latest` + оба архива, `SHA256SUMS`, `install.sh`.

- [ ] **Step 1: тесты** `scripts/tests/package-release.test.sh`:
  - `the archive has exactly the release layout` — `tar -tzf` даёт ровно `cc-switch-web-4.0.35-web.1-linux-x86_64/{cc-switch-web,install.sh,self-update.sh,LICENSE}` и `dist-web/…`; бинарник 0755.
  - `the installer inside the archive has the tag baked in` — содержит `RELEASE_TAG="v4.0.35-web.1"`, не содержит `__RELEASE_TAG__`.
  - `--installer writes the same baked copy`.
  - `a tag that is not a fork version is refused` — `v4.0.35` → код ≠ 0.
- [ ] **Step 2:** `bash scripts/tests/package-release.test.sh` → FAIL; написать `scripts/package-release.sh` → PASS.
- [ ] **Step 3:** написать `smoke-in-container.sh` и проверить локально на x86_64: собрать архив (`pnpm build:web`, `cargo build --release --example server` с `CC_SWITCH_WEB_VERSION=4.0.20-web.0`, `package-release.sh v4.0.20-web.0 x86_64 …`), сложить в `artifacts/v4.0.20-web.0/` с `SHA256SUMS`, `docker run --rm -v "$PWD:/w" debian:12 bash /w/scripts/tests/smoke-in-container.sh v4.0.20-web.0` → код 0. Временные `artifacts/` не коммитятся.
- [ ] **Step 4:** написать `.github/workflows/release-web.yml`; `actionlint` (если установлен) без ошибок.
- [ ] **Step 5:** commit `ci(web): release workflow builds, smoke-tests and publishes release archives`, push ветки; открыть draft PR фазы (`gh pr create --draft -R alxwlw/cc-switch-web --base main`, если ещё не открыт); `gh run watch` по `Release (web)` → `scripts`, `meta`, `build` (оба), `smoke` (все четыре) зелёные, `publish` пропущен; `Scripts` зелёный.

### Task 6: Docker — образ в ghcr и `deploy/compose.yml`

**Files:**
- Modify: `Dockerfile.web` (финальная стадия и шапка)
- Create: `deploy/compose.yml`
- Create: `scripts/tests/compose-smoke.sh` (запускается в CI и локально, имя без `.test.sh`)
- Modify: `.github/workflows/release-web.yml` (jobs `docker`, `docker-manifest`; `publish` ждёт `docker-manifest`)

**Interfaces:**
- Consumes: `meta` и `publish` из Task 5; `ARG CC_SWITCH_WEB_VERSION` (Task 1).
- Produces: образ `ghcr.io/alxwlw/cc-switch-web:<версия>` и `:latest` (amd64 + arm64); переменная `CC_SWITCH_WEB_IMAGE` в compose — подменить образ (тесты, свои сборки).

`Dockerfile.web`, финальная стадия:
- `ENV HOST=127.0.0.1 PORT=18766`, без `TRUSTED_TAILNET` (сейчас `HOST=0.0.0.0 TRUSTED_TAILNET=1`). Образ рассчитан на `network_mode: host`; в bridge-сети с `-p` пользователь сам задаёт `HOST=0.0.0.0` и `TRUSTED_TAILNET=1`.
- `dist-web` — в `/opt/cc-switch-web/dist-web` (root, 0755), `WEB_DIST` туда же. Причина: контейнер запускается от UID хоста, а домашний каталог `ccswitch` может быть закрыт для чужих (`HOME_MODE` в Debian) — тогда консоль не раздавалась бы.
- Шапку переписать: образ собирается в CI (`release-web.yml`) и публикуется в ghcr; основной способ запуска — `deploy/compose.yml`.
- `LABEL org.opencontainers.image.source="https://github.com/alxwlw/cc-switch-web"` и `org.opencontainers.image.licenses="MIT"`: пакет в ghcr привязывается к репозиторию.

`deploy/compose.yml` — ровно это:

```yaml
# cc-switch-web console. It runs as your user and sees your home directory, so it
# manages the same agent configs your agents use. It listens on 127.0.0.1 only.
#
#   printf 'CCS_UID=%s\nCCS_GID=%s\n' "$(id -u)" "$(id -g)" > .env
#   docker compose up -d
#   # on your laptop: ssh -L 18766:localhost:18766 you@server, then open http://localhost:18766
#
# Update: docker compose pull && docker compose up -d
services:
  cc-switch-web:
    image: ${CC_SWITCH_WEB_IMAGE:-ghcr.io/alxwlw/cc-switch-web:latest}
    network_mode: host
    user: "${CCS_UID:?set CCS_UID to $(id -u)}:${CCS_GID:?set CCS_GID to $(id -g)}"
    environment:
      HOME: ${HOME}
      CC_SWITCH_CONFIG_DIR: ${HOME}/.cc-switch-web
      HOST: 127.0.0.1
      PORT: "18766"
      # Tailscale instead of an SSH tunnel: your tailnet address and the trust flag.
      # HOST: 100.x.y.z
      # TRUSTED_TAILNET: "1"
    volumes:
      - ${HOME}:${HOME}
    restart: unless-stopped
```

`compose-smoke.sh <image>`: временный каталог как `HOME` (владелец — текущий пользователь), `.env` с `id -u`/`id -g`; `HOME=<tmp> CC_SWITCH_WEB_IMAGE=<image> docker compose -f deploy/compose.yml --env-file <tmp>/.env up -d`; до 60 с ждать `/api/health` на `127.0.0.1:18766`; проверить `"webVersion":"<версия>"` в `/api/capabilities`, `<div id="root"` в `/`, что `<tmp>/.cc-switch-web` создан и `stat -c %u` равен `id -u`; в конце `docker compose down` (и при сбое тоже, с логами контейнера).

Jobs в `release-web.yml`:
- `docker`, матрица `{ubuntu-latest, linux/amd64}`, `{ubuntu-22.04-arm, linux/arm64}`: `docker/build-push-action` с `file: Dockerfile.web`, build-args `CC_SWITCH_WEB_GIT_SHA=${{ github.sha }}`, `CC_SWITCH_WEB_VERSION=<version>`; сначала `load: true` и `compose-smoke.sh` на собранном образе; при `publish=true` — push по digest в `ghcr.io/alxwlw/cc-switch-web` (логин `GITHUB_TOKEN`, права `packages: write` только у этого job), digest — в артефакт.
- `docker-manifest` (только `publish=true`): `docker buildx imagetools create -t ghcr.io/alxwlw/cc-switch-web:<version> -t ghcr.io/alxwlw/cc-switch-web:latest <оба digest>`.
- `publish` дополнительно ждёт `docker-manifest`: релиз появляется только когда образ уже есть.

- [ ] **Step 1:** изменить `Dockerfile.web`, написать `deploy/compose.yml` и `scripts/tests/compose-smoke.sh`.
- [ ] **Step 2:** локально: `docker build -f Dockerfile.web --build-arg CC_SWITCH_WEB_VERSION=4.0.20-web.0 -t ccsw-local . && bash scripts/tests/compose-smoke.sh ccsw-local` → код 0. Порт 18766 на машине должен быть свободен (на agentbox systemd unit `cc-switch-web` остановлен, Dokploy слушает 8890).
- [ ] **Step 3:** дописать jobs в `release-web.yml`; `actionlint` без ошибок.
- [ ] **Step 4:** commit `ci(web): build the image, smoke-test it with compose and publish it to ghcr`, push; `gh run watch` → `docker` (оба) зелёные, `docker-manifest` и `publish` пропущены.

### Task 7: гейты и PR фазы

- [ ] **Step 1:** `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, `cargo nextest run --manifest-path src-tauri/Cargo.toml` (или `cargo test`, если nextest не установлен) → зелёные; `pnpm typecheck && pnpm test:unit` → зелёные (фронтенд в фазе не менялся, проверяем, что типы не разъехались); `for t in scripts/tests/*.test.sh; do bash "$t"; done` → везде `0 failed`.
- [ ] **Step 2:** описание draft PR: что даёт фаза, что тег в ней не ставится и почему (апстримовский `release.yml` на `v*`), что консоль покажет режим релизов только после фазы 2. `plan-state verify-push` перед словом «pushed».
- [ ] **Step 3:** CI PR зелёный (`CI` и `Release (web)`); PR остаётся draft, сливает владелец.

## Notes for next phase

- Пока фаза 2 не слита, сборка с `webVersion` в консоли покажет «behind by 0 commits» в режиме релизов и сырые имена новых стадий (`downloading`, `verifying`): фронтенд их ещё не знает. Релизов до фазы 3 нет, так что пользователи этого не увидят.
- Консоль (фаза 2, `useSelfUpdate`): финальную стадию `done` хук считает «ничего не изменилось» только при `fromSha===toSha` и обоих не `null`. Релизный «already up to date» `done` приходит с `fromVersion===toVersion` и `fromSha`/`toSha` равными `null`, и страница перезагрузилась бы зря, поэтому фаза 2 сравнивает ещё и версии (то же правило записано в `phase-2.md`, Task 1). Ответ установки теперь `{unit, branch|null, repo|null}`; в статус добавлены `fromVersion`/`toVersion`; новые стадии — `downloading` и `verifying`.
- `Release (web)` теперь срабатывает на PR, которые трогают любой вход архива или образа (`src-tauri/**`, `src/**`, `package.json`, `pnpm-lock.yaml`, `.node-version`, `vite.config.web.mts`, `rust-toolchain.toml`, `scripts/**`, `deploy/**`, `Dockerfile.web*`, сам workflow), а `workflow_dispatch` никогда не публикует: `publish=true` только при push тега, прошедшего проверки (`scripts/release-meta.sh`).
- Первый запуск по тегу будет первым запуском входа в ghcr, push по digest, `imagetools create` и `gh release create`: до тега они не выполнялись. Образ, который уходит в ghcr, пересобирается из кеша, а не тот, что прошёл smoke. rust-cache в workflow не сохраняется ни разу (триггера на `main` нет), поэтому сборки стартуют холодными.
- Образ по умолчанию слушает `HOST=127.0.0.1` без `TRUSTED_TAILNET` и берёт `WEB_DIST=/opt/cc-switch-web/dist-web`. Compose владельца в Dokploy переопределяет HOST/PORT/TRUSTED_TAILNET и использует `WEB_DIST` образа.
- Хвосты фазы: DP-046 (`CC_SWITCH_WEB_UPDATE_REPO` как запасное имя для `CC_SWITCH_WEB_REPO` в `install.sh`), `unreachable!` в `web_update.rs:328`, clippy `--tests` type_complexity в `self_update/tests.rs`.
