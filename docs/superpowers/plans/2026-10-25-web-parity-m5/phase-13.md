# Phase 13 — WEB-12 / DX-70: администрирование usage и pricing

Цель: редактирование, пакетное обновление и удаление цен моделей, источник модели для pricing, настройки и результат автосинхронизации models.dev, пересборка Codex usage, data sources и время последней синхронизации работают в web.

Спека: § WEB-12. Оценка M (2–3 дн.).

⚠️ Уточнения к спеке: (1) `check_provider_limits` из того же диапазона usage.ts перенесён в фазу 31 (WEB-04 называет его явно). (2) «После rebuild/sync явно инвалидировать queries» уже сделано на фронте: `UsageDashboard.tsx:391`, `useUpdateModelPricing`/`useDeleteModelPricing`, `ModelsDevAutoSyncPanel` инвалидируют `usageKeys.all` сами. В web не работал только путь через событие `usage-log-recorded` — его оживляет фаза 8.

Scope: `src-tauri/src/web_api.rs` (11 роутов), `src-tauri/tests/web_api_usage_admin.rs`, `src/lib/api/adapter.ts` (11 case), тесты фронта.

Факты разведки:
- Фронт usage.ts:167-237 и proxy.ts:159-169: `update_model_pricing {modelId, displayName, inputCost, outputCost, cacheReadCost, cacheCreationCost}`, `update_model_pricing_batch {entries}` → number, `delete_model_pricing {modelId}`, `get_models_dev_sync_config`, `save_models_dev_sync_config {config}`, `record_models_dev_sync_result {syncedAt, error}`, `get_session_usage_last_sync`, `rebuild_codex_usage`, `get_usage_data_sources`, `get_pricing_model_source {appType}` / `set_pricing_model_source {appType, value}` (`"request" | "response"`). Уже в web: `get_model_pricing` (adapter.ts:307), `sync_session_usage` (adapter.ts:309 → `POST /api/usage/sync`, web_api.rs:875).
- Вызовы: `UsageDashboard.tsx:390` (rebuild), `PricingConfigPanel.tsx:84,:142`, `ModelsDevAutoSyncPanel.tsx:196,:470,:486,:510`, `useUsageAutoRefresh.ts:37,:63,:91`. models.dev скачивается браузером (`lib/modelsDevAutoSync.ts:30-76`, `lib/modelsDev.ts:57`), в backend уходят только `updateModelPricingBatch` и `recordModelsDevSyncResult`.
- Backend commands/usage.rs (все `State<AppState>`, без AppHandle): `update_model_pricing` :237 → `services::model_pricing::update_model_pricing(&Database, ModelPricingInfo)` (:431); batch :259 → :435; models.dev config get :267 → :317, save :275 → :329; record result :283 → :345; delete :~301 → :442; `get_session_usage_last_sync()` :~327 (без State) → `session_usage::last_sync_completed_at()`; `rebuild_codex_usage` (async) :~345-365 — берёт `session_sync_mutex()`, в `spawn_blocking`: `db.backup_database_file()`, `db.reset_codex_usage()`, `sync_codex_usage(&db)`, затем `usage_events::notify_log_recorded()`; `get_usage_data_sources` :~370 → `get_data_source_breakdown(&Database)`.
- `get/set_pricing_model_source` (commands/proxy.rs:341-390) делегируют в handle-free `*_internal(&AppState, …)`; `test_hook`-варианты используются в `src-tauri/tests/proxy_commands.rs`.
- Команды возвращают `AppError` (собственный `Serialize`, error.rs:117), а не String. DTO `ModelPricingInfo`, `ModelsDevSyncConfig`, `ModelsDevSyncState`, `SessionSyncResult`, `DataSourceSummary` — camelCase.
- `usage_events::notify_log_recorded()` в сервере — тихий no-op (OnceLock<AppHandle> не задан); после фазы 8 событие идёт через bus.

