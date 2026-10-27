# Phase 16 — WEB-15 / DX-73: OpenClaw config, workspace и memory

Цель: import из live-конфига, live IDs, live provider, health scan, default model, catalog, agents defaults, env, tools OpenClaw, а также workspace-файлы и daily memory (CRUD + поиск) работают в web. HTTP-тесты закрепляют allowlist путей и атомарность записи. «Открыть каталог» в web не переносится.

Спека: § WEB-15. Оценка M (3–5 дн.).

Scope: `src-tauri/src/web_api.rs` (≈21 роут), `src-tauri/tests/web_api_openclaw.rs`, `src-tauri/tests/web_api_workspace.rs`, `src/lib/api/adapter.ts`, тесты фронта.

Факты разведки:
- Фронт openclaw.ts:20-121: `get/set_openclaw_default_model {model}`, `get/set_openclaw_model_catalog {catalog}`, `get/set_openclaw_agents_defaults {defaults}`, `get/set_openclaw_env {env}`, `get/set_openclaw_tools {tools}`, `scan_openclaw_config_health`, `get_openclaw_live_provider {providerId}`; providers.ts:246-268: `get_openclaw_live_provider_ids`, `import_openclaw_providers_from_live`; workspace.ts:20-55: `read_workspace_file {filename}`, `write_workspace_file {filename, content}`, `list_daily_memory_files`, `read_daily_memory_file {filename}`, `write_daily_memory_file {filename, content}`, `delete_daily_memory_file {filename}`, `search_daily_memory_files {query}`, `open_workspace_directory {subdir}`.
- Вызовы: `useOpenClaw.ts:32-88`, `EditProviderDialog.tsx:211`, `ProviderList.tsx:217`, `DailyMemoryPanel.tsx:103,185,213,247,263,363`, `WorkspaceFilesPanel.tsx:66,91`, `WorkspaceFileEditor.tsx:66`.
- Backend commands/openclaw.rs: :17 `import_openclaw_providers_from_live(State<AppState>)` → `services::provider::import_openclaw_providers_from_live(&AppState)`; :24, :32, :40 (live IDs, live provider, health) и :49-127 (get/set пары) — функции без State над `openclaw_config::*`. Handle-free.
- commands/workspace.rs (все async, без State, логика inline): `list_daily_memory_files` :60, `read_daily_memory_file` :99, `write_daily_memory_file` :113, `search_daily_memory_files` :208, `delete_daily_memory_file` :272, `read_workspace_file` :287, `write_workspace_file` :302. Allowlist: workspace-файлы — `ALLOWED_FILES` (:8-18: AGENTS, SOUL, USER, IDENTITY, TOOLS, MEMORY, HEARTBEAT, BOOTSTRAP, BOOT — все `.md`); daily memory — `^\d{4}-\d{2}-\d{2}\.md$` (:33-45). Запись через `config::write_text_file` (атомарность по комментариям, не проверена). Корень — `openclaw_config::get_openclaw_dir()` (openclaw_config.rs:35). `open_workspace_directory(handle: AppHandle, subdir)` (:326) — opener, не переносим.
- DTO: `DailyMemoryFileInfo`, `DailyMemorySearchResult` (workspace.rs:45,:192), `OpenClawHealthWarning`, `OpenClawWriteOutcome` — camelCase; `OpenClawDefaultModel`, `OpenClawModelCatalogEntry`, `OpenClawAgentsDefaults`, `OpenClawEnvConfig`, `OpenClawToolsConfig` — без `rename_all`, с `serde(flatten)`. Ключи тела запроса — `model`, `catalog`, `defaults`, `env`, `tools`.
- Отдельного update у daily memory нет: write = create-or-update.
- Тесты: Rust для OpenClaw/workspace нет (в workspace.rs нет `#[test]`); фронт — `DailyMemoryPanel.test.tsx`, `WorkspaceFileEditor.test.tsx` (mock API).

### Task 1: backend OpenClaw — роуты + тесты
- [x] `POST /api/openclaw/import-live`, `GET /api/openclaw/live-provider-ids`, `GET /api/openclaw/live-provider/:id`, `GET /api/openclaw/health`, `GET/PUT /api/openclaw/{default-model|model-catalog|agents-defaults|env|tools}` (тело `{model}`/`{catalog}`/`{defaults}`/`{env}`/`{tools}`). Команды без tauri-типов вызывать напрямую.
- [x] `src-tauri/tests/web_api_openclaw.rs` (temp-каталог OpenClaw): каждая get/set пара — round-trip, неизвестные поля конфига (`flatten`) сохраняются после set; health на конфиге с известной проблемой возвращает warning; import-live импортирует N провайдеров.

