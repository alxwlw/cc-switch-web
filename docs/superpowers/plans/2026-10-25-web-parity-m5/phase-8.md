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
- [ ] `tokio::sync::broadcast` канал (имя события + JSON payload) в AppState; API `state.events.emit(name, payload)`; в Tauri setup — мост bus→`app_handle.emit` (сохранить desktop-поведение 1:1, включая имена событий и форму payload); в headless — только bus.
- [ ] Перевести на bus серверные emit-точки в объёме спеки: provider switched, usage/proxy updates, sync status, config/deeplink (configLoadError) — те, что сегодня эмитятся из кода, доступного без AppHandle, плюс точки, которые станет видно при реализации; чисто desktop-события (tray/окна) не переносить.
- [ ] Юнит-тесты hub (emit→receive, lagged receiver не роняет сервер).

### Task 2: SSE endpoint
- [ ] `GET /api/events` — `axum::response::sse` c keep-alive comment-фреймом; подписка на broadcast; фильтр/пересылка всех событий; корректное закрытие при disconnect.
- [ ] Интеграционный тест (oneshot/tower или spawned server): два события приходят в stream в порядке emit.

### Task 3: frontend shim + reconnect
- [ ] `web-shims/event.ts`: общий `EventSource("/api/events")` (singleton, lazy), `listen`/`once` регистрируют handler по имени события и возвращают РЕАЛЬНЫЙ unlisten; reconnect с backoff при `onerror`; при reconnect — не дублировать handler-ы. Поведение в desktop-сборке не меняется (shim используется только web-алиасом).
- [ ] `tests/lib/webShimsEvent.test.ts` (mock EventSource): listen→handler fire по сообщению; unlisten отписывает; once срабатывает один раз; reconnect переподписывает.

### Task 4: query invalidation, не ломая явные refetch
- [ ] Маппинг событие→query keys (provider switched → providers/usage; sync status → webdav/s3; config/deeplink → config queries) в одном месте (например `src/lib/query/eventInvalidation.ts`), подписка на уровне провайдера/корня; существующие явные refetch после mutations оставить как есть (инвалидация идемпотентна).
- [ ] Тест: после события соответствующий query инвалидируется (QueryClient spies); двойной refetch не ломает данные.

### Task 5: гейты, PR, Plane
- [ ] полный набор гейтов + `cargo build --release --example server`.
- [ ] ветка `web-parity/web-29-realtime-sse`, PR; DX-87 → In Review + ссылка. Закрыть чекбоксы, Notes (список событий и query-маппинг — понадобится WEB-01/04/06/11/13).

## Notes for next phase

- (заполнить по итогам)
