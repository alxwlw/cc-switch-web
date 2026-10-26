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
- [ ] `PUT /api/usage/model-pricing/:modelId` (поля pricing), `POST /api/usage/model-pricing/batch` (`{entries}`), `DELETE /api/usage/model-pricing/:modelId`, `GET/PUT /api/usage/models-dev/config`, `POST /api/usage/models-dev/result` (`{syncedAt, error}`), `GET /api/usage/last-sync`, `POST /api/usage/rebuild-codex` (тот же порядок: mutex → backup → reset → sync → notify), `GET /api/usage/data-sources`, `GET/PUT /api/:app/pricing-model-source`.
- [ ] `AppError` → `fail(e.to_string())`, текст как на desktop.
- [ ] Фоновый session-usage worker: фаза 7 оставила desktop-блок lib.rs:1406-1455 вне bootstrap, поэтому в web `last-sync` двигается только ручным/фронтовым sync. Прочитать блок; если worker handle-free или освобождается выносом emit на bus — запускать из headless bootstrap (тест: после старта worker выполняет sync и `get_session_usage_last_sync` меняется); если нет — Notes с причиной.
- [ ] `src-tauri/tests/web_api_usage_admin.rs`: update → `GET /api/usage/model-pricing` отражает; batch из 3 записей → 3; delete; models.dev config round-trip; record result → config показывает `lastSyncedAt`/ошибку; pricing source round-trip, невалидное значение → 400; rebuild на seeded Codex-сессиях в temp HOME создаёт backup-файл и пересобирает usage; два одновременных rebuild сериализуются mutex-ом.

### Task 2: frontend
- [x] 11 case в `mapCommand`.
- [x] `tests/lib/usageAdminApi.test.ts`: URL/метод/body каждой функции.
- [x] Проверить в web: models.dev отвечает браузеру с CORS-заголовками (desktop-webview тоже ходит браузерным fetch, но с другим origin). Если нет — Notes и отдельный server-side fetch-роут в этой же фазе. (Закрыто: `models.dev/api.json` отвечает `access-control-allow-origin: *`, `fetchModelsDev` — простой GET без кастомных заголовков, preflight нет.)
- [x] Снять WEB-00-гейты с `PricingConfigPanel`/`ModelsDevAutoSyncPanel`/rebuild (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-12-usage-admin`, PR; DX-70 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Rebuild во время идущего `sync_session_usage`: оба берут `session_sync_mutex` — тест на чередование.
- Батч с невалидной записью посередине: поведение (всё или ничего / частично) как у сервиса на desktop, и UI не показывает «обновлено N», если сервер вернул ошибку.

## Notes for next phase

- (заполнить по итогам)