### Task 2: backend workspace — роуты + тесты
- [x] `GET/PUT /api/openclaw/workspace/:filename` (`{content}`), `GET /api/openclaw/daily-memory`, `GET/PUT/DELETE /api/openclaw/daily-memory/:filename`, `GET /api/openclaw/daily-memory/search?q=`, `GET /api/openclaw/workspace-paths`. Хендлеры вызывают команды workspace.rs напрямую — валидация имени остаётся единственной, в командах (400 — через те же валидаторы, см. «Уточнения контроллера»).
- [x] `src-tauri/tests/web_api_workspace.rs`: `../../etc/passwd`, `%2e%2e%2fsecret.md`, `AGENTS.txt`, `notes.md` → 400/`{ok:false}` и файл вне корня не создан; `2026-13-45.md` проходит regex — зафиксировать фактическое поведение команды тестом; write → read; после write в каталоге нет временных файлов (проверка атомарности `write_text_file`; если он не атомарен — сделать атомарным в этой фазе); search находит подстроку; delete.

### Task 3: frontend
- [x] adapter: case для всех команд, кроме `open_workspace_directory`.
- [x] Кнопку «открыть каталог» в web заменить показом серверного пути (`get_openclaw_dir`) с копированием.
- [x] Тесты: `tests/lib/openclawApi.test.ts`, `tests/lib/workspaceApi.test.ts`; существующие компонентные тесты зелёные.
- [x] Снять WEB-00-гейты (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-15-openclaw`, PR; DX-73 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Symlink внутри каталога workspace, указывающий наружу (`MEMORY.md -> /etc/…`): allowlist по имени его не ловит. Тест фиксирует поведение; если запись идёт по ссылке наружу — проверять canonical path.

Уточнения контроллера (разведка на `764d6649`, 2026-10-26) — имеют приоритет над «Фактами разведки» выше:
- Строки в `commands/workspace.rs` сдвинулись (все `pub async fn`, без State): `list_daily_memory_files` :60, `read_daily_memory_file` :119 (→ `Option<String>`), `write_daily_memory_file` :138, `search_daily_memory_files` :194, `delete_daily_memory_file` :289, `read_workspace_file` :310, `write_workspace_file` :327, `open_workspace_directory(AppHandle, subdir)` :346. `ALLOWED_FILES` :10-20, `validate_filename` :22-31, `DAILY_MEMORY_RE` :34-35, `validate_daily_memory_filename` :37 (оба валидатора приватные). В `commands/openclaw.rs` всё sync `pub fn`; State есть только у `import_openclaw_providers_from_live` (:16).
- 400 vs 500 (Т2): ошибка команды через `blocking_api` → `fail()` = 500 (`web_api.rs:42-46`); 400 даёт только `bad_request()` (:49-53). Решение: валидаторы имени сделать `pub(crate)`, хендлер вызывает их до команды и на `Err` отвечает `bad_request`; затем вызывает команду, которая валидирует той же функцией. Логика валидации остаётся одна, новых правил в хендлерах нет.
- Атомарность (Т2): `config::write_text_file` уже атомарен (`config.rs:460-509`: temp `{name}.tmp.{pid}.{ns}.{n}` в том же каталоге → rename). Ничего не менять, только закрепить тестом: после write в каталоге лишь целевой файл (как `tests/web_api_hermes.rs:72-77`).
- Путь каталога (Т2 → Т3): `get_openclaw_dir`-команды нет. `get_config_dir` в web мапится на `GET /api/paths` → `configDir` = каталог cc-switch, а не OpenClaw (`adapter.ts:527-528`, `web_api.rs:1851-1856`); для этой фазы его НЕ использовать. Т2 добавляет `GET /api/openclaw/workspace-paths` → `{workspace, memory}` (абсолютные пути на сервере), вычисленные тем же helper'ом, что и в `open_workspace_directory`: вынести вычисление каталога по `subdir` в общую функцию, без второго `join`.
- Literal `../../etc/passwd` в URL, скорее всего, не матчит `:filename` (ответ 404 от роутера), а `%2e%2e%2fsecret.md` axum декодирует в `../secret.md`, и тот доходит до хендлера (прецедент — `tests/web_api_usage_admin.rs:75`). В тесте зафиксировать фактический статус каждого кейса: для декодированного имени — 400, для literal-пути — не 2xx. В обоих случаях файл вне корня не создан.
- Тестовый каркас (Т1, Т2): зеркалить `tests/web_api_hermes.rs` — `mod support;`, `#[tokio::test] #[serial]`, `reset_test_fs()` + `ensure_test_home()`, каталог `home.join(".openclaw-web-api")` через `update_settings(AppSettings { openclaw_config_dir: Some(..), .. })`, `web_router_with(Arc::new(create_test_state()))`.
- Гейты WEB-00 (Т3): `navigation.ts:62-67` (`workspace`, `openclawConfig` в `WEB_HIDDEN_VIEWS`), `App.tsx:1440` (вкладки sub-view кроме hermes), `App.tsx:1135` (live ids при duplicate), `useOpenClaw.ts:38,51,64,77,90,100` (`!isWeb`), `useProviderActions.ts:98` (catalog/default после add), `ProviderList.tsx:70,73-77,181` (`openclaw` в membership/import-списках, skeleton). Строки фазы 6: `phase-6.md:114,115,119` (+ заметки :161-168). Снимать только гейты, команды которых обслуживают роуты этой фазы; `OpenClawFormFields.tsx:312-318` (fetch models) — вне scope. Каждый снятый гейт закрепить тестом.
- «Открыть каталог» (Т3): кликабельный `<p>` с захардкоженным `~/.openclaw/workspace/` / `.../memory/` + `FolderOpen` в `WorkspaceFilesPanel.tsx:88-96` и `DailyMemoryPanel.tsx:360-368`. В web — серверный путь из `workspace-paths` и кнопка копирования; desktop без изменений.

## Notes for next phase

- Т3 (frontend): сняты гейты `navigation.ts` (workspace/openclawConfig), `App.tsx` (табы openclaw, live ids при duplicate), `useOpenClaw.ts` (6 хуков), `useProviderActions.ts` (catalog/default после add), `ProviderList.tsx` (membership/import/skeleton для openclaw), `presentation.ts` (Remove openclaw больше не disabled — секция «неизвестно» осталась только у opencode; ключ i18n `web.removeUnverifiable` удалён из 5 локалей в fix round 1). «Открыть каталог» в web — `WorkspaceDirLink` (серверный путь из `get_openclaw_workspace_paths` + копирование). `OpenClawFormFields.tsx:312-318` (fetch models) не тронут.
- **Статус до merge (2026-10-26)**: ветка `web-parity/web-15-openclaw` (worktree `.worktrees/web-15`), PR #17 (https://github.com/alxwlw/cc-switch-web/pull/17) открыт; DX-73 In Review (комментарий со ссылкой).
  - Т1 и Т2 одобрены с r0. Concern r0 по Т1 (`put_with_wrong_body_key_is_rejected` оставался зелёным и без PUT-роута) закрыт в Т2: тест проверяет ровно 422.
  - Т3 одобрена с r2. r0: SHOULD-FIX DP-019, строки фазы 6 про openclaw-гейты устарели; NIT DP-029, неиспользуемый ключ `web.removeUnverifiable`; NIT DP-047, `WorkspaceDirLink` копировал тильду при ошибке `getPaths`. Всё закрыто в `afcee314`. r1: остаточный DP-019, две строки docs про ключ, закрыл контроллер в `f22ec77c`.
  - Финальные гейты на `f22ec77c`: `cargo fmt --check` и `cargo clippy -- -D warnings` чисто. `cargo test --no-fail-fast`: 4382 passed / 3 failed — только baseline (omp_search_paths, 2× file_modes::switch_*). `cargo build --release --example server` ок. `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3752 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
- **Post-merge**: пересобрать server и `dist-web` (Rust менялся); перед install — `cp cc-switch-web cc-switch-web.prev`. Миграций БД нет. Smoke: `GET /api/openclaw/{health,live-provider-ids,default-model,workspace-paths,daily-memory}`, `GET /api/openclaw/workspace/notes.md` → 400. DX-73 → Done.
- **Роуты** (`web_api.rs`, блок «OpenClaw (WEB-15)», :277-324):
  - Config: `POST import-live`, `GET live-provider-ids`, `GET live-provider/:id`, `GET health`, `GET|PUT {default-model,model-catalog,agents-defaults,env,tools}` с телами `{model}`/`{catalog}`/`{defaults}`/`{env}`/`{tools}`. Неверный ключ тела даёт plain-text 422 от axum, как у остальных `Json<Req>`-роутов.
  - Workspace: `GET|PUT workspace/:filename`, `GET daily-memory`, `GET|PUT|DELETE daily-memory/:filename`, `GET daily-memory/search?q=`, `GET workspace-paths` → `{workspace, memory}`.
  - 400 на имя: хендлер сначала вызывает `pub(crate)` валидаторы из `commands/workspace.rs`, затем команду. Через `blocking_api` ошибка команды была бы 500.
  - `workspace_subdir()` — общий helper пути для `open_workspace_directory` и `workspace-paths`.
- **Поведение, закреплённое тестами**:
  - literal `../../etc/passwd` → 404 (роутер); `%2e%2e%2f…` и `..%2F…` декодируются axum и дают 400;
  - `2026-13-45.md` проходит regex;
  - после write нет temp-файлов (`write_text_file` атомарен, менять не пришлось);
  - write поверх symlink заменяет ссылку обычным файлом, цель не трогается;
  - read по symlink отдаёт файл вне корня — принято (ruling в journal): через API ссылку не создать, desktop так же. Возможное ужесточение позже: отказ при `symlink_metadata().is_symlink()` в read и search.
- **Ловушка для следующих фаз**: `get_config_dir` в web мапится на `GET /api/paths` → `configDir` cc-switch, игнорируя `app` (`adapter.ts:527-528`, `web_api.rs:1851-1856`). Для путей конкретного приложения нужен свой роут, как `workspace-paths`.
