# Phase 27 — WEB-01 / DX-59 (этап 1): local proxy lifecycle, routing/Stack, takeover

Цель: из web запускается и останавливается local proxy на сервере, переключаются direct/route/Stack, включается и выключается live takeover для приложений, выбирается маршрут, меняются global/per-app настройки proxy. После перезапуска сервера режимы восстанавливаются, при остановке сервера takeover корректно снимается. Failover, circuit breaker, health и OAuth-маршруты — фаза 28; DX-59 после этой фазы остаётся In Progress.

Спека: § WEB-01 (L, 10–15 дн.; этап «базовый proxy без OAuth-маршрутов»). Зависимости WEB-32 (фаза 7) и WEB-29 (фаза 8) закрыты; OAuth-менеджеры уже в AppState (фазы 25–26).

Scope: `src-tauri/src/services/proxy.rs` (emit/tray без AppHandle), `src-tauri/src/mode/controller.rs:846` (emit → bus), `src-tauri/src/commands/proxy.rs` (tray-обёртки), `src-tauri/src/bootstrap.rs` (startup), `src-tauri/examples/server.rs` (graceful shutdown), `src-tauri/src/web_api.rs` (≈20 роутов), `src-tauri/tests/web_api_proxy.rs`, `src/lib/api/adapter.ts`, тесты фронта.

Факты разведки:
- Фронт proxy.ts:21-168 — 24 invoke; в эту фазу входят 20: `start_proxy_server`, `stop_proxy_server`, `stop_proxy_with_restore`, `get_proxy_status`, `get_proxy_takeover_status`, `set_proxy_takeover_for_app {appType, enabled, stack, route?}`, `set_proxy_route {appType, providerId}`, `get_app_mode`, `take_startup_attach_failures`, `exit_proxy_apps_in_mode {stack}`, `get_direct_provider`, `get_proxy_stack`, `set_proxy_stack_member {appType, providerId, enabled}`, `adopt_codex_stack_catalog`, `get/update_global_proxy_config`, `get/set_proxy_client_host`, `get/update_proxy_config_for_app`. Не входят: `get/set_pricing_model_source` (фаза 13), `restart_codex_app_server_daemon` и `acknowledge_codex_stale_clients` входят в эту фазу двумя простыми роутами без State: они управляют Codex-демоном на сервере, где Codex CLI и работает под takeover.
- Вызовы: `useProxyStatus.ts:34,61,86,127,167`, `useModeActions.ts:48,95`, `SwitchModePanel.tsx:224,365`, `RoutingSection.tsx:126`, `lib/query/proxy.ts:31-355`. Событие `proxy-official-warning` (`App.tsx:527-528`, `{appType, providerName}`); статус proxy фронт опрашивает query-хуками.
- Backend commands/proxy.rs (`State<AppState>`, если не сказано иное): :33 start → `ProxyService::start()` (services/proxy.rs:84; handle-free, кроме `notify_tray`); :41 stop → `ProxyService::stop()` (:91; отказ, если какое-то приложение в takeover); :61 stop-with-restore → `mode::controller::exit_all(&AppState)` (controller.rs:968); :67 takeover status (:184); :77 set takeover → `enter_with_route` (:718) / `exit` (:855); :100 app mode → `app_mode_view` (:1869); :111 `set_proxy_route(app_handle, …)` → `controller::set_route` (:1148) + `tray::refresh_tray_menu`; :126 `take_startup_attach_failures` (без State, глобал controller.rs:1911); :133 → controller.rs:870; :142 → :1853; :152 → :1390; :188 `set_proxy_stack_member(AppHandle, …)` → :1270 + tray, ошибка `StackWriteError`; :208 → :1447; :216 status (services/proxy.rs:453); :245/:261 global config → `update_global_config` (:502) + `resync_routes` (controller.rs:1595); :274/:283 client host — в `settings.json`, не в DB; :304/:319 per-app config.
- `ProxyService` (services/proxy.rs:19-26) = `{db, server, app_handle: Arc<RwLock<Option<AppHandle>>>, switch_locks}`; `set_app_handle` вызывается только в lib.rs:734. Без handle: `emit` (:214) — no-op (используется только для `proxy-official-warning`, controller.rs:846); `notify_tray` (:98-101) → `tray::schedule_tray_status_check`; handle передаётся в `ProxyServer::new` (:140, :576) — для failover и OAuth (фаза 28).
- Старт: `mode::operation::recover_on_startup(&Database)` (operation.rs:464, sync) уже в bootstrap фазы 7; `mode::controller::startup(&AppState)` (controller.rs:1662, async) — цикл по `PROXY_APPS` (Claude, Codex, Gemini, GrokBuild, ClaudeDesktop): `lock_settled` → `startup_app` (legacy backups, direct/proxy по флагам DB, `live-state.json`) → re-attach (может запустить proxy) → `stop_server_if_unused` → `codex_client_catalog::observe`; на desktop вызывается в lib.rs:1371.
- Takeover пишет live-конфиги CLI на сервере (Claude `settings.json`, Codex `config.toml`/`auth.json`, Gemini `.env`, GrokBuild, Claude Desktop) через `live::engine` с журналом intent; URL proxy — `build_proxy_urls` (services/proxy.rs:251) с client host из `settings.json`.
- Хранение: per-app — таблица `proxy_config` (schema.rs:134), global — dao/proxy.rs:32.
- DTO: camelCase — `GlobalProxyConfig`, `AppProxyConfig`, `RouteSwitchResult`, `StackWriteError`, `AppModeView`, `StartupAttachFailure`, `StackView`; snake_case — `ProxyStatus` (types.rs:60), `ProxyServerInfo` (:104); `ProxyTakeoverStatus` (:112) с rename `"claude-desktop"`. Отдаём как есть.
- `cleanup_before_exit(&app)` на desktop восстанавливает takeover/live-бэкапы перед выходом (используется в restart/update).
- Тесты: `src-tauri/tests/proxy_commands.rs` (только pricing source), unit-тесты в controller.rs (~:2665), golden-тесты `tests/golden/`; HTTP-тестов takeover/recovery нет.

