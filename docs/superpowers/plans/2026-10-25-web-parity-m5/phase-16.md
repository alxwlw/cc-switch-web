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
- [ ] `POST /api/openclaw/import-live`, `GET /api/openclaw/live-provider-ids`, `GET /api/openclaw/live-provider/:id`, `GET /api/openclaw/health`, `GET/PUT /api/openclaw/{default-model|model-catalog|agents-defaults|env|tools}` (тело `{model}`/`{catalog}`/`{defaults}`/`{env}`/`{tools}`). Команды без tauri-типов вызывать напрямую.
- [ ] `src-tauri/tests/web_api_openclaw.rs` (temp-каталог OpenClaw): каждая get/set пара — round-trip, неизвестные поля конфига (`flatten`) сохраняются после set; health на конфиге с известной проблемой возвращает warning; import-live импортирует N провайдеров.

### Task 2: backend workspace — роуты + тесты
- [ ] `GET/PUT /api/openclaw/workspace/:filename` (`{content}`), `GET /api/openclaw/daily-memory`, `GET/PUT/DELETE /api/openclaw/daily-memory/:filename`, `GET /api/openclaw/daily-memory/search?q=`. Хендлеры вызывают команды workspace.rs напрямую — валидация имени остаётся единственной, в командах.
- [ ] `src-tauri/tests/web_api_workspace.rs`: `../../etc/passwd`, `%2e%2e%2fsecret.md`, `AGENTS.txt`, `notes.md` → 400/`{ok:false}` и файл вне корня не создан; `2026-13-45.md` проходит regex — зафиксировать фактическое поведение команды тестом; write → read; после write в каталоге нет временных файлов (проверка атомарности `write_text_file`; если он не атомарен — сделать атомарным в этой фазе); search находит подстроку; delete.

### Task 3: frontend
- [ ] adapter: case для всех команд, кроме `open_workspace_directory`.
- [ ] Кнопку «открыть каталог» в web заменить показом серверного пути (`get_openclaw_dir`) с копированием.
- [ ] Тесты: `tests/lib/openclawApi.test.ts`, `tests/lib/workspaceApi.test.ts`; существующие компонентные тесты зелёные.
- [ ] Снять WEB-00-гейты (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-15-openclaw`, PR; DX-73 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Symlink внутри каталога workspace, указывающий наружу (`MEMORY.md -> /etc/…`): allowlist по имени его не ловит. Тест фиксирует поведение; если запись идёт по ссылке наружу — проверять canonical path.

## Notes for next phase

- (заполнить по итогам)
