# Phase 28 — WEB-01 / DX-59 (этап 2): failover, circuit breaker, health, OAuth data plane

Цель: в web работают очередь failover, автоматический failover, circuit breaker (конфигурация, сброс) и health провайдеров. Proxy без `AppHandle` обслуживает и OAuth-провайдеров (Copilot, Codex OAuth, xAI) через менеджеры из `AppState`. Переключения после failover доходят во фронт событием `provider-switched` через SSE. DX-59 закрывается этой фазой.

Спека: § WEB-01 (остаток после фазы 27). OAuth data plane опирается на фазы 25–26, realtime — на фазу 8.

Scope: `src-tauri/src/proxy/forwarder.rs` (менеджеры из AppState), `src-tauri/src/proxy/failover_switch.rs` (без обязательного AppHandle), `src-tauri/src/services/proxy.rs` (`ProxyServer::new` без handle), `src-tauri/src/commands/proxy.rs:425-562`, `src-tauri/src/commands/failover.rs`, `src-tauri/src/web_api.rs` (11 роутов), `src-tauri/tests/web_api_failover.rs`, `src/lib/api/adapter.ts`, тесты фронта.

Факты разведки:
- Фронт failover.ts:31-97 — 11 invoke: `get_provider_health`, `reset_circuit_breaker`, `get/update_circuit_breaker_config`, `get_circuit_breaker_stats`, `get_failover_queue`, `get_available_providers_for_failover`, `add_to_failover_queue`, `remove_from_failover_queue`, `get/set_auto_failover_enabled`. Хуки — `lib/query/failover.ts:21-230`.
- Backend: proxy.rs:425 `get_provider_health` (DB); :443 `reset_circuit_breaker(AppHandle, …)` — DB + сброс in-memory breaker (services/proxy.rs:645) + возможно `FailoverSwitchManager::try_switch(Some(&app_handle), …)` (emit `provider-switched` + tray); :529/:540 breaker config (DB + горячее обновление, services/proxy.rs:612); :562 `get_circuit_breaker_stats` — заглушка, всегда `Ok(None)`. failover.rs:74, :102 — DB; :121/:139 add/remove queue — `AppHandle` только для `refresh_tray_menu`; :156 get auto-failover — DB; :173 `set_auto_failover_enabled(AppHandle, …)` — emit `provider-switched {appType, providerId, source:"failoverEnabled"}` (:295-301) + tray (:306).
- `ProxyServer` использует handle: (1) `FailoverSwitchManager::try_switch` после успешного failover (forwarder.rs:657-662 → failover_switch.rs:91-116: `try_state::<AppState>`, `record_failover_route`, tray, emit `provider-switched`; всё под `if let Some(app)`); (2) OAuth data plane — `app_handle.state::<CopilotAuthState>()` (forwarder.rs:1681, 2107, 3199, 3249, 3273), `CodexOAuthState` (:1427, :2158), `XaiOAuthState` (:2227); без handle — `AuthError "…(无 AppHandle)"` (:1425, 2150, 2217, 2255). Итог сейчас: headless proxy работает с plain-key провайдерами и fail-closed для OAuth.
- После фаз 25–26 все менеджеры — поля `AppState` (те же Arc, что в managed states desktop).
- Хранение: очередь — колонка `in_failover_queue` + индекс сортировки (schema.rs:41, dao/failover.rs:23); health — таблица `provider_health` (schema.rs:194); breakers — только память (`ProviderRouter.circuit_breakers`, provider_router.rs:28), живут, пока работает сервер.
- Claude Desktop участвует в route/takeover, но не в failover (`require_failover_app` → `supports_local_proxy`).
- DTO: `CircuitBreakerConfig`/`Stats` (circuit_breaker.rs:37/392), `FailoverQueueItem` (dao/failover.rs:12) — camelCase; `ProviderHealth` (types.rs:125) — snake_case.

### Task 1: OAuth data plane без AppHandle
- [ ] `ProxyServer::new(config, db, …)` получает клон `AppState` (или нужные Arc менеджеров) вместо `Option<AppHandle>` для OAuth; forwarder берёт Copilot/Codex/xAI из него. Ветки «无 AppHandle» удаляются.
- [ ] Тест: proxy в headless с Copilot-провайдером и подготовленным `copilot_auth.json` → запрос к заглушке upstream уходит с токеном менеджера (seam базового URL upstream — как в существующих тестах forwarder; если seam нет — добавить).

