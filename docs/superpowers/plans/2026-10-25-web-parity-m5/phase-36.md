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

**Уточнения контроллера (2026-11-04, разведка и проба GitHub API; важнее текста задач ниже).** Проверено: `gh release list` пуст, репозиторий публичный. Анонимный compare: без User-Agent → 403, неизвестный SHA → 404, ответ содержит `status`, `ahead_by`, `behind_by`, `commits`.

Task 1:
- Вместо константы — `pub fn build_sha() -> Option<&'static str>` поверх `option_env!`; пустое значение = `None` (у Docker ARG значение по умолчанию пустое).
- В builder-стадии `Dockerfile.web` нет `git`, `.dockerignore` нет. Поэтому `ARG CC_SWITCH_WEB_GIT_SHA=` и передача в `cargo build`; команда сборки образа `--build-arg CC_SWITCH_WEB_GIT_SHA=$(git rev-parse HEAD)` описана в `docs/deploy.md` (файл добавляется в scope).
- `install-server.sh`: SHA передаётся, только если `git -C "$ROOT" rev-parse HEAD` сработал и `git -C "$ROOT" diff --quiet HEAD` чист. Смотрим только отслеживаемые файлы: untracked `.scratch/` есть в боевом checkout. Иначе переменная снимается с предупреждением, и сборка честно даёт `unknown`.
- `build.rs` не трогаем: rustc пишет `option_env!` в dep-info, cargo пересобирает при смене значения. Это проверяется двумя сборками в Task 4.
- `buildSha` приходит из общего хендлера `capabilities()` (`web_api.rs:743`) в обоих роутерах; в `ServerCapabilities` (`src/lib/api/capabilities.ts`) добавить поле.

Task 2:
- Сигнатура `check(cfg, build_sha: Option<&str>, force: bool)`: тесты собираются без SHA, поэтому статусы проверяются вызовом сервиса со SHA. Тест роута: без SHA → `unknown`, заглушка получила 0 запросов. Env читается на каждый запрос.
- Клиент — `http_client::get_for_url` (`get()` не обходит системный proxy для loopback), таймаут запроса 15 с (у клиента 600 с), явные заголовки `User-Agent` и `Accept: application/vnd.github+json`. Заглушка отвечает 403 на запрос без User-Agent, как настоящий GitHub.
- Статус считается по обоим счётчикам: `behind_by > 0` → `unknown` (в сборке есть коммиты не из ветки, включая `diverged`; иначе будет ложный `up-to-date`), 404 и 422 → `unknown`, затем `ahead_by > 0` → `behind`, иначе `up-to-date`. На `diverged` и на сборку впереди ветки — отдельные тесты. `behindBy` = `ahead_by`, не длина списка.
- `commits` приходят от старых к новым, не больше 250: берём 20 последних из полученных, новые первыми, только первую строку сообщения. UI пишет «показано N из M». `latestSha` — последний коммит; если `ahead_by` больше длины списка — `GET {api_base}/repos/{repo}/commits/{branch}` (тест); при `up-to-date` равен `currentSha`.
- Кэш: `static OnceLock<tokio::sync::Mutex<Option<…>>>`, замок держится на время запроса, поэтому параллельные вкладки дают один запрос. Ключ (`api_base`, `repo`, `branch`, sha): заглушки на разных портах не делят кэш, хук сброса не нужен. Ошибки (403/429, сеть) не кэшируются; `unknown` от 404 кэшируется. Тест на параллельные запросы — один hit. Заглушки считают только ожидаемый метод и путь.
- Модель тестов: `Stub`, `EnvGuard`, `#[serial]` из `tests/web_api_usage_live.rs`.

Task 3:
- Шим сейчас отдаёт `4.0.20-web`, сервер — `4.0.20`. Его читают `WhatsNewNotice` (`isNewerThanSeen`) и `FirstRunNoticeDialog` (`whatsNewSeenVersion`). Смена значения не должна заново показать «Что нового» тому, у кого записано `4.0.20-web`: проверить и закрепить тестом.
- `updater.ts`: вариант `{status:"unknown"}` и `force` в `CheckOptions`. Ручная кнопка идёт с `force`, автопроверка без него. Web-данные (`commits`, `instructions`, `behindBy`, `status`) доходят до `AboutSection` через новое поле контекста. `unsupported` и пропуск автопроверки в web (`UpdateContext.tsx:125,139`) удаляются, моки в `tests/components/WebShellGating.test.tsx` и `tests/components/shell/SidebarToday.test.tsx` обновляются.
- Ошибка проверки (rate limit) показывается текстом сервера и не выглядит как «актуальная версия». `AboutSection:233` заменяется, неиспользуемый ключ `web.update.unsupported` убирается из 5 локалей, новые строки — во всех 5.
- Зелёная точка в сайдбаре и переход в About (`App.tsx:863`, `Sidebar.tsx:227`) в web начинают работать при `behind`: закрепить тестом. Строку таблицы фазы 6 про обновления отметить закрытой.

