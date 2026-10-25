# Phase 31 — WEB-04 / DX-62: provider usage, подписки, баланс и connectivity

Цель: в web работают запрос usage провайдера и тест usage-script, квоты официальных подписок (CLI, Codex OAuth, xAI, Antigravity), квота coding plan, баланс, stream check провайдера и проверка лимитов провайдера. Результаты попадают в серверный usage-кэш и во фронт тем же событием `usage-cache-updated`, что на desktop.

Спека: § WEB-04. Оценка M (4–6 дн.). Managed OAuth-состояния — в `AppState` (фазы 25–26). Endpoint speed test — фаза 11, здесь не дублируется. `check_provider_limits` перенесён сюда из WEB-12 (фаза 13).

⚠️ Уточнение к спеке: «отделить вычисление результата от AppHandle» частично уже сделано — `query_provider_usage_inner` (provider.rs:605) handle-free; в обёртке остаётся только хвост cache/emit/tray.

Scope: `src-tauri/src/commands/{provider,subscription,codex_oauth,xai_oauth,agy_account,stream_check}.rs` (хвост cache+emit в общую функцию, менеджеры из AppState), `src-tauri/src/web_api.rs` (≈11 роутов), `src-tauri/tests/web_api_usage_live.rs`, `src/lib/api/adapter.ts`, тесты фронта.

Факты разведки:
- Фронт: usage.ts:25 `queryProviderUsage {providerId, app}`, :39 `testUsageScript {providerId, app, scriptCode, timeout, apiKey, baseUrl, accessToken, userId, templateType}`, :218 `check_provider_limits {providerId, appType}`; subscription.ts:6 `get_subscription_quota {tool}`, :8 `get_codex_oauth_quota {accountId}`, :10 `get_xai_oauth_quota {accountId}`, :12 `agy_account_quota {accountId}`, :24 `get_coding_plan_quota {baseUrl, apiKey, accessKeyId?, secretAccessKey?, codingPlanProvider?, teamOrganizationId?, teamProjectId?}`, :37 `get_balance {baseUrl, apiKey}`; connectivity-check.ts:28 `stream_check_provider {appType, providerId}`.
- Вызовы: `lib/query/queries.ts:246-262` (`useUsageQuery` → `UsageFooter.tsx:127`, `ProviderCard.tsx:290`), `lib/query/subscription.ts:93,140,196`, `UsageScriptModal.tsx:573-712`, `useStreamCheck.ts:29`, `lib/query/usage.ts:423`. События: `usage-cache-updated` (`useUsageCacheBridge.ts:30` пишет payload прямо в кэш React Query), `usage-log-recorded` (`useUsageEventBridge.ts:23`).
- Backend: provider.rs:517 `queryProviderUsage(app_handle, state, copilot_state, xai_state, providerId, app)` → `query_provider_usage_inner(&AppState, &CopilotAuthState, &XaiOAuthState, AppType, &str)` (:605); хвост при `Ok` (:538-556): emit `usage-cache-updated {kind:"script", appType, providerId, data}`, `usage_cache.put_script`, `tray::schedule_tray_refresh`. provider.rs:~838 `testUsageScript(state, …)` — handle-free (`ProviderService::test_usage_script`, usage.rs:193). subscription.rs:20 `get_subscription_quota(app, state, tool)`: для `agy` — `app.try_state::<AgyAccountState>()`, иначе `services::subscription::get_subscription_quota(&tool)` (читает файлы учётных данных CLI на сервере); хвост при `Ok` — emit `{kind:"subscription"}`, `put_subscription`, tray (:28-42). codex_oauth.rs:28 `get_codex_oauth_quota` — `put_codex_oauth` (:50) + tray, без emit. xai_oauth.rs:70 → `query_xai_oauth_quota_for` (без кэша и emit). agy_account.rs:22 `agy_account_quota` → `agy_quota` (keychain/`agy /usage`; на Linux — фоллбек `subscription_agy`). coding_plan.rs:4, balance.rs:4 — чистый HTTP. usage.rs:295 `check_provider_limits` — sync, DB (`usage_stats.rs:2259`). stream_check.rs:~16 `stream_check_provider(state, copilot_state, app_type, provider_id) -> Result<StreamCheckResult, AppError>` — Copilot-state нужен только для `resolve_copilot_base_url_override`.
- Usage-кэш — `AppState.usage_cache: Arc<UsageCache>` (in-process, три карты), читается tray-ем. Usage-script исполняется во встроенном QuickJS (usage_script.rs: 16 МиБ памяти, стек 256 КиБ, дедлайн 5 с; `validate_base_url` требует https, loopback — исключение).
- DTO: `SubscriptionQuota`, `StreamCheckResult`, `ProviderLimitStatus` — camelCase; `UsageResult`/`UsageData` — поштучные rename.
- Тестов этих команд в `src-tauri/tests` нет.

