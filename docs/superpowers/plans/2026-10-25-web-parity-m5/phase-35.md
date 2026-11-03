# Phase 35 — WEB-26 / DX-84: смена data directory и restart

Цель: в web видно, какой каталог данных использует сервер и откуда он взят. Если каталог не зафиксирован оператором через env, его можно сменить текстовым путём. Перезапуск сервера доступен из UI, когда процесс работает под supervisor (systemd/Docker).

Спека: § WEB-26. Оценка M (3–5 дн.).

Scope: `src-tauri/src/app_store.rs` (headless-бэкенд override), `src-tauri/src/config.rs:318` (резолвер не меняется по порядку), `src-tauri/src/bootstrap.rs` (загрузка override до `Database::init`), `src-tauri/src/web_api.rs` (3 роута), `src-tauri/examples/server.rs` (graceful shutdown + restart), `src/lib/api/adapter.ts`, `src/components/settings/SettingsPage.tsx:274-310,408-448`, `src/hooks/useDirectorySettings.ts`, `docs/deploy.md`.

Факты разведки:
- Резолвер `config::get_app_config_dir()` (config.rs:318): 1) env `CC_SWITCH_CONFIG_DIR` (непустой), 2) кэш override из Tauri Store (`app_store::get_app_config_dir_override`, app_store.rs:24), 3) `~/.cc-switch`. От него зависят `Database::init` (`<dir>/cc-switch.db`, database/mod.rs:100), лог сервера и OAuth-каталоги. Не зависят: `settings.json` (settings.rs:716), `live-state.json` (live/engine.rs:36) и device backups всегда лежат в `<home>/.cc-switch` — это хранилище устройства upstream, поведение не меняем. Сообщения UI и `docs/deploy.md` не обещают, что переезжает всё (проверить список при T5).
- Production-unit (`scripts/cc-switch-web.service.template`, рендерит `scripts/install-server.sh`) задаёт `CC_SWITCH_CONFIG_DIR=%h/.cc-switch-web` и `Restart=always`, `RestartSec=3`. В сервере кэш Store всегда None — override не читается.
- Desktop: `get_app_config_dir_override(app)` (commands/settings.rs:375) / `set_app_config_dir_override(app, path)` (:384) → `app_store::set_app_config_dir_to_store` пишет Tauri Store `app_paths.json`, ключ `app_config_dir_override` (пустой путь удаляет ключ; при чтении несуществующий путь игнорируется с warning, `~` раскрывается). DB при смене не копируется — новая директория начинает с того, что в ней лежит.
- `restart_app(app)` (settings.rs:253): `save_window_state_before_exit` → `cleanup_before_exit(&app)` (восстанавливает takeover/live-бэкапы) → `restart_process` (lib.rs:2375). `pick_directory` (config.rs:255) — native dialog. `is_portable_mode` (misc.rs:126) — handle-free (`portable.ini` рядом с exe).
- Фронт: `useSettings.ts:399` (`setAppConfigDirOverride` после save), `:515` (`requiresRestart`), `useDirectorySettings.ts:211` (чтение override), `:392,:411` (picker), `SettingsPage.tsx:288-307` (`handleRestartNow` → `settingsApi.restart()`), `:408-448` (`DirectoryInput` с browse/reset). В web не замаплены: `restart_app`, `get/set_app_config_dir_override`, `pick_directory`, `is_portable_mode`.
- Graceful shutdown с восстановлением takeover в headless сделан в фазе 27 (handle-free ядро `cleanup_before_exit`) — переиспользовать.

Уточнения контроллера (разведка 2026-11-03, `main@2054627e`). Там, где они расходятся с задачами ниже, действуют они:

