# Phase 8 — WEB-29 / DX-87: realtime events через SSE/WebSocket

Цель: web-фронт получает серверные события (provider switched, usage/proxy updates, sync status, config/deeplink events) через SSE с reconnect и query invalidation; уже работающие явные refetch после mutations НЕ ломать. Потребители (следующие модули): WEB-01, 04, 06, 11, 13.

Спека: § WEB-29. Оценка M (3–5 дн.).

Scope: общий event-hub в `AppState` (broadcast), emit-точки серверных мутаций, `GET /api/events` (SSE) в `web_api.rs`, реальная реализация `src/lib/web-shims/event.ts` (listen/once поверх EventSource + reconnect), инвалидация React Query; тесты backend+frontend.

Факты разведки:
- `web-shims/event.ts` (20 строк): `listen`/`once` мгновенно возвращают no-op unlisten — обработчики не fire-ят никогда. Алиасится через vite.config.web.mts:23-32, т.е. весь фронт в web уже «подписан» через этот shim (`main.tsx:83-90` configLoadError, `useUsageEventBridge.ts:15-31`, `providers.ts:194-200` и др.) — достаточно оживить shim, вызовы менять не нужно.
- Desktop-события эмитятся через AppHandle (`usage_events::init(app.handle())`, lib.rs:595; proxy_service.set_app_handle, lib.rs:734) — в headless AppHandle нет; нужна абстракция: event bus в AppState, desktop-мост bus→app.emit, web-мост bus→broadcast→SSE.
- axum 0.7 + tower/tower-http уже в Cargo.toml:57-59 (SSE — `axum::response::sse`, доп. зависимостей не нужно).
- `plugin:*` noop-список adapter.ts:325-334 не мешает: shim заменяет сам модуль event.

### Task 1: event hub в AppState
- [x] `tokio::sync::broadcast` канал (имя события + JSON payload) в AppState; API `state.events.emit(name, payload)`; в Tauri setup — мост bus→`app_handle.emit` (сохранить desktop-поведение 1:1, включая имена событий и форму payload); в headless — только bus.
- [x] Перевести на bus серверные emit-точки в объёме спеки: provider switched, usage/proxy updates, sync status, config/deeplink (configLoadError) — те, что сегодня эмитятся из кода, доступного без AppHandle, плюс точки, которые станет видно при реализации; чисто desktop-события (tray/окна) не переносить.
- [x] Юнит-тесты hub (emit→receive, lagged receiver не роняет сервер).

### Task 2: SSE endpoint
- [x] `GET /api/events` — `axum::response::sse` c keep-alive comment-фреймом; подписка на broadcast; фильтр/пересылка всех событий; корректное закрытие при disconnect.
- [x] Интеграционный тест (oneshot/tower или spawned server): два события приходят в stream в порядке emit.

### Task 3: frontend shim + reconnect
- [x] `web-shims/event.ts`: общий `EventSource("/api/events")` (singleton, lazy), `listen`/`once` регистрируют handler по имени события и возвращают РЕАЛЬНЫЙ unlisten; reconnect с backoff при `onerror`; при reconnect — не дублировать handler-ы. Поведение в desktop-сборке не меняется (shim используется только web-алиасом).
- [x] `tests/lib/webShimsEvent.test.ts` (mock EventSource): listen→handler fire по сообщению; unlisten отписывает; once срабатывает один раз; reconnect переподписывает.

### Task 4: query invalidation, не ломая явные refetch
- [x] Маппинг событие→query keys (provider switched → providers/usage; sync status → webdav/s3; config/deeplink → config queries) в одном месте (например `src/lib/query/eventInvalidation.ts`), подписка на уровне провайдера/корня; существующие явные refetch после mutations оставить как есть (инвалидация идемпотентна).
- [x] Тест: после события соответствующий query инвалидируется (QueryClient spies); двойной refetch не ломает данные.

### Task 5: гейты, PR, Plane
- [x] полный набор гейтов + `cargo build --release --example server`.
- [x] ветка `web-parity/web-29-realtime-sse`, PR; DX-87 → In Review + ссылка. Закрыть чекбоксы, Notes (список событий и query-маппинг — понадобится WEB-01/04/06/11/13).

## Notes for next phase

