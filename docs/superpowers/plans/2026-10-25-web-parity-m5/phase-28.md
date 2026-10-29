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
- [x] `ProxyServer::new(config, db, …)` получает клон `AppState` (или нужные Arc менеджеров) вместо `Option<AppHandle>` для OAuth; forwarder берёт Copilot/Codex/xAI из него. Ветки «无 AppHandle» удаляются.
- [x] Тест: proxy в headless с Copilot-провайдером и подготовленным `copilot_auth.json` → запрос к заглушке upstream уходит с токеном менеджера (seam базового URL upstream — как в существующих тестах forwarder; если seam нет — добавить).

### Task 2: failover switch через bus
- [x] `FailoverSwitchManager::try_switch(state: &AppState, app: Option<&AppHandle>, …)`: `record_failover_route` и emit `provider-switched` — через `state` и bus; tray — только при `Some(app)`. Forwarder вызывает его и в headless.
- [x] `reset_circuit_breaker`, `set_auto_failover_enabled`: логика в handle-free функциях от `&AppState` с emit через bus; desktop-обёртки добавляют tray-refresh. `add/remove_from_failover_queue` — то же без emit.

### Task 3: backend — роуты + тесты
- [x] `GET /api/failover/health/:app/:providerId`, `POST /api/failover/breaker/:app/:providerId/reset`, `GET/PUT /api/failover/breaker-config/:app`, `GET /api/failover/breaker-stats/:app/:providerId` (заглушка `null`, как на desktop), `GET /api/failover/queue/:app`, `GET /api/failover/available/:app`, `POST /api/failover/queue/:app/:providerId`, `DELETE /api/failover/queue/:app/:providerId`, `GET/PUT /api/failover/auto/:app` (`{enabled}`). Приложение без поддержки failover (Claude Desktop) → ошибка `require_failover_app`, как на desktop.
- [x] `src-tauri/tests/web_api_failover.rs`: очередь из двух провайдеров, upstream первого отвечает 500 → после порога breaker открывается, запросы идут на второго, bus выдаёт `provider-switched`, `record_failover_route` записан, health первого — unhealthy; reset breaker → первый снова в ротации; auto-failover off → переключения нет; set auto-failover эмитит `source:"failoverEnabled"`.

### Task 4: frontend
- [x] 11 case в `mapCommand`.
- [x] Тесты: `tests/lib/failoverApi.test.ts`; SSE `provider-switched` после failover инвалидирует providers/proxy queries (маппинг фазы 8).
- [x] Снять оставшиеся WEB-00-гейты failover/breaker (таблица фазы 6).

### Task 6: TCP-connect raw-пути с happy eyeballs (IPv6 → IPv4)
Добавлена по итогам расследования из Notes фазы 27 (proxy висит на upstream за Cloudflare на agentbox). Без неё живая проверка Task 5 на реальных провайдерах невозможна.

Причина:
- `server.rs:186-203` прикладывает `OriginalHeaderCases` к каждому соединению, поэтому `send_request` (`hyper_client.rs:288-298`) всегда идёт сначала в raw-путь.
- `send_raw_request` подключается через `tokio::net::TcpStream::connect((host, port))` (`hyper_client.rs:425`). Это последовательный перебор адресов без таймаута на адрес.
- IPv6-адреса стоят первыми. SYN по v6 до Cloudflare уходит в пустоту, до v4 очередь не доходит.
- Внешний timeout возвращает `Err(Timeout)`. Fallback на hyper-клиент (`:300-309`), у которого happy eyeballs работает (замер: 0.36 с, v4), срабатывает только на не-timeout ошибки.
- `connect_via_proxy` (`:502`) подключается так же.

- [x] TCP для raw-пути (`:425`) и для `connect_via_proxy` (`:502`) — через `hyper_util::client::legacy::connect::HttpConnector` (`enforce_http(false)`, happy eyeballs 300 мс по умолчанию; `tower::ServiceExt::oneshot`, новых зависимостей нет), один общий helper.
- [x] Тест без внешней сети: resolver-seam отдаёт `[недостижимый IPv6, 127.0.0.1:<порт stub>]` → raw-запрос доходит до stub за ограниченное время (секунды, не timeout запроса).

### Task 7: флак `web_api_failover` и bookkeeping-тесты forwarder
Добавлена по итогам ревью T6. Ревьюер прогнал `a_failing_provider_switches_the_route_and_emits_through_the_proxy` по 10 раз: на базе `dd8fed87` и на голове T6 одинаково падает 6/10 (`b_seen 2 != 1`). Соседний `with_auto_failover_off_a_failing_provider_is_an_error_and_nothing_switches` падает с `b_seen 2 != 0`: stub B получает запросы, которых этот тест не порождает. Гипотеза ревьюера, не доказана: proxy или failover-задача соседнего теста живёт дальше в том же процессе (общий только mutex). Ещё в полном прогоне под нагрузкой упали три `forwarder::tests::bookkeeping::*_neutral`; на базе не проверялись.