### Task 2: failover switch через bus
- [ ] `FailoverSwitchManager::try_switch(state: &AppState, app: Option<&AppHandle>, …)`: `record_failover_route` и emit `provider-switched` — через `state` и bus; tray — только при `Some(app)`. Forwarder вызывает его и в headless.
- [ ] `reset_circuit_breaker`, `set_auto_failover_enabled`: логика в handle-free функциях от `&AppState` с emit через bus; desktop-обёртки добавляют tray-refresh. `add/remove_from_failover_queue` — то же без emit.

### Task 3: backend — роуты + тесты
- [ ] `GET /api/failover/health/:app/:providerId`, `POST /api/failover/breaker/:app/:providerId/reset`, `GET/PUT /api/failover/breaker-config/:app`, `GET /api/failover/breaker-stats/:app/:providerId` (заглушка `null`, как на desktop), `GET /api/failover/queue/:app`, `GET /api/failover/available/:app`, `POST /api/failover/queue/:app/:providerId`, `DELETE /api/failover/queue/:app/:providerId`, `GET/PUT /api/failover/auto/:app` (`{enabled}`). Приложение без поддержки failover (Claude Desktop) → ошибка `require_failover_app`, как на desktop.
- [ ] `src-tauri/tests/web_api_failover.rs`: очередь из двух провайдеров, upstream первого отвечает 500 → после порога breaker открывается, запросы идут на второго, bus выдаёт `provider-switched`, `record_failover_route` записан, health первого — unhealthy; reset breaker → первый снова в ротации; auto-failover off → переключения нет; set auto-failover эмитит `source:"failoverEnabled"`.

### Task 4: frontend
- [ ] 11 case в `mapCommand`.
- [ ] Тесты: `tests/lib/failoverApi.test.ts`; SSE `provider-switched` после failover инвалидирует providers/proxy queries (маппинг фазы 8).
- [ ] Снять оставшиеся WEB-00-гейты failover/breaker (таблица фазы 6).

### Task 6: TCP-connect raw-пути с happy eyeballs (IPv6 → IPv4)
Добавлена по итогам расследования из Notes фазы 27 (proxy висит на upstream за Cloudflare на agentbox). Без неё живая проверка Task 5 на реальных провайдерах невозможна.

Причина:
- `server.rs:186-203` прикладывает `OriginalHeaderCases` к каждому соединению, поэтому `send_request` (`hyper_client.rs:288-298`) всегда идёт сначала в raw-путь.
- `send_raw_request` подключается через `tokio::net::TcpStream::connect((host, port))` (`hyper_client.rs:425`). Это последовательный перебор адресов без таймаута на адрес.
- IPv6-адреса стоят первыми. SYN по v6 до Cloudflare уходит в пустоту, до v4 очередь не доходит.
- Внешний timeout возвращает `Err(Timeout)`. Fallback на hyper-клиент (`:300-309`), у которого happy eyeballs работает (замер: 0.36 с, v4), срабатывает только на не-timeout ошибки.
- `connect_via_proxy` (`:502`) подключается так же.

- [ ] TCP для raw-пути (`:425`) и для `connect_via_proxy` (`:502`) — через `hyper_util::client::legacy::connect::HttpConnector` (`enforce_http(false)`, happy eyeballs 300 мс по умолчанию; `tower::ServiceExt::oneshot`, новых зависимостей нет), один общий helper.
- [ ] Тест без внешней сети: resolver-seam отдаёт `[недостижимый IPv6, 127.0.0.1:<порт stub>]` → raw-запрос доходит до stub за ограниченное время (секунды, не timeout запроса).

### Task 5: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; на agentbox — failover между двумя реальными провайдерами при искусственно сломанном ключе первого.
- [ ] Ветка `web-parity/web-01-failover-oauth`, PR; DX-59 → In Review + ссылка («этап 2 из 2»). Закрыть чекбоксы, Notes.

Риски для ревью:
- Refresh OAuth-токена в forwarder одновременно с logout из web (фаза 25): запрос не должен уйти со старым токеном после logout; поведение — как у desktop, тест с параллельным logout.
- Breakers в памяти сбрасываются при restart сервера: UI не должен показывать «открыт» по данным из DB после перезапуска.

## Notes for next phase

- (заполнить по итогам)