- **Общее.** Ядро cleanup называется `bootstrap::shutdown_cleanup(&AppState)` (`bootstrap.rs:767`), тест takeover — `mode/controller.rs:3173`, обёртка с таймаутом — `bounded_shutdown_cleanup` (`examples/server.rs:60`). State — `SharedState = Arc<AppState>` (`web_api.rs:36`). В `web_api` нет проверки Origin/CSRF. В оболочке агента задан `INVOCATION_ID`: тесты «без supervisor» обязаны снимать обе переменные и идти под `#[serial]`.
- **T1, путь файла.** `dirs::config_dir` в проекте не используется, `CC_SWITCH_TEST_HOME` на него не действует, а при заданном `XDG_CONFIG_HOME` тест ушёл бы в настоящий конфиг. Функция пути: при `test_home_override` — `<test_home>/.config/cc-switch-web/app_paths.json`, иначе `dirs::config_dir()/cc-switch-web/app_paths.json`.
- **T1, порядок.** `load_headless_override()` — первым шагом `run_headless_bootstrap_with`, до `BootstrapPaths::resolve()` (а не просто до `Database::init`); проверить, что в `examples/server.rs` никто не зовёт `get_app_config_dir` раньше. Desktop-путь её не вызывает. Битый JSON или несуществующий путь — warning и `None`, сервер стартует.
- **T1/T2, кэш.** Кэш заполняет только `load_headless_override`. `PUT` пишет файл и кэш не трогает: в работающем процессе `get_app_config_dir()` не меняется до restart.
- **T2, `GET`.** `effective` и `source` — про текущий процесс (env / кэш / default). `override` — значение из файла, то есть то, что применится после restart. Дополнительное поле `default` (`get_home_dir().join(".cc-switch")`): web-shim `homeDir` годится только для показа.
- **T2, `PUT`.** Проверка схемы — `Database::stored_user_version_exceeds_supported(&Path)` (`database/mod.rs:173`), не `precheck_db_version`: у той побочный эффект `init_status` на работающем сервере. Файл не SQLite, путь на файл, относительный путь и `~user` → 400. Пустая строка после trim = `null` (как на desktop). Хранится раскрытый абсолютный путь; `~` раскрывается через `get_home_dir`. Проверка записи не оставляет файлов в каталоге.
- **T3, выход.** Restart идёт тем же путём, что SIGTERM: `shutdown_signal` ждёт ещё и `ShutdownHandle`, дальше `bounded_shutdown_cleanup` и drain; `process::exit(75)` — в `main` на обеих ветках `select!` (`server.rs:207-219`), не в хендлере.
- **T3, роут.** Сигнатура `web_api::router(state)` не меняется (её зовут десятки тестов), handle передаётся отдельно; без handle — 409. Порядок проверок: `require_json_content_type` (`web_api.rs:4652`, 415: без неё POST без тела с чужого сайта перезапускает сервер, тест обязателен) → supervisor (409) → 202. Повторный запрос во время shutdown — снова 202, без паники.
- **T3, `instanceId`.** Cleanup идёт до того, как axum перестаёт принимать соединения (`server.rs:201-206`), поэтому старый процесс отвечает на `/api/health` ещё до 8 с. Случайный на процесс `instanceId` добавляется в `/api/health` и в тело 202. `init_error_router` не меняется: там `/api/health` = 503, это закреплено в `tests/web_api_init_status.rs:62`.
- **T4, команды.** Отдельная web-команда для полного ответа `GET` (`envLocked`, `source`, `effective`, `default`). При `envLocked` `useSettings.ts:403` не вызывает `PUT`, иначе каждое сохранение настроек получит 409. Снять гейты `useDirectorySettings.ts:214`, `useSettings.ts:403`, `SettingsPage.tsx:418`. Browse в `DirectorySettings.tsx` сейчас `disabled` с подсказкой — скрыть в web. Новый проп read-only; при `envLocked` reset тоже скрыт. Строки — во все 5 локалей. Строки `:94`, `:97` таблицы фазы 6 отметить закрытыми.
- **T4, опрос.** `/api/health` — обычный `fetch` (ответ без конверта `ok`). `location.reload()`: 200 с другим `instanceId` или 503 (новый процесс в init-error). Сетевая ошибка — опрос продолжается. Через 60 с — toast без reload. `adapter` не отдаёт HTTP-статус: 409 приходит текстом ошибки, в toast идёт он.
- **T5, документация.** Снятие env из unit без заранее записанного override переводит сервер на `~/.cc-switch`, а это может быть каталог desktop (ограничение из Notes фазы 34). Поэтому порядок такой: сначала файл `~/.config/cc-switch-web/app_paths.json`, потом правка unit. `install-server.sh:15` при переустановке возвращает env. Выход из плохого override (экран init-error): удалить файл и перезапустить. Docker: `Dockerfile.web:30` задаёт env, значит env-locked; `CC_SWITCH_WEB_SUPERVISED=1` ставить только вместе с restart policy, в Dockerfile не добавлять.
- **T5, живая проверка.** Боевой `cc-switch-web.service` не трогаем: в нём ещё нет этого кода. Проверка — на сборке ветки во временном user-unit (`systemd-run --user`, `Restart=always`) на копии БД, временном HOME и порту 18799: код выхода 75 и подъём сервиса, повторный клик, активный takeover с побайтовым возвратом live-файлов, смена override с применением после restart. Restart из UI на боевом сервисе — smoke после merge.

