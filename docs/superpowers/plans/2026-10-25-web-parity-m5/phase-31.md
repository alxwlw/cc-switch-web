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
- [x] `fn publish_usage(state: &AppState, kind: UsageKind, …)`: put в `usage_cache` + emit `usage-cache-updated` через bus с тем же payload; desktop-обёртки после неё вызывают `schedule_tray_refresh`. Применить в provider.rs (script), subscription.rs (subscription), codex_oauth.rs (codex_oauth — put без emit, как сейчас).
- [x] `query_provider_usage_inner`, agy-ветка subscription, xAI/Codex/agy quota, stream check — берут менеджеры из `AppState` вместо `State<…>`/`try_state`.

### Task 2: backend — роуты + тесты
- [x] `POST /api/:app/providers/:id/usage`, `POST /api/:app/providers/:id/usage-script/test` (тело = аргументы testUsageScript), `GET /api/:app/providers/:id/limits`, `GET /api/subscription/:tool/quota`, `GET /api/subscription/codex-oauth/:accountId/quota`, `GET /api/subscription/xai-oauth/:accountId/quota`, `GET /api/subscription/agy/:accountId/quota`, `POST /api/subscription/coding-plan` (тело — аргументы), `POST /api/balance` (`{baseUrl, apiKey}`), `POST /api/:app/providers/:id/stream-check`.
- [x] `src-tauri/tests/web_api_usage_live.rs`: usage-script провайдер со скриптом, обращающимся к loopback-заглушке → `UsageResult`, кэш содержит запись, bus выдал `usage-cache-updated {kind:"script"}`; ошибка скрипта → `{ok:false}`, кэш и события не тронуты; test-script с бесконечным циклом → ошибка дедлайна 5 с; balance/coding-plan против заглушки; limits на seeded usage; stream check против заглушки, отвечающей SSE-потоком → `StreamCheckResult` ok; subscription quota для CLI-tool с подготовленным файлом учётных данных в temp HOME.

