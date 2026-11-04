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
- [x] Полный набор гейтов + `cargo build --release --example server` с `CC_SWITCH_WEB_GIT_SHA`. После сборки с переменной: запустить пример-сервер и `curl /api/capabilities` должен показать `buildSha`, равный этому SHA; после пересборки без переменной тот же `curl` показывает `buildSha: null`. Это единственная проверка, что обработчик берёт значение из `web_update::build_sha()`, а `option_env!` читает переменную: в обычной сборке тесты сравнивают `null` с `null`.
- [ ] Ветка `web-parity/web-28-update-check`, PR; DX-86 остаётся In Progress, комментарий со ссылкой на PR, «этап 1 из 2» и решением про источник обновлений.

Риски для ревью:
- Сборка из грязного checkout или локального коммита, которого нет на origin, → compare 404 → `unknown`, а не ложный `up-to-date`.
- Автопроверка из каждой открытой вкладки не должна выжечь лимит GitHub: кэш на сервере общий для всех клиентов.

## Notes for next phase

**Статус.** Ветка `web-parity/web-28-update-check` (base `main`), до merge; PR открывает контроллер после финального ревью. DX-86 остаётся In Progress: это этап 1 из 2 (проверка и инструкция), установка из консоли — фаза 37. Отметка «Фаза 35» в индексе стоит в первом коммите ветки (`eaacded7`), отметка «Фаза 36» — первым коммитом ветки фазы 37.

**Что сделано (file:line на HEAD ветки).**
- T1 SHA сборки: `services/web_update.rs:104` (`build_sha()` поверх `option_env!("CC_SWITCH_WEB_GIT_SHA")`, пустая строка → `None`, `normalize_sha` `:110`); `web_api.rs:732` (`capabilities_payload(build_sha)` с `buildSha`), `:744` (`capabilities()` берёт значение из `web_update::build_sha()`), `runtime_boundaries` `:740` («updates: check in UI, install by operator»). `scripts/install-server.sh:20-55` задаёт переменную только из корня git-checkout без изменённых tracked-файлов, иначе `unset` и предупреждение; `Dockerfile.web:17-20` (`ARG CC_SWITCH_WEB_GIT_SHA=` → `cargo build`); `docs/deploy.md` (раздел про консоль и build-arg).
- T2 проверка: `web_update.rs:205` (`check(cfg, build_sha, force)`), `:23` (`UpdateSourceConfig`, `:32` `from_env`: `CC_SWITCH_WEB_UPDATE_API|REPO|BRANCH`, читается на каждый запрос), `:234` (`get_json`: `http_client::get_for_url`, таймаут 15 с `:18`, 403/429 → понятная ошибка, 404/422 → `unknown`), `:293` (`fetch`: `behind_by > 0` → `unknown`, затем `ahead_by`), `:19` (не больше 20 коммитов, новые первыми, первая строка), `:173` (`static CACHE` под `tokio::sync::Mutex`, ключ api/repo/branch/sha, TTL `:16` = 1 час, ошибки не кэшируются). Роут `web_api.rs:106` (`/api/update/check`), `:755` (`update_check`, `?force=1`), `:767` (`update_check_with`). Тесты `src-tauri/tests/web_api_update.rs` (заглушка GitHub на axum, 403 без User-Agent) и модульные в `web_api.rs:5151-5235`.
- T3 frontend: `web-shims/app.ts:8` (`getVersion()` из `/api/capabilities`); `adapter.ts` — `web_check_update` (`force=1` только у ручной кнопки); `updater.ts:24` (`WebUpdateStatus`), `:74` (`parseWebUpdateCheck`, строгий разбор), `:148` (`invoke("web_check_update", {force})`), `:152` (`unknown`); `UpdateContext.tsx:34` (`checkUpdate({force})`), `:49` (`webUpdate`), `:136` (автопроверка без `force`; `unsupported` из фазы 6 убран); `AboutSection.tsx:50` (`WebUpdateStatus`), `:59` (`handleCopy` через `copyText`), `:104` (`unknown`), `:114` (`up-to-date`), `:131-151` (N, «показано N из M», список), `:433` (`!isWeb && hasUpdate` — кнопка install в web скрыта), `:249-259` (ссылка Release notes от текущей `version`, без `+sha`); `Sidebar.tsx:389,767` и `App.tsx:863` (точка и переход в About по `hasUpdate`); `lib/whatsNew.ts:38-46` (`4.0.20` против записанного `4.0.20-web` не показывает «Что нового» заново); ключ `web.update.unsupported` убран, новые строки во всех 5 локалях. Строка таблицы фазы 6 про обновления отмечена закрытой.
- T4: гейты и живая проверка ниже; эти Notes.