### Task 1: SHA сборки
- [x] `services/web_update.rs`: `pub const BUILD_SHA: Option<&str> = option_env!("CC_SWITCH_WEB_GIT_SHA");`. `scripts/install-server.sh` и `Dockerfile.web` передают `CC_SWITCH_WEB_GIT_SHA=$(git rev-parse HEAD)` в `cargo build`.
- [x] `/api/capabilities` дополнительно отдаёт `buildSha` (`null`, если не задан); `runtime_boundaries` — убрать «no auto-update», написать «updates: check in UI, install by operator».

### Task 2: проверка обновлений
- [x] `pub async fn check(cfg: &UpdateSourceConfig) -> Result<UpdateCheck, AppError>`; `UpdateSourceConfig {api_base, repo, branch}` из env `CC_SWITCH_WEB_UPDATE_API` (по умолчанию `https://api.github.com`), `CC_SWITCH_WEB_UPDATE_REPO` (`alxwlw/cc-switch-web`), `CC_SWITCH_WEB_UPDATE_BRANCH` (`main`). Запрос `GET {api_base}/repos/{repo}/compare/{BUILD_SHA}...{branch}`.
- [x] `UpdateCheck` (camelCase): `{status: "up-to-date"|"behind"|"unknown", currentVersion, currentSha, latestSha, behindBy, commits: [{sha, message, date}] (не больше 20), checkedAt, instructions: {systemd, docker}}`. `unknown` — `BUILD_SHA` не задан или SHA нет в ветке (404 от compare).
- [x] Кэш результата в памяти на 1 час (лимит GitHub без токена — 60 запросов в час); `?force=1` обходит кэш.
- [x] Тексты инструкций: systemd — `cd <checkout> && git pull --ff-only && scripts/install-server.sh`; Docker — пересборка образа из `Dockerfile.web` и пересоздание контейнера.
- [x] `GET /api/update/check` → `ok(UpdateCheck)`.
- [x] Тесты (`tests/web_api_update.rs`, заглушка GitHub API на axum через `CC_SWITCH_WEB_UPDATE_API`): `ahead_by: 3` → `behind`, `behindBy: 3`, три коммита; `ahead_by: 0` → `up-to-date`; 404 → `unknown`; 403 rate limit → `{ok:false}` с понятным текстом; второй запрос в пределах часа не идёт в заглушку.

### Task 3: frontend
- [x] `web-shims/app.ts`: `getVersion()` берёт `version` из `/api/capabilities` вместо хардкода.
- [x] `updater.ts`: web-ветка вызывает `invoke("web_check_update")` (adapter → `GET /api/update/check`) и маппит `behind` в `hasUpdate` с `availableVersion = "<version>+<latestSha[0..7]>"`; статус `unsupported` из фазы 6 для проверки убрать, установку оставить недоступной.
- [x] `AboutSection`: в web — блок «сборка отстаёт на N коммитов» со списком коммитов и инструкцией (кнопка копирования через `copyText` из `src/lib/clipboard.ts`, с фоллбеком фазы 25 для http-origin); кнопка install скрыта до фазы 37.
- [x] Тесты `UpdateContext`/`AboutSection` (web): `behind` → баннер и инструкция; `up-to-date` → без баннера; `unknown` → «версию сборки определить нельзя».

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server` с `CC_SWITCH_WEB_GIT_SHA`. После сборки с переменной: запустить пример-сервер и `curl /api/capabilities` должен показать `buildSha`, равный этому SHA; после пересборки без переменной тот же `curl` показывает `buildSha: null`. Это единственная проверка, что обработчик берёт значение из `web_update::build_sha()`, а `option_env!` читает переменную: в обычной сборке тесты сравнивают `null` с `null`.
- [ ] Ветка `web-parity/web-28-update-check`, PR; DX-86 остаётся In Progress, комментарий со ссылкой на PR, «этап 1 из 2» и решением про источник обновлений.

Риски для ревью:
- Сборка из грязного checkout или локального коммита, которого нет на origin, → compare 404 → `unknown`, а не ложный `up-to-date`.
- Автопроверка из каждой открытой вкладки не должна выжечь лимит GitHub: кэш на сервере общий для всех клиентов.

## Notes for next phase

- (заполнить по итогам)