- **Закрыта 2026-10-26**: PR #9 merged `8c6d681b` → main; live на `cc-switch-web.service`. Server пересобран из main (`cargo build --release --example server` → `~/.local/bin/cc-switch-web`, бинарник фазы 7 сохранён в `cc-switch-web.prev`), `pnpm build:web` в main-checkout, restart unit (миграций нет, бэкап БД не снимали). Проверено: unit active, в journal строка `事件总线已注入`, WARN/ERROR нет; `/api/health` 200, `/api/init-status` → `null`, отдаваемый `index.html` = сборка; `GET /api/events` → 200 `text/event-stream`, за 35 с пришло 2× `: keep-alive` (`curl -N … | head` ничего не показывает из-за буферизации пайпа — писать в файл через `-o`). Живое событие на сервере не проверяли: переключать провайдера на реальных данных хоста не стали. DX-87 Done. Worktree `.worktrees/web-29`, локальная и remote ветки удалены; ledgers прогона перенесены в `.claude/plans/2026-10-25-web-parity-m5-p8` main-checkout.
- **Статус до merge**: ветка `web-parity/web-29-realtime-sse` (worktree `.worktrees/web-29`), PR #9 (https://github.com/alxwlw/cc-switch-web/pull/9) открыт, DX-87 In Review (комментарий со ссылкой). Задачи 1–4 прошли implement→review, у каждой один fix round: T1 (DP-011 флаги payload не закреплены, DP-018 мост не покрыт тестами), T2 (resync при lag, тест keep-alive), T3 (битый JSON, stale backoff timer), T4 (DP-001 `cancelRefetch:false` терял инвалидацию). T5 делал контроллер. Финальные гейты на `916d750c`: fmt/clippy чисто, `cargo build --release --example server` ок, cargo test 4288 passed / 3 failed (baseline: omp_search_paths, 2× file_modes::switch_*); `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок, `test:unit` 3660 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice; `PiProviderForm` один раз упал под нагрузкой параллельного cargo test, отдельно и в повторном прогоне зелёный). В PR едет и неподтолкнутый `cfbf81f7` (docs закрытия фазы 7 с локального main). После merge — `post-merge`: пересобрать server и `dist-web` (Rust менялся), `cp cc-switch-web cc-switch-web.prev` перед install; DX-87 → Done.
- **API шины** (`src-tauri/src/events.rs`; для WEB-01/04/06/11/13):
  - `Event { name: String, payload: serde_json::Value }`. `AppState.events: EventHub` (Clone, broadcast на 256). `emit(name, payload)` не блокирует и не падает. `subscribe()` возвращает receiver: `Lagged` → пропустить и продолжить, `Closed` → конец.
  - Desktop: `bridge_to_tauri(AppHandle)` ставит синхронный sink (`app.emit` внутри `emit()`), а не задачу-подписчика. Так порядок 1:1 и lag не теряет desktop-события.
  - Код, у которого есть только `AppHandle`: `emit_via_app(app, name, payload)` идёт через managed AppState hub, а до того, как state managed (deeplink на старте), — `app.emit`.
  - `provider_switched_payload(state, app_type, id)` общий для tray и web.
  - `usage_events::init` и auto-sync воркеры webdav/s3 принимают hub. `ProxyService::new` создаёт отвязанный hub, пока не вызван `with_events`, — его вызывает только `AppState::new`.
  - Dev-deps: `tauri` с `features=["test"]` (`mock_app`), `tokio` с `test-util`; в не-тестовые сборки не протекают (resolver 2).
- **События на шине** (имя → payload):
  - `provider-switched` — `{appType, proxyEnabled, autoFailoverEnabled, providerId}` (web `switch_provider`, profile, tray) или `{appType, providerId, source: "failover"|"failoverEnabled"}`;
  - `profile-applied` — `{profileId, scope}`;
  - `proxy-flags-changed` — 4-key, как `provider-switched`;
  - `usage-log-recorded` — `null`, с debounce 200 мс;
  - `usage-cache-updated` — `{kind: "subscription"|"script", appType, providerId?, data}`;
  - `universal-provider-synced` — `{action, id}`;
  - `webdav-sync-status-updated` / `s3-sync-status-updated` — `{source: "auto", status, error?}`;
  - `deeplink-pending` — `null`;
  - `proxy-official-warning` — `{appType, providerName}`.

  На `AppHandle` остались только desktop-события: `tray-navigate`, `update-download-progress` и `emit_to` в `tray_panel`. `configLoadError` слушает только фронт (`main.tsx`); на сервере его никто не эмитит, init-ошибки идут через `init_status`.