- [x] Найти механизм лишних запросов к stub B, с доказательством, и устранить причину. Не ретраи и не ослабленные asserts. Бинарник `web_api_failover` — 20/20 зелёных прогонов подряд.
- [x] `forwarder::tests::bookkeeping::*_neutral`: воспроизводится ли на `main`/базе фазы, в одиночку и в полном прогоне. Если это регресс фазы или флак — исправить с тем же критерием. Если нет — записать в Notes с доказательством.

### Task 8: rustls CryptoProvider в headless
Добавлена по итогам живой проверки T5 (release-сборка ветки на `127.0.0.1:18799`, копия БД, временный HOME).

Что видно:
- `claude -p` через proxy на `https://opencode.ai/zen/go` висит 180 с. Каждый запрос паникует в `tokio-rt-worker` на `rustls-0.23.37/src/crypto/mod.rs:249` («Could not automatically determine the process-level CryptoProvider … make sure exactly one of the 'aws-lc-rs' and 'ring' features is enabled»). Соединение рвётся, `claude` повторяет запрос, health и request-логи не пишутся.
- Desktop ставит провайдер в `setup` (`lib.rs:524`, `rustls::crypto::ring::default_provider().install_default()`). Headless-сервер не ставит нигде.
- Баг старый: TLS в raw-пути proxy в headless не работал никогда. В фазе 27 до него не доходило из-за зависания на IPv6, его убрала T6.

- [x] Ставить ring-провайдер один раз, до любого TLS: один helper в библиотеке вызывают desktop `setup` и headless bootstrap (`bootstrap.rs`, путь `run_headless_bootstrap_with`).
- [x] Тест в отдельном integration-бинарнике (свой процесс, никто больше не ставит провайдер): после headless bootstrap `CryptoProvider::get_default()` есть. Проба: без вызова helper — красный.

### Task 5: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`; на agentbox — failover между двумя реальными провайдерами при искусственно сломанном ключе первого.
- [x] Ветка `web-parity/web-01-failover-oauth`, PR; DX-59 → In Review + ссылка («этап 2 из 2»). Закрыть чекбоксы, Notes.

Риски для ревью:
- Refresh OAuth-токена в forwarder одновременно с logout из web (фаза 25): запрос не должен уйти со старым токеном после logout; поведение — как у desktop, тест с параллельным logout.
- Breakers в памяти сбрасываются при restart сервера: UI не должен показывать «открыт» по данным из DB после перезапуска.

## Notes for next phase