### Task 1: общий хвост публикации и менеджеры из AppState
- [ ] `fn publish_usage(state: &AppState, kind: UsageKind, …)`: put в `usage_cache` + emit `usage-cache-updated` через bus с тем же payload; desktop-обёртки после неё вызывают `schedule_tray_refresh`. Применить в provider.rs (script), subscription.rs (subscription), codex_oauth.rs (codex_oauth — put без emit, как сейчас).
- [ ] `query_provider_usage_inner`, agy-ветка subscription, xAI/Codex/agy quota, stream check — берут менеджеры из `AppState` вместо `State<…>`/`try_state`.

### Task 2: backend — роуты + тесты
- [ ] `POST /api/:app/providers/:id/usage`, `POST /api/:app/providers/:id/usage-script/test` (тело = аргументы testUsageScript), `GET /api/:app/providers/:id/limits`, `GET /api/subscription/:tool/quota`, `GET /api/subscription/codex-oauth/:accountId/quota`, `GET /api/subscription/xai-oauth/:accountId/quota`, `GET /api/subscription/agy/:accountId/quota`, `POST /api/subscription/coding-plan` (тело — аргументы), `POST /api/balance` (`{baseUrl, apiKey}`), `POST /api/:app/providers/:id/stream-check`.
- [ ] `src-tauri/tests/web_api_usage_live.rs`: usage-script провайдер со скриптом, обращающимся к loopback-заглушке → `UsageResult`, кэш содержит запись, bus выдал `usage-cache-updated {kind:"script"}`; ошибка скрипта → `{ok:false}`, кэш и события не тронуты; test-script с бесконечным циклом → ошибка дедлайна 5 с; balance/coding-plan против заглушки; limits на seeded usage; stream check против заглушки, отвечающей SSE-потоком → `StreamCheckResult` ok; subscription quota для CLI-tool с подготовленным файлом учётных данных в temp HOME.

### Task 3: frontend
- [ ] case в `mapCommand` для всех команд фазы.
- [ ] Подписи в web: квоты подписок CLI — «по учётным данным CLI на сервере»; stream check — «проверка выполняется с сервера».
- [ ] Тесты: `tests/lib/usageLiveApi.test.ts`; SSE `usage-cache-updated` обновляет кэш `useUsageQuery` без refetch (mock EventSource фазы 8); `UsageScriptModal` test-script проходит через adapter.
- [ ] Снять WEB-00-гейты (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-04-usage-subscriptions`, PR; DX-62 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Usage-script в web — пользовательский JS, исполняемый на сервере по запросу любого участника tailnet: песочница QuickJS (память, дедлайн, https-only) должна действовать на HTTP-пути так же, как на desktop. Тест: скрипт, пытающийся выйти на `http://` не-loopback → отказ `validate_base_url`.
- Agy quota на Linux-сервере: фоллбек `subscription_agy` должен давать понятную ошибку, а не пустую карточку.

## Notes for next phase

- (заполнить по итогам)