### Task 3: frontend
- [x] case в `mapCommand` для всех команд фазы.
- [x] Подписи в web: квоты подписок CLI — «по учётным данным CLI на сервере»; stream check — «проверка выполняется с сервера».
- [x] Тесты: `tests/lib/usageLiveApi.test.ts`; SSE `usage-cache-updated` обновляет кэш `useUsageQuery` без refetch (mock EventSource фазы 8); `UsageScriptModal` test-script проходит через adapter.
- [x] Снять WEB-00-гейты (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-04-usage-subscriptions`, PR; DX-62 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Usage-script в web — пользовательский JS, исполняемый на сервере по запросу любого участника tailnet: песочница QuickJS (память, дедлайн, https-only) должна действовать на HTTP-пути так же, как на desktop. Тест: скрипт, пытающийся выйти на `http://` не-loopback → отказ `validate_base_url`.
- Agy quota на Linux-сервере: фоллбек `subscription_agy` должен давать понятную ошибку, а не пустую карточку.

## Notes for next phase

- **Статус до merge (2026-10-30)**: ветка `web-parity/web-04-usage-subscriptions` (worktree `.worktrees/web-04-usage`), PR → main (открывается в T4 STEP 4; URL — в T4-отчёте и комментарии DX-62); DX-62 → In Review + ссылка. Чекбоксы Т1–Т4 закрыты, Notes заполнены (`docs(plans)` — WEB-04 T4).
  - Финальные гейты на `e24f02a7` (код; docs-коммит T4 сверху):
    - `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — чисто (RC=0);
    - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — чисто (`Finished dev profile [unoptimized + debuginfo] target(s) in 15.52s`, RC=0);
    - `(umask 022; cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml)` — lib `test result: FAILED. 4091 passed; 1 failed; 10 ignored` (baseline `commands::misc::tests::omp_search_paths_include_the_bun_global_bin`, host-PATH-зависим, тот же 1 baseline из T4-брифa); все остальные интеграционные бинарники ok (в т.ч. `web_api_usage_live` 11 passed, `web_api_usage_admin` 12 passed, `web_api_workspace` 9 passed);
    - `cargo build --release --example server` — ок (`Finished release profile [optimized] target(s) in 2m 44s`; артефакт `src-tauri/target/release/examples/server`, 29597656 байт);
    - `pnpm typecheck` — чисто (RC=0); `pnpm format:check` — `All matched files use Prettier code style!` (RC=0);
    - `prettier --check tests/**` — 14 файлов (12 warn + 2 no-parser error), все untouched фазой и на main (`git diff --name-only fdb7391e..HEAD` и `main..HEAD` vs список — пересечений ноль; файлы фазы — `UsageScriptModal.test.tsx`, `UsageScriptModalWeb.test.tsx`, `WebProviderGating.test.tsx`, `useUsageCacheBridge.test.tsx`, `webSilencedQueries.test.tsx`, `usageLiveApi.test.ts`);
    - `pnpm test:unit` — `Test Files 2 failed | 319 passed (321)`, `Tests 3 failed | 3968 passed (3971)` (baseline: 2× UsageDashboard — `draws the request log…`, `wires the tabs…` — + rememberedChoice `keeps working when storage is unavailable`; промежуточный прогон показывал флак 2 тестов `App.test.tsx` под нагрузкой сюиты — изолированно 50/50 зелёных, финальный полный прогон снова ровно baseline-3);
    - `pnpm build:renderer` — ок (`✓ built in 6.76s`); `pnpm build:web` — ок (`✓ built in 6.71s`).
  - Ревью:
    - Т1 approved r0 (publish tail + AppState managers; 2 doc-only findings).
    - Т2 changes_requested r0 (F1 agy-hermeticity SHOULD-FIX; F2 balance-forwarding note) → approved r1 (F1 закрыт собственным fake-agy тестом, F2 pinned; fix r1 — только тесты, 11/11).
    - Т3 changes_requested r0 (F1 null→default SHOULD-FIX [DP-261]) → approved r1 (F1 закрыт `web_oauth_account_param`, F2 closed-keep; fix r1 — только backend, +44/-2).
- **Backend**:
  - Общий хвост — `services/usage_cache.rs:121` (`UsageKind::{Script, Subscription, CodexOAuth}`), `:151` (`pub(crate) fn publish_usage` — put в кэш + emit `usage-cache-updated` тем же payload, CodexOAuth — put-only без emit, tray только в обёртках); `query_provider_usage_inner` — `pub(crate)`, сигнатура без изменений (врапперы строят Copilot/Xai-состояния из полей AppState); менеджеры из AppState — agy-ветка subscription (`state.agy_account`), Codex (`state.codex_oauth_manager`), xAI (`state.xai_oauth`), agy quota (`state.agy_account`), stream-check resolve (`state.copilot_auth`); unit-тесты touched-модулей 50/50, мутационные пробы 10/10.
  - Роуты — `web_api.rs:451-478`, хендлеры `:3457-3700` (`POST /api/:app/providers/:id/usage` → inner + `publish_usage(Script)` на любом `Ok`, без tray; `POST …/usage-script/test` → `ProviderService::test_usage_script` напрямую, без кэша/emit; `GET …/limits` → sync `state.db.check_provider_limits`; `GET /api/subscription/:tool/quota` → inner + AppType-гейт + publish; codex — put-only publish при resolved id + `Ok`; xai/agy — без кэша/emit; agy без CLI → 500 понятной ошибкой `real_agy_bin().is_none()` на обоих роутах; `POST /api/subscription/coding-plan`, `POST /api/balance`, `POST …/stream-check`); ошибки сервиса — `fail()` дословно, тела — `Json<Value>` (400), apiKey только в `Authorization`, canary-assertions в тестах. Fix r1: `web_oauth_account_param` (`:3606`; `:accountId="default"`/пустой → `None`, как desktop) в codex/xai-хендлерах (`:3620`, `:3646`) + unit-тест (`:4679-4696`).
  - Тесты — `src-tauri/tests/web_api_usage_live.rs` (11 тестов, `#[serial]`: loopback-скрипт → `UsageResult` + кэш + bus `{kind:"script"}`; refused-connection → 500 `{ok:false}`, кэш/события нетронуты; бесконечный цикл → дедлайн ≥5 с; не-loopback `http:` → https-отказ без I/O; balance/coding-plan против заглушки; limits на seeded $2.50; SSE stream-check → `operational`; CLI quota с credential-файлом; agy без CLI → 500; malformed → 400); мутационные пробы 11/11 RED.
- **Frontend**: 10 case — `adapter.ts:649-718` (`queryProviderUsage`, `testUsageScript`, `check_provider_limits`, `get_subscription_quota`, `get_codex_oauth_quota`, `get_xai_oauth_quota`, `agy_account_quota`, `get_coding_plan_quota`, `get_balance`, `stream_check_provider`; имена/поля без изменений, null accountId → `"default"`); подписи через `t()`+`defaultValue` (CLI — «по учётным данным CLI на сервере», OAuth — «данные запрашиваются на сервере», stream-check — «проверка выполняется с сервера»); тесты — `usageLiveApi.test.ts` (13), `useUsageCacheBridge.test.tsx` (2, SSE без refetch, без `EVENT_INVALIDATIONS`), `UsageScriptModalWeb.test.tsx` (1, через adapter), `webSilencedQueries.test.tsx` (43), `WebProviderGating.test.tsx` (25), `UsageScriptModal.test.tsx` (6) — focused 90 passed. Гейты сняты: `queries.ts:265`, `subscription.ts:96-99,145,202,230-234` (agy — да; kimi нетронут), `usage.ts:426`, `UsageScriptModal.tsx:890-896`, `ProviderCardActions.tsx:192-194`; строки 109/112/113/123 таблицы фазы 6 закрыты (`phase-6.md`; copilot_get_usage* вне скоупа).
- **Отклонения от плана**: нет beyond Rulings T1–T4 (journal p31): test-body `providerId`/`app` echoes accepted-and-ignored (path authoritative); `ProviderMeta.usage_script` snake_case без rename; non-loopback тест — TEST-NET-1 без I/O; null→`"default"` (T3) с backend-нормализацией в fix r1; OAuth-caption wording изобретён (не в спеке).
- **Известные ограничения**:
  - Balance/coding-plan vendor-URL захардкожены (кроме ZenMux) — hermetic success-тесты невозможны, покрыты детерминированные error-пути + canary.
  - `http_client::get()` идёт через системный proxy env (кроме loopback stream-check) — при proxy без `NO_PROXY` возможен флак; в этом env proxy-переменных нет.
  - Живая проверка — только чтение уже закэшированного usage (новые usage-скрипты на живом сервисе не запускать — JS исполняется на сервере).
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Миграций не ожидается, но VERIFY перед рестартом.
  - Smoke без takeover: usage query, usage-script test, quotas (CLI/Codex/XAI/agy), balance, stream-check в web; `usage-cache-updated` доходит до фронта.
  - DX-62 → Done после merge.