- **Пробелы (для следующих WEB)**:
  - web-обработчики profile apply, ручной webdav/s3 sync, universal providers и обновления usage cache ничего не эмитят и держатся на явном refetch. WEB-06/11 должны добавить emit в свои web-роуты;
  - headless failover не переключает и не эмитит: `proxy/failover_switch.rs` требует `AppHandle`. Решать в WEB-01;
  - конвертированные call sites (`profile.rs`, `failover.rs`, `subscription.rs`, `provider.rs`, `tray.rs`, `failover_switch.rs`) — однострочные замены, отдельных тестов нет, тестами покрыт слой helper-ов.
- **SSE wire contract** (`GET /api/events`, `web_api.rs`):
  - на каждое событие шины — именованное SSE-событие: `event: <name>`, `data: <payload JSON>` (`null`, если payload нет), без конверта. `onmessage` не срабатывает;
  - `id:`/`retry:` нет, replay нет. Keep-alive — comment `: keep-alive` раз в 15 с (`SSE_KEEP_ALIVE`), из JS не виден;
  - lag дальше 256 событий даёт кадр `event: resync`, `data: null`, поток продолжается;
  - в init-error router маршрут уходит в `/api/*rest` и отвечает 503 JSON; EventSource остаётся CLOSED без автоповтора;
  - middleware (auth/timeout/compression) на router нет. Reverse proxy перед сервером не должен буферизовать.
- **Web shim** (`src/lib/web-shims/event.ts`):
  - общий lazy `EventSource` открывается на первый `listen`, закрывается после последнего unlisten. Handler-ы получают Tauri-форму `{event, id, payload}`, id — локальный счётчик;
  - битый JSON логируется и отбрасывается, исключение в handler-е изолировано;
  - CONNECTING → повторяет сам браузер. CLOSED (503, рестарт) → backoff 1 с с удвоением до 30 с, сброс на open, отмена, если отписались все;
  - `resync` — обычное имя события: его слушатели получают и серверный кадр, и синтетический `resync` (`null`) на каждый open после обрыва, но не на первый open;
  - ping/watchdog для half-open соединений (сон ноутбука) не делали: браузер обычно даёт onerror при пробуждении. Вернуться, если появятся «залипшие» панели.
- **Инвалидация** (`src/lib/query/eventInvalidation.ts`; расширять здесь):
  - `startEventInvalidation(qc, enabled, onError)` вызывается из `main.tsx` с `isWeb`. Только web: в desktop те же события уже доходят до существующих слушателей, а `resync` в Tauri не бывает. Корневая подписка не снимается, поэтому EventSource живёт всё время работы приложения;
  - маппинг (только непокрытые события):
    - `provider-switched {appType}` → `["autoFailoverEnabled",app]`, `["failoverQueue",app]`, `["availableProvidersForFailover",app]`, `subscriptionKeys.quota(app)`, `proxyKeys.status`, `proxyKeys.takeoverStatus` (без appType — ключи без app);
    - `proxy-flags-changed` → `proxyKeys.status`, `proxyKeys.takeoverStatus`, `["autoFailoverEnabled"]`;
    - `usage-log-recorded` → `usageKeys.all`;
    - `resync` → `invalidateQueries()`;
  - уже обрабатываются в других местах, сюда не дублировать:
    - `provider-switched` → `["providers",app]` (App.tsx);
    - `universal-provider-synced`, `profile-applied`;
    - webdav/s3 status → `["settings"]` (App.tsx);
    - `usage-cache-updated` → `setQueryData` (`useUsageCacheBridge`);
    - `deeplink-pending`;
  - **Грабли**: инвалидировать с дефолтным `cancelRefetch`. С `{cancelRefetch:false}` событие присоединяется к fetch, ушедшему до изменения на сервере, его успех снимает invalidated, и UI остаётся со старыми данными (DP-001, пойман на ревью).
- **Init-error режим**: слушатель `configLoadError` в `main.tsx` держит shim в backoff-цикле на 503. Это безвредно (не чаще раза в 30 с), но шумит в devtools.
- **Деплой (post-merge)**: миграций БД нет. Server пересобрать и задеплоить так же, как в фазе 7 (`~/.local/bin/cc-switch-web`, unit `cc-switch-web.service`). После restart проверить `curl -N http://<host>/api/events` (идут keep-alive comment-ы) и переключение провайдера из второй вкладки: первая вкладка обновляется без reload.
