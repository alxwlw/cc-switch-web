# Phase 7 — WEB-32 / DX-90: общий headless startup/bootstrap и migrations

Цель: web-сервер при старте выполняет те же миграции/seed/инициализацию, что и desktop setup: legacy `config.json`→DB, default skills seed/migration, official providers, first-run импорты live-конфигов, crash-recovery. Общий bootstrap вынести из Tauri `setup` в handle-free модуль — НЕ копировать целиком; desktop-поведение (диалоги, окна) сохранить. Фундамент для WEB-01/05/11/23/26/28.

Спека: § WEB-32. Оценка M (3–5 дн.). Приоритет high.

Scope: новый `src-tauri/src/bootstrap.rs` (или `bootstrap/`), `src-tauri/src/lib.rs` (setup вызывает общий код), `src-tauri/examples/server.rs` (64-66 → bootstrap), `web_api.rs` (роут init-статуса), тесты `src-tauri/tests/bootstrap*.rs`.

Факты разведки — что делает Tauri setup (lib.rs:522+) и НЕ делает server.rs (64-66: только `Database::init` + `AppState::new` + `router`):
- ВКЛЮЧАЕМ в общий bootstrap (handle-free или легко освобождаемые): пути конфига (598-600); legacy config.json load-loop (602-633, без native-диалогов — ошибки в init_status/log); DB version-too-new precheck → `init_status::set_init_error` (643-671); `Database::init` retry (673-688); log-level из persisted config (692-705); `migrate_from_json` + archive `config.json`→`config.json.migrated` + `set_migration_success` (708-729); `mode::operation::recover_on_startup` (738); `init_default_skill_repos` (745-751); skills SSOT migration (755-792); first-run flag + per-app `import_default_config` (802-841); `init_default_official_providers` (843-849); codex history migrations в spawn_blocking (851-930); additive live-импорты opencode/openclaw/hermes/pi/omp (948-982); OMO+OMO-Slim импорт (985-1032); MCP-импорты (1037-1086); prompt-импорт first launch (1090-1111).
- НЕ трогаем (другие задачи): tray (1203-1262), webdav/s3 workers (1263-1270 → WEB-11), OAuth-менеджеры (1283-1310 → WEB-02), global proxy client (1325-1345 → WEB-23), `mode::controller::startup` + snippets (1353-1379 → WEB-01), codex models checks/periodic backup (1381-1392), session-usage worker (1406-1455), `usage_events::init(AppHandle)` (595 → WEB-29), окна/silent-startup (1488-1510).
- Фронт уже читает init-ошибку: `main.tsx:98-111` → `get_init_error` (desktop-команда); для web нужен HTTP-эквивалент.
- `init_status` — существующий модуль (`set_init_error`/`get_init_error`).

### Task 1: выделить bootstrap-модуль
- [ ] `bootstrap.rs`: `pub struct BootstrapReport {…}` + `pub fn run_headless_bootstrap(...) -> Result<BootstrapReport, BootstrapError>` — шаги выше по порядку, идемпотентно, ошибки не-фатальных шагов логируются и попадают в report; диалоги/окна НЕ вызывать.
- [ ] Tauri `setup` переключить на общий код там, где это безопасно (диалоговые обёртки остаются в lib.rs; минимум — миграция/seed-шаги); поведение desktop не меняется (проверить существующими тестами nextest).
- [ ] Если какой-то шаг намертво связан с AppHandle и не выделяется — пропустить в headless и записать в Notes (не ломать desktop).

### Task 2: server.rs + init-status route
- [ ] `examples/server.rs`: вместо голого `Database::init` — bootstrap; при init-ошибке не падать молча: лог + `init_status::set_init_error`, сервер поднимается в режиме «ошибка инициализации».
- [ ] `GET /api/init-status` в web_api → `{ok:true,data:{error:string|null}}` из `init_status`; adapter: `get_init_error` → этот роут (чтобы `main.tsx:98-111` DatabaseUpgrade-поток заработал в web).
- [ ] Тест роута.

### Task 3: тесты first-run/upgrade
- [ ] `src-tauri/tests/bootstrap.rs`: first-run (пустой HOME → DB создана, skill repos/official providers seeded, first-run flag); upgrade (legacy `config.json` → провайдеры в DB, `config.json.migrated` создан, повторный старт не дублирует); version-too-new (DB с большим schema version → init_status error, HTTP init-status его отдаёт); recover_on_startup вызывается (live-state crash-флаг).
- [ ] Тесты изолированы: `ensure_test_home`/`reset_test_fs`/`test_mutex` из support.rs.

### Task 4: гейты, PR, Plane
- [ ] полный набор гейтов + `cargo build --release --example server` компилируется (CI его не собирает — проверить локально).
- [ ] ветка `web-parity/web-32-bootstrap`, PR; DX-90 → In Review + ссылка. Закрыть чекбоксы, Notes (что пропущено и почему — нужно для WEB-01/11/23/26/28).

## Notes for next phase

- (заполнить по итогам)