### Task 1: backend — роуты + тесты
- [x] `PUT /api/usage/model-pricing/:modelId` (поля pricing), `POST /api/usage/model-pricing/batch` (`{entries}`), `DELETE /api/usage/model-pricing/:modelId`, `GET/PUT /api/usage/models-dev/config`, `POST /api/usage/models-dev/result` (`{syncedAt, error}`), `GET /api/usage/last-sync`, `POST /api/usage/rebuild-codex` (тот же порядок: mutex → backup → reset → sync → notify), `GET /api/usage/data-sources`, `GET/PUT /api/:app/pricing-model-source`.
- [x] `AppError` → `fail(e.to_string())`, текст как на desktop.
- [x] Фоновый session-usage worker: фаза 7 оставила desktop-блок lib.rs:1406-1455 вне bootstrap, поэтому в web `last-sync` двигается только ручным/фронтовым sync. Прочитать блок; если worker handle-free или освобождается выносом emit на bus — запускать из headless bootstrap (тест: после старта worker выполняет sync и `get_session_usage_last_sync` меняется); если нет — Notes с причиной. (Закрыто: worker вынесен в `services::session_usage::run_background_worker`, на desktop его зовёт `lib.rs`, в web — `bootstrap::spawn_session_usage_worker` из `examples/server.rs`; см. Notes.)
- [x] `src-tauri/tests/web_api_usage_admin.rs`: update → `GET /api/usage/model-pricing` отражает; batch из 3 записей → 3; delete; models.dev config round-trip; record result → config показывает `lastSyncedAt`/ошибку; pricing source round-trip, невалидное значение → 400; rebuild на seeded Codex-сессиях в temp HOME создаёт backup-файл и пересобирает usage; два одновременных rebuild сериализуются mutex-ом.

### Task 2: frontend
- [x] 11 case в `mapCommand`.
- [x] `tests/lib/usageAdminApi.test.ts`: URL/метод/body каждой функции.
- [x] Проверить в web: models.dev отвечает браузеру с CORS-заголовками (desktop-webview тоже ходит браузерным fetch, но с другим origin). Если нет — Notes и отдельный server-side fetch-роут в этой же фазе. (Закрыто: `models.dev/api.json` отвечает `access-control-allow-origin: *`, `fetchModelsDev` — простой GET без кастомных заголовков, preflight нет.)
- [x] Снять WEB-00-гейты с `PricingConfigPanel`/`ModelsDevAutoSyncPanel`/rebuild (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-12-usage-admin`, PR; DX-70 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Rebuild во время идущего `sync_session_usage`: оба берут `session_sync_mutex` — тест на чередование.
- Батч с невалидной записью посередине: поведение (всё или ничего / частично) как у сервиса на desktop, и UI не показывает «обновлено N», если сервер вернул ошибку.

## Notes for next phase

- **Статус до merge (2026-10-26)**: ветка `web-parity/web-12-usage-admin` (worktree `.worktrees/web-12`), PR #14 (https://github.com/alxwlw/cc-switch-web/pull/14) открыт; DX-70 In Review (комментарий со ссылкой).
  - Т1 одобрена с r3. r0: SHOULD-FIX DP-018, шаг notify после rebuild не закреплён тестом, закрыто в `16b15223`. r1: SHOULD-FIX DP-017, `IsolatedHome` менял `CC_SWITCH_TEST_HOME` без блокировки, закрыто модульным mutex в `8c8ee539`.
  - После одобрения r2 финальный полный `cargo test` уронил `services::provider::tests::update_claude_desktop_route_provider_rewrites_profile_in_route_mode`: модульный mutex не исключает `#[serial]`-тесты других модулей. Т1 переоткрыта через `--force`, тесты переведены на `#[serial]` в `09c8c2b9`, r3 одобрено.
  - Т2 одобрена с r1. r0: SHOULD-FIX DP-026, пункт «открыть папку» в `ModelsDevAutoSyncPanel` был живым в web и всегда падал, закрыто в `af44f2ca` вместе с NIT DP-027.
  - Финальные гейты: на `74a165f` fmt и clippy `-D warnings` чисто. `cargo test --no-fail-fast` (nextest не установлен): 4344 passed / 6 failed.
    - Baseline: omp_search_paths, 2× file_modes::switch_*.
    - Ещё 3× `proxy::forwarder::tests::bookkeeping::*` — flake: отдельный прогон даёт 2 зелёных из 3, proxy ветка не трогает.
    - `cargo build --release --example server` ок.
    - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3717 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice) на `af44f2ca` — фронт после этого не менялся.
  - В PR едет неподтолкнутый `ac9d7fdf` (docs закрытия фазы 12 с локального main).
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся); перед install — `cp cc-switch-web cc-switch-web.prev`. Миграций БД нет.
  - **На живом сервисе впервые запустится session-usage worker.** При старте он делает раунд с `backfill_missing_usage_costs` и, если включён `session_auto_sync_enabled`, импортирует сессии `~/.claude`/`~/.codex` серверного хоста. Дальше раунд каждые 10 минут. После restart проверить journal на WARN «Session usage sync completed with N error(s)» и что `GET /api/usage/last-sync` не null.
  - DX-70 → Done.