- **Статус до merge (2026-10-29)**: ветка `web-parity/web-01-failover-oauth` (worktree `.worktrees/web-01-failover`), PR #29 (https://github.com/alxwlw/cc-switch-web/pull/29) открыт; DX-59 → In Review (комментарий со ссылкой, «этап 2 из 2»). Ветка также несёт закрытие фазы 27 в плане (`a34fa730`).
  - Финальные гейты на `7a36def4`:
    - fmt и `clippy --lib --examples -D warnings` — чисто;
    - `(umask 022; cargo test --no-fail-fast)` — 52 бинарника, 4519 passed / 1 failed (baseline omp_search_paths);
    - `cargo build --release --example server` — ок;
    - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` — ок; `prettier --check tests/**` находит 14 файлов, все с main, файлы фазы среди них нет;
    - `test:unit` — 3934 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - Ревью:
    - Т1 — r0 changes_requested: DP-018 (restart через `start_with` без теста) и NIT-переименование `ProxyAuth` → `OAuthManagers`; fix r1 одобрен.
    - Т2 одобрена с r0, forward F1 (spawn `try_switch` в forwarder не закреплён) закрыт тестом Т3.
    - Т3 одобрена с r0, 2 NIT не исправлялись (решение в journal).
    - Т4 одобрена с r0 без замечаний.
    - Т6 — r0 changes_requested: DP-086 (doc `ProxyStream` оторван); fix r1 одобрен, заодно закреплён `connect_via_proxy`.
    - Т7 — r0 changes_requested: DP-026 (stub в `web_api_proxy.rs` того же класса); fix r1 одобрен.
    - Т8 одобрена с r0.
    - Решения — в journal p28.
- **Отличия от плана**:
  - Добавлены три задачи:
    - Т6 — TCP-connect raw-пути с happy eyeballs. Это причина «proxy висит на Cloudflare» из Notes фазы 27.
    - Т7 — флак тестов. Причина — сканер портов на хосте, см. ниже.
    - Т8 — rustls CryptoProvider в headless, найден живой проверкой.
  - Breaker config глобальный: `GET/PUT /api/failover/breaker-config` без `:app`. Desktop-команды и фронт приложение не передают. PUT принимает объект конфига без обёртки.
  - Health и breaker-stats принимают claude-desktop (`require_mode_app`, как desktop). Роуты из `commands/failover.rs` отвечают на него 400.
  - SSE-маппинг `provider-switched` не расширялся: health и breaker-stats перезапрашиваются каждые 5 с.
- **Backend**:
  - `OAuthManagers` — `proxy/server.rs:63`. Он приходит из `ProxyService::with_auth` (`services/proxy.rs:84`, вызывается в `AppState::new`) и переживает `start_with`.
  - `ProxyService::bind_app_state` — `services/proxy.rs:91`; вызывается в `lib.rs:684` (desktop) и `bootstrap.rs:856` (headless). В `AppState::new` его нет: получилась бы петля `Arc` в каждом тестовом state. Тесты, которым нужен failover, привязывают state сами.
  - `try_switch(&AppState, Option<&AppHandle>, …)` — `proxy/failover_switch.rs:38`. Событие идёт через `state.events`, tray — только при `Some`.
  - `provider_health` очищается при каждом старте нового `ProxyServer`.
  - `*_inner`: `commands/failover.rs:81-227`, `reset_circuit_breaker_inner` — `commands/proxy.rs`.
  - Роуты — `web_api.rs:508-530`, хендлеры — `:4071-4180`.
  - `connect_tcp` — `proxy/hyper_client.rs:390`, resolver-seam для тестов.
  - `bootstrap::install_rustls_provider` — `bootstrap.rs:805`.
- **Frontend**: case — `adapter.ts:1093-1140`; тесты `tests/lib/failoverApi.test.ts`, SSE — `tests/lib/eventInvalidation.test.ts`. Гейты failover из `query/failover.ts` и `SwitchModePanel.tsx` сняты, строки таблицы фазы 6 отмечены.
- **Флак тестов — сканер портов на хосте**:
  - `moshi-hook` каждые ~16–18 с шлёт `GET /` (`Go-http-client/1.1`) на loopback-порты. Клиентский конец пойман через `ss -tnp`.
  - Stub-ы, считающие все соединения, получали лишний «запрос»: `web_api_failover` падал 6/10. Scripted fakes в тестах forwarder отдавали ответ пробе и паниковали «upstream script exhausted».
  - Теперь stub-ы фазы считают только ожидаемый запрос. Single-accept stub-ы принимают соединения в цикле.
  - Listener-ы вне фазы не проверялись: `proxy/server.rs` (6), `services/coding_plan.rs` (4), `tests/web_api_endpoints.rs` (3), `tests/web_api_global_proxy.rs` (3), `tests/web_api_hermes.rs` (2). Урок записан в auto memory.
- **Известные ограничения**:
  - OAuth data plane вживую не проверен. В конфиге сервера нет входа Copilot или Codex OAuth, подписки Copilot у владельца нет. Покрыт headless-тестом с подложенным аккаунтом Copilot (`store.rs`).
  - `get_circuit_breaker_stats` по-прежнему `null`, как заглушка desktop.
  - В тесте auto-failover off проверка флага в `provider_router.rs:70` закреплена только вместе с `max_retries=0` (`handler_context.rs:268`). Это было и до фазы.
  - Ни один тест не проверяет, что health и stats отвечают 200 для claude-desktop.
- **Живая проверка** (release-сборка ветки, `127.0.0.1:18799`, копия БД с намеренно сломанным ключом OpenCode Go, временный `HOME`, proxy на `127.0.0.1:15731`, порог breaker 1, очередь [OpenCode Go, OpenRouter]):
  - Первая попытка (до Т8): `claude -p` висел 180 с, каждый HTTPS-запрос паниковал в rustls. Отсюда Т8.
  - После Т8:
    - OpenCode Go ответил 401, breaker открылся, ответил OpenRouter (Cloudflare, то есть Т6 работает вживую).
    - Маршрут переключился на OpenRouter, по SSE пришло `provider-switched` `source:"failover"`, health OpenCode Go — unhealthy с текстом 401.
    - `claude -p` через proxy вернул `OK-P28` за 4.4 с.
    - Reset breaker: OpenCode Go снова healthy, маршрут вернулся на него, пришло событие.
    - Auto-failover off: 401, переключения и события нет.
  - UI: переключатель Failover (диалог подтверждения) включает auto-failover (backend `true`, SSE `failoverEnabled`). Видны очередь P1/P2 и бейдж «Circuit Open», ошибок в консоли нет. SIGTERM возвращает клиента на direct.
- **Живой сервис на main до merge**: TLS proxy в headless паникует (Т8), Cloudflare-upstream висят (Т6). Takeover на живом сервисе включать только после деплоя этой ветки.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет.
  - Smoke без takeover:
    - `GET /api/failover/breaker-config` → конфиг;
    - `GET /api/failover/queue/claude` → список;
    - `GET /api/failover/auto/claude` → `data` bool;
    - `GET /api/failover/health/claude/<id>` → `is_healthy`.
  - Takeover на живом сервисе — с ведома владельца: он переписывает `~/.claude/settings.json` пользователя сервера, и это перенаправит `claude` на этом хосте.
  - DX-59 → Done (этап 2 из 2).