**Отклонения от плана (журнал `.claude/plans/2026-10-25-web-parity-m5-p36/journal.md`).**
- Ruling 08:31 — `gh release list` пуст, источник обновлений остаётся compare API (SHA сборки против `main`), release manifest не используется (решение зафиксировано для комментария в Plane).
- Вместо константы `BUILD_SHA` — функция `build_sha()` с нормализацией пустого значения (Docker ARG по умолчанию пуст). `Dockerfile.web` и `docs/deploy.md` добавлены в scope.
- T1 f1 — `pub use services::web_update` в `lib.rs` ради теста не добавлен; привязка хендлера к `web_update::build_sha()` проверяется в крейте (`web_api.rs:5235`, красная при подмене) и curl-гейтом T4 (ниже): мутант «хендлер передаёт `None`» в обычной сборке без SHA не виден.
- T2 — статус считается по обоим счётчикам (`behind_by > 0` → `unknown`, иначе ложный `up-to-date` для сборки с коммитами не из ветки); ответ без счётчиков (`{}` с кодом 200) — ошибка, а не закэшированный `up-to-date`; `check` принимает `build_sha` и `force` параметрами (тесты собираются без SHA).
- T3 r0 (ruling 11:20) — в web Release notes при `behind` строит ссылку от текущей `version`, как при `up-to-date`; список коммитов уже в блоке, compare-URL форка не добавляли. Сброс `hasUpdate` при rate limit на ручной перепроверке принят: ошибка видна, как на desktop.
- T4 — `whatsNewSeenVersion` хранится не в БД, а в `settings.json` (`~/.cc-switch/settings.json` под `HOME` процесса): проверка f сделана правкой этого файла во временном `HOME`, в копии БД записи нет.

**Гейты (логи в scratchpad, строго по одному).**
- `cargo fmt --check`: rc=0. `cargo clippy --lib --examples -- -D warnings`: rc=0.
- `cargo test --no-fail-fast` (umask 022): сумма строк `test result` скриптом (awk) — 65 строк, 4678 passed, 1 failed, 10 ignored; единственный fail — известный `commands::misc::tests::omp_search_paths_include_the_bun_global_bin`.
- `pnpm typecheck`: rc=0. `pnpm format:check`: `All matched files use Prettier code style!`.
- `pnpm test:unit`: `Test Files 2 failed | 329 passed (331)`, `Tests 3 failed | 4142 passed (4145)`; ровно известные 3: `UsageDashboard` (2) и `remembered usage choices › keeps working when storage is unavailable`.
- `pnpm build:renderer`: `✓ built in 11.83s`. `pnpm build:web`: `✓ built in 9.86s`.
- SHA-гейт (единственная проверка `option_env!` и хендлера): `CC_SWITCH_WEB_GIT_SHA=f89b3703070b89337b56cb8de94add420e27dc16 cargo build --release --example server` (`Finished … in 2m 50s`) → `curl /api/capabilities` → `buildSha: "f89b3703070b89337b56cb8de94add420e27dc16"` (= `git rev-parse HEAD`). Пересборка с `env -u CC_SWITCH_WEB_GIT_SHA` (`1m 49s`) → `buildSha: null`. Ещё две сборки с SHA (`e4dc248d`, `749133e8`) для живой проверки, оба раза `buildSha` равен переменной.

**Живая проверка** (настоящий Chromium через Playwright MCP, release-бинарники ветки, `127.0.0.1:18799`, `HOME` и `CC_SWITCH_CONFIG_DIR` во временных каталогах, копия БД через sqlite3 backup API; реальных запросов к GitHub 4: по одному на запуск сервера).
- a. Behind, сборка `e4dc248d` (`origin/main~25`), настоящий GitHub: статус `behind`, `behindBy` 279 (счёт по всем коммитам, включая слитые; `latestSha` = `749133e8`), 20 коммитов в ответе. About: «This build is 279 commit(s) behind the branch», «Build e4dc248 · Checked …», «New commits on the branch · Showing 20 of 279», список (sha7 + первая строка), обе инструкции (systemd и Docker), кнопок install нет. Copy на `127.0.0.1`: в буфере ровно текст systemd-инструкции, затем Docker-инструкции (`clipboard.readText`), toast «Copied to clipboard». На http-origin `100.123.155.62` (`TRUSTED_TAILNET=1`): `isSecureContext=false`, `navigator.clipboard` не определён, Copy вызывает `execCommand("copy")` на textarea с текстом инструкции, toast «Copied to clipboard». Зелёная точка на «Settings» в сайдбаре; клик открывает About. Release notes → `https://github.com/farion1231/cc-switch/releases/tag/v4.0.20` (без `+`).
- b. Up to date, сборка `749133e8` (HEAD `origin/main`), настоящий GitHub: статус `up-to-date`, `behindBy` 0; About «This build matches the fork's branch», баннера и списка нет, точки на «Settings» нет (скриншот сайдбара).
- c. Unknown: сборка без SHA с заглушкой вместо GitHub — `buildSha: null`, About «The build version cannot be determined», автопроверка и ручная кнопка дали 0 запросов `compare` к заглушке. Сборка на HEAD ветки `f89b3703` (коммиты не из `main`, `behind_by > 0`), настоящий GitHub: `unknown`, не `up-to-date`; About показывает «Build f89b370».
- d. Rate limit (заглушка отвечает 403): About «Update check failed» + текст сервера «The update source refused the check (HTTP 403): the GitHub rate limit is probably reached…», слов «up to date» нет; то же при ручной перепроверке после успешной.
- e. Кэш: заглушка `ok`, 4 загрузки страницы в первой вкладке и 2 во второй (6 запросов `GET /api/update/check`) → к заглушке ушёл 1 запрос (счётчик 2 → 3). Ручная кнопка отправила `GET /api/update/check?force=1` → счётчик 3 → 4.
- f. What's New: контроль — `whatsNewSeenVersion` = `4.0.19-web` показывает диалог «Updated to v4.0.20 / What changed since v4.0.19-web»; при `4.0.20-web` диалога нет, `GET /api/settings` возвращает `4.0.20-web`. Версия в About «v4.0.20» (с сервера; раньше шим отдавал `4.0.20-web`).
- g. 390 px (состояние `behind`, 5 коммитов от заглушки и 20 от настоящего GitHub): `documentElement.scrollWidth` = 390, максимум правой границы элементов в `main` = 390.
- Очистка: серверы и заглушка остановлены, копия БД и временный `HOME` удалены, `.playwright-mcp/` удалён, `cc-switch-web.service` не тронут (`ExecMainStartTimestamp` до и после: Wed 2026-11-04 08:27:18 EET). `scripts/install-server.sh` не запускался.