- **Роуты** (`web_api.rs`):
  - `PUT|DELETE /api/usage/model-pricing/:model_id`. PUT принимает body `{displayName, inputCost, outputCost, cacheReadCost, cacheCreationCost}`. Model id со `/` приходит как `%2F` (тест есть), адаптер делает `encodeURIComponent`.
  - `POST /api/usage/model-pricing/batch`, body `{entries}` (`ModelPricingInfo`, camelCase), отвечает числом.
  - `GET|PUT /api/usage/models-dev/config`: **PUT принимает голый объект config**, а не `{config}`; адаптер разворачивает.
  - `POST /api/usage/models-dev/result`, body `{syncedAt, error}`.
  - `GET /api/usage/last-sync`, `POST /api/usage/rebuild-codex`, `GET /api/usage/data-sources`.
  - `GET|PUT /api/:app/pricing-model-source`, body `{value}`. Неизвестный app или значение вне `request|response` → 400. Desktop принимал любую строку app.
  - Ошибки сервисов — дословно, `fail(e.to_string())`.
  - Мелочь, не правили: `PUT /api/usage/model-pricing/batch` матчится на `:model_id`, поэтому модель с id ровно «batch» нельзя отредактировать или удалить.
- **Batch всё-или-ничего**, как на desktop: сервис нормализует все записи до первой записи в БД, невалидная запись посередине отклоняет весь батч (тест есть). На фронте ошибка батча → `syncModelsDevPricing` бросает → тост syncFailed, «обновлено N» не показывается (два теста).
- **Rebuild Codex**:
  - Последовательность вынесена в handle-free `services::session_usage::rebuild_codex_usage_unlocked`: backup → reset → import → `finish_codex_rebuild`, то есть notify. Вызывающий держит `session_sync_mutex`.
  - Desktop-команда и web-роут вызывают одно и то же. Notify идёт в `usage_events::notify_log_recorded` → event bus фазы 8.
  - Интеграционного теста на emit в bus нет (hub — глобальный OnceLock); notify закреплён unit-тестами в `session_usage.rs`.
- **Session-usage worker**:
  - Desktop-блок `lib.rs` вынесен в `services::session_usage::run_background_worker`; desktop вызывает его как раньше.
  - Headless-сервер запускает его через `bootstrap::spawn_session_usage_worker` из `examples/server.rs`, а не внутри `run_headless_bootstrap_with`: тому нужен tokio runtime, а тесты bootstrap синхронные.
  - Тест worker опрашивает `last-sync` до 20 с вместо fake time.
- **Тестовые хуки**: `web_api::session_sync_lock_test_hook` (`doc(hidden)`, как прочие `*_test_hook`) и `support::web_router_with` в `src-tauri/tests/support.rs`. Env-мутирующие тесты — только `#[serial]` (см. выше).
- **Frontend**:
  - 11 case в `mapCommand`.
  - Сняты WEB-00-гейты: `PricingConfigPanel`, rebuild Codex в `UsageSettingsSheet`, ранний skip стартового sync models.dev в `modelsDevAutoSync.ts`, `useSessionUsageLastSync`.
  - Пункт «открыть папку» в `ModelsDevAutoSyncPanel` скрыт под `!isWeb`; `open_app_config_folder` остаётся за WEB-01/26. Server path показывается как есть.
  - `check_provider_limits` остаётся под гейтом, комментарий перенацелен на фазу 31 (WEB-04).
  - CORS: `GET https://models.dev/api.json` отвечает `access-control-allow-origin: *`, `fetchModelsDev` — простой GET без своих заголовков. Серверный fetch-роут не нужен.
- **Инвентаризация фазы 6**: строка usage/pricing перенесена в «закрыто»; `get_usage_data_sources` убран из строки «мёртвое».
- **Не чинили, вне scope**: `cargo clippy --all-targets -- -D warnings` падает на dead code `SystemKeychain::named` (`agy_account/keychain.rs:74`). CI запускает clippy без `--all-targets`.
