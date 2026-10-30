# Phase 36 — WEB-28 / DX-86 (этап 1): проверка обновлений и инструкция

Цель: web-консоль честно показывает, отстаёт ли запущенная сборка сервера от ветки форка, и даёт инструкцию по обновлению для systemd и Docker. Установка из UI — фаза 37; DX-86 после этой фазы остаётся In Progress.

Спека: § WEB-28 (проверка + инструкция — M, 2–4 дн.).

⚠️ Решение, отличающееся от буквы спеки: спека говорит «проверка через release manifest», но у форка нет своих релизов. Updater-endpoints в `tauri.conf.json:64-70` указывают на upstream (`dl.ccswitch.io`, `farion1231/cc-switch`), а версия сервера (`CARGO_PKG_VERSION` = 4.0.20) совпадает с upstream. Сервер собирается из checkout (`scripts/install-server.sh:25-26`, `Dockerfile.web:17-18`). Поэтому сигнал обновления = git SHA сборки против HEAD ветки форка через GitHub compare API. В начале фазы проверить `gh release list -R alxwlw/cc-switch-web`: если релизы появились, источником становится manifest формата `latest.json` (`.github/workflows/release.yml:829-951`), остальная фаза не меняется. Решение зафиксировать комментарием в DX-86.

Scope: `src-tauri/src/services/web_update.rs` (новый), `src-tauri/src/web_api.rs` (роут + SHA в `/api/capabilities`), `scripts/install-server.sh`, `Dockerfile.web` (передача SHA при сборке), `src/lib/web-shims/app.ts`, `src/lib/updater.ts`, `src/contexts/UpdateContext.tsx`, `src/components/settings/AboutSection.tsx`.

Факты разведки:
- `updater.ts:25` `checkForUpdate` не использует `invoke`: динамический `import("@tauri-apps/plugin-updater").check()`; фаза 6 сделала в web явный статус `unsupported`. `UpdateContext.tsx:62-97` (`checkUpdate`, `hasUpdate`/`updateInfo{currentVersion,availableVersion,notes,pubDate}`), `:119-127` (автопроверка через 1 с после старта), отказ от уведомления хранится в localStorage.
- `AboutSection.tsx:138` (portable), `:150` (`installUpdateAndRestart`), `:161` (`checkUpdates`).
- Версия: `web-shims/app.ts:2` `getVersion()` захардкожен `"4.0.20-web"`; `/api/capabilities` (web_api.rs:147-153) отдаёт `env!("CARGO_PKG_VERSION")` и `runtime_boundaries` с «no auto-update».
- `install_update_and_restart` (commands/settings.rs:277) — Tauri updater + `restart_process`, в web не переносится. `open_portable_update_download` (misc.rs:110) ведёт на релизы upstream — в web не нужен.
- Исходящие запросы — через глобальный клиент `proxy::http_client::get()` (с фазы 24 инициализируется в bootstrap).

### Task 1: SHA сборки
- [ ] `services/web_update.rs`: `pub const BUILD_SHA: Option<&str> = option_env!("CC_SWITCH_WEB_GIT_SHA");`. `scripts/install-server.sh` и `Dockerfile.web` передают `CC_SWITCH_WEB_GIT_SHA=$(git rev-parse HEAD)` в `cargo build`.
- [ ] `/api/capabilities` дополнительно отдаёт `buildSha` (`null`, если не задан); `runtime_boundaries` — убрать «no auto-update», написать «updates: check in UI, install by operator».

### Task 2: проверка обновлений
- [ ] `pub async fn check(cfg: &UpdateSourceConfig) -> Result<UpdateCheck, AppError>`; `UpdateSourceConfig {api_base, repo, branch}` из env `CC_SWITCH_WEB_UPDATE_API` (по умолчанию `https://api.github.com`), `CC_SWITCH_WEB_UPDATE_REPO` (`alxwlw/cc-switch-web`), `CC_SWITCH_WEB_UPDATE_BRANCH` (`main`). Запрос `GET {api_base}/repos/{repo}/compare/{BUILD_SHA}...{branch}`.
- [ ] `UpdateCheck` (camelCase): `{status: "up-to-date"|"behind"|"unknown", currentVersion, currentSha, latestSha, behindBy, commits: [{sha, message, date}] (не больше 20), checkedAt, instructions: {systemd, docker}}`. `unknown` — `BUILD_SHA` не задан или SHA нет в ветке (404 от compare).
- [ ] Кэш результата в памяти на 1 час (лимит GitHub без токена — 60 запросов в час); `?force=1` обходит кэш.
- [ ] Тексты инструкций: systemd — `cd <checkout> && git pull --ff-only && scripts/install-server.sh`; Docker — пересборка образа из `Dockerfile.web` и пересоздание контейнера.
- [ ] `GET /api/update/check` → `ok(UpdateCheck)`.
- [ ] Тесты (`tests/web_api_update.rs`, заглушка GitHub API на axum через `CC_SWITCH_WEB_UPDATE_API`): `ahead_by: 3` → `behind`, `behindBy: 3`, три коммита; `ahead_by: 0` → `up-to-date`; 404 → `unknown`; 403 rate limit → `{ok:false}` с понятным текстом; второй запрос в пределах часа не идёт в заглушку.

### Task 3: frontend
- [ ] `web-shims/app.ts`: `getVersion()` берёт `version` из `/api/capabilities` вместо хардкода.
- [ ] `updater.ts`: web-ветка вызывает `invoke("web_check_update")` (adapter → `GET /api/update/check`) и маппит `behind` в `hasUpdate` с `availableVersion = "<version>+<latestSha[0..7]>"`; статус `unsupported` из фазы 6 для проверки убрать, установку оставить недоступной.
- [ ] `AboutSection`: в web — блок «сборка отстаёт на N коммитов» со списком коммитов и инструкцией (кнопка копирования через `copyText` из `src/lib/clipboard.ts`, с фоллбеком фазы 25 для http-origin); кнопка install скрыта до фазы 37.
- [ ] Тесты `UpdateContext`/`AboutSection` (web): `behind` → баннер и инструкция; `up-to-date` → без баннера; `unknown` → «версию сборки определить нельзя».

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server` с `CC_SWITCH_WEB_GIT_SHA`.
- [ ] Ветка `web-parity/web-28-update-check`, PR; DX-86 остаётся In Progress, комментарий со ссылкой на PR, «этап 1 из 2» и решением про источник обновлений.

Риски для ревью:
- Сборка из грязного checkout или локального коммита, которого нет на origin, → compare 404 → `unknown`, а не ложный `up-to-date`.
- Автопроверка из каждой открытой вкладки не должна выжечь лимит GitHub: кэш на сервере общий для всех клиентов.

## Notes for next phase

- (заполнить по итогам)