**Не проверялось.** Docker (`Dockerfile.web` с build-arg) не собирался и не запускался. `scripts/install-server.sh` в живую не запускался (боевой сервис): ветка с `CC_SWITCH_WEB_GIT_SHA` и `unset` для грязного дерева проверена только чтением и сборками вручную; первая сборка через скрипт будет при деплое. Содержимое буфера на http-origin не считывалось (в небезопасном контексте API чтения нет): проверен вызов `execCommand("copy")` на textarea с нужным текстом и toast. Real GitHub на `main` без SHA сборки не опрашивался (0 запросов — так и задумано). Другие браузеры (Safari, Firefox) не проверялись. Проверка таймаута 15 с и недоступности сети вживую не делалась (покрыта тестами).

**Известные ограничения.**
- Ошибка проверки (403) приходит как `{ok:false}` с HTTP 500 (общая форма ошибок `web_api`); в консоли браузера остаются 3 ERROR-записи на каждую неудачную проверку (ресурс 500, `检查更新失败`, повтор). Текст пользователю показывается корректно.
- `behindBy` — `ahead_by` compare API: считает все коммиты, достижимые из `main` и не из сборки, включая коммиты внутри слитых PR (279 при 25 первых родителях). «N commit(s)» читается как «N коммитов в истории», не «N PR».
- Кнопка Release notes в web открывает вкладку, но показывает toast «не удалось открыть» (`adapter.ts:1396-1402`): `window.open(..., "noopener,noreferrer")` всегда возвращает `null`, и код принимает это за блокировку popup. Дефект есть на `main`, фаза его не меняла. Стоит отдельной задачей.
- Кэш — в памяти процесса: после restart первая загрузка идёт в GitHub; лимит 60 запросов в час на IP сервера общий для всех клиентов. После rate limit ошибка не кэшируется, каждая автопроверка вкладки снова идёт в GitHub, пока лимит не снят (по одному запросу на загрузку страницы).
- Release notes ведут на страницу тега апстрима `farion1231/cc-switch` текущей версии, не форка (у форка релизов нет).
- Сборка из грязного дерева или локального коммита, которого нет на origin, даёт `unknown` («определить нельзя»), а не `behind`.

**Что может переиспользовать фаза 37.** `web_update::{build_sha, check, UpdateSourceConfig}` и `instructions()` (`web_update.rs:89`, тексты инструкций те же); `buildSha` в `/api/capabilities` для сверки с новым HEAD после установки и restart (опрос — `useServerRestart` фазы 35, `instanceId`); `?force=1` для перепроверки после установки; `webUpdate` в `UpdateContext` и блок `AboutSection.tsx:131-190` (кнопку install добавлять рядом, условие `!isWeb && hasUpdate` на `:433` сейчас скрывает её в web); точка в сайдбаре и переход в About уже работают от `hasUpdate`.

**Post-merge.**
1. Пересобрать server и `dist-web` через `scripts/install-server.sh` (чистое дерево на `main`, чтобы собралась с SHA), не вручную.
2. Бэкап БД `~/.cc-switch-web/backups-predeploy-p36-<дата>/` перед деплоем.
3. Миграций БД нет: `git diff main --stat -- src-tauri/src/database` пуст (проверено на HEAD ветки).
4. Smoke на боевом сервисе: `GET /api/capabilities` → `buildSha` = HEAD `main` после merge; `GET /api/update/check` → `up-to-date`; About показывает «This build matches the fork's branch», точки нет. Бюджет: 1–2 запроса к GitHub.
5. Закрытие фазы 36 (отметка в индексе) — первым коммитом ветки фазы 37.