### Task 1: headless override
- [x] `app_store`: headless-бэкенд — JSON-файл `app_paths.json` в `dirs::config_dir()/cc-switch-web/` с тем же ключом `app_config_dir_override`; `pub fn load_headless_override() -> Option<PathBuf>` заполняет тот же OnceLock-кэш, что читает резолвер. Запись атомарная (tmp + rename). Порядок резолвера не меняется: env > override > default.
- [x] `bootstrap.rs`: `load_headless_override()` — первым шагом, до `Database::init`.
- [x] Тест: без env и с файлом override → `get_app_config_dir()` указывает на override; с env → env.

### Task 2: роуты data dir
- [x] `GET /api/app-config-dir` → `{effective, source: "env"|"override"|"default", override: string|null, envLocked: bool}`.
- [x] `PUT /api/app-config-dir` (body `{path: string|null}`): при `envLocked` → 409 «каталог задан CC_SWITCH_CONFIG_DIR в unit сервера»; валидация пути — абсолютный или `~`, существует или создаётся, доступен на запись, если в нём есть `cc-switch.db` — её schema version не новее поддерживаемой (та же проверка, что в bootstrap фазы 7); иначе 400 с причиной. `null` удаляет override. Применяется только после restart.
- [x] `is_portable_mode` — в adapter локально `false` (сервер не portable), без роута.

### Task 3: restart под supervisor
- [x] `POST /api/restart`: supervisor определяется по `INVOCATION_ID` (systemd) или `CC_SWITCH_WEB_SUPERVISED=1` (Docker и прочие); без supervisor → 409 «перезапустите сервер вручную». С supervisor — ответ 202, затем graceful shutdown (ядро `cleanup_before_exit` из фазы 27) и `std::process::exit(75)`: код 75 перезапускается и при `Restart=always`, и при `on-failure`.
- [x] Выход через инъецируемый `ShutdownHandle` в state, чтобы тест проверял сигнал, а не убивал процесс.
- [x] Тесты: 409 без supervisor; с `CC_SWITCH_WEB_SUPERVISED=1` сигнал shutdown отправлен после ответа 202; PUT: env-locked → 409, путь без прав на запись → 400, DB с будущей schema → 400, валидный путь → override записан. Тесты restart — `src-tauri/tests/web_api_restart.rs` (handle) и `src-tauri/tests/server_example_restart.rs` (настоящий `examples/server`: 202 и код 75, то же при открытом `/api/events`, SIGTERM → 0, без supervisor 409); тесты PUT уже в `src-tauri/tests/web_api_app_config_dir.rs` (T2). Двойной запрос на настоящем процессе не проверяется (после закрытия listener соединение отклоняется), он закреплён на handle.

### Task 4: frontend
- [ ] adapter: `get_app_config_dir_override` → GET (`pick: "override"`), `set_app_config_dir_override` → PUT, `restart_app` → POST restart.
- [ ] `SettingsPage`/`DirectoryInput` в web: кнопка browse скрыта (native picker), текстовый ввод работает; при `envLocked` поле read-only с пояснением и значением env; показывать `source`.
- [ ] `handleRestartNow` в web: после 202 опрашивать `/api/health` (интервал 1 с, до 60 с), затем `location.reload()`; 409 → toast с текстом сервера.
- [ ] Тесты компонента: env-locked read-only, browse скрыт, restart-поток (mock fetch).

### Task 5: документация, гейты, PR, Plane
- [ ] `docs/deploy.md`: env-locked режим (по умолчанию) и режим override (убрать `CC_SWITCH_CONFIG_DIR` из unit); почему exit 75.
- [ ] Полный набор гейтов + `cargo build --release --example server`; ручная проверка на agentbox: restart из UI поднимает сервис (`systemctl --user status cc-switch-web`).
- [ ] Ветка `web-parity/web-26-data-dir`, PR; DX-84 → In Review + ссылка. Закрыть чекбоксы, Notes (интерфейс restart нужен фазе 37).

Риски для ревью:
- Restart при активном takeover: live-конфиги CLI на сервере должны вернуться в исходное состояние до выхода (тест на ядро cleanup из фазы 27).
- Двойной клик «перезапустить»: второй запрос во время shutdown не должен ронять процесс с паникой.

## Notes for next phase

- (заполнить по итогам)