### Task 1: proxy без AppHandle
- [ ] `ProxyService`: `emit` → `state.events` (bus фазы 8) — `proxy-official-warning` с тем же payload; `notify_tray` остаётся no-op без handle. `set_app_handle` остаётся только для tray/failover/OAuth до фазы 28.
- [ ] tray-refresh в `set_proxy_route`/`set_proxy_stack_member` оставить в desktop-обёртках; логику вызвать через общую функцию, которую зовёт и web-хендлер.

### Task 2: startup и shutdown в headless
- [ ] `bootstrap.rs`: после `recover_on_startup` и init клиента — `mode::controller::startup(&state).await`; сбои attach попадают в `take_startup_attach_failures`, как на desktop. Удалить из Notes фазы 7 пометку «controller::startup → WEB-01», если она там есть.
- [ ] Найти handle-free ядро `cleanup_before_exit` (снятие takeover + восстановление live-бэкапов) — `pub fn shutdown_cleanup(state: &AppState)`; desktop-`cleanup_before_exit` вызывает его. server.rs: `axum::serve(...).with_graceful_shutdown(ctrl_c | SIGTERM)` → `shutdown_cleanup`. Фазы 35 и 37 используют это ядро.

### Task 3: backend — роуты + тесты
- [ ] Роуты для 20 команд из фактов и двух Codex-daemon команд (`POST /api/proxy/codex/restart-daemon`, `POST /api/proxy/codex/ack-stale-clients`): `POST /api/proxy/start|stop|stop-with-restore`, `GET /api/proxy/status`, `GET /api/proxy/takeover`, `PUT /api/proxy/takeover/:app` (`{enabled, stack, route?}`), `PUT /api/proxy/route/:app` (`{providerId}`), `GET /api/proxy/mode/:app`, `POST /api/proxy/startup-attach-failures/take`, `POST /api/proxy/exit-apps` (`{stack}`), `GET /api/proxy/direct/:app`, `GET /api/proxy/stack/:app`, `PUT /api/proxy/stack/:app/:providerId` (`{enabled}`), `POST /api/proxy/codex/adopt-stack-catalog`, `GET/PUT /api/proxy/config`, `GET/PUT /api/proxy/client-host`, `GET/PUT /api/proxy/config/:app`. Ошибки `StackWriteError` — сериализованные, как desktop отдаёт фронту.
- [ ] `src-tauri/tests/web_api_proxy.rs` (temp HOME, порт proxy = 0/свободный): start → status `running`; takeover Claude → `settings.json` указывает на proxy URL, takeover status `claude: true`; stop при активном takeover → отказ; takeover off → live-файл снова указывает на direct-провайдера (совпадает с файлом пользователя, если direct-провайдер — тот же); route switch меняет провайдера маршрута; stack member add/remove; запрос через proxy к заглушке upstream с plain-ключом доходит; startup после «падения» (оставленный `live-state.json` с takeover) повторно attach-ит и поднимает proxy; `shutdown_cleanup` снимает takeover и возвращает исходные live-файлы.

### Task 4: frontend
- [ ] case в `mapCommand` для команд фазы.
- [ ] `RoutingSection`/`SwitchModePanel` в web: подпись «proxy работает на сервере и перенастраивает CLI на сервере; CLI на вашей машине он не затрагивает».
- [ ] Тесты: `tests/lib/proxyApi.test.ts`; `useProxyStatus` в web опрашивает через adapter; SSE `proxy-official-warning` показывает toast.
- [ ] Снять WEB-00-гейты с proxy/routing (таблица фазы 6); failover-controls остаются под гейтом до фазы 28.

### Task 5: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; на agentbox: takeover Claude Code на сервере → `claude` CLI на сервере ходит через proxy; `systemctl --user restart cc-switch-web` → режим восстановлен.
- [ ] Ветка `web-parity/web-01-proxy-core`, PR; DX-59 остаётся In Progress, комментарий со ссылкой на PR и «этап 1 из 2». Закрыть чекбоксы, Notes (ядро shutdown — для фаз 35/37; restore из фазы 18 при активном takeover).

Риски для ревью:
- `kill -9` сервера при активном takeover: live-файлы CLI остаются направленными на мёртвый proxy до следующего старта; `recover_on_startup` + `controller::startup` обязаны это исправить (тест с журналом, оставленным «посреди» операции).
- Listen address proxy: по умолчанию loopback. Если пользователь в web выставит `0.0.0.0`, proxy станет доступен tailnet без аутентификации — предупреждение в UI рядом с полем.

## Notes for next phase

- (заполнить по итогам)
- Риск из фазы 18 (WEB-10): restore DB-backup-а (`POST /api/backups/:filename/restore`, `restore_db_backup_file`) меняет DB под работающим proxy/takeover. Поведение должно совпадать с desktop — проверить тестом или явной заметкой при реализации lifecycle proxy.
