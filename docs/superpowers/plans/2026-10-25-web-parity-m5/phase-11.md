# Phase 11 — WEB-21 / DX-79: endpoint speed test и custom endpoints

Цель: тест скорости endpoint-ов, CRUD custom endpoints и чтение live-настроек провайдера работают в web. UI прямо говорит, что задержка измерена с сервера, а не из браузера.

Спека: § WEB-21. Оценка M (1–3 дн.).

Scope: `src-tauri/src/web_api.rs` (6 роутов), `src-tauri/tests/web_api_endpoints.rs`, `src/lib/api/adapter.ts` (6 case), `src/components/providers/forms/EndpointSpeedTest.tsx` (подпись + сохранение без частичного состояния), тесты фронта.

Факты разведки:
- Фронт vscode.ts:12-70: `read_live_provider_settings {app}`, `test_api_endpoints {urls, timeoutSecs?}`, `get_custom_endpoints {app, providerId}`, `add_custom_endpoint {app, providerId, url}`, `remove_custom_endpoint {app, providerId, url}`, `update_endpoint_last_used {app, providerId, url}` (без UI-вызова).
- UI: `EndpointSpeedTest.tsx:132` (get), `:337` (test), `:425` (add), `:430` (remove) — add/remove идут последовательно в цикле, ошибка посередине оставляет DB частично сохранённой; `EditProviderDialog.tsx:233` (live settings, только для текущего провайдера). React Query нет; endpoints лежат в `meta.custom_endpoints` провайдера → устаревает ключ `["providers", appId]` (mutations.ts:105).
- Backend provider.rs: `read_live_provider_settings(app: String)` :870 (без State), `test_api_endpoints` (async) :879 (без State) → `SpeedtestService::test_endpoints(urls, Option<u64>)` (services/speedtest.rs:26) через `proxy::http_client::get_for_url` (:72); `get_custom_endpoints` :887, `add_custom_endpoint` :898, `remove_custom_endpoint` :910, `update_endpoint_last_used` :922 — `State<AppState>` → `ProviderService::*` (provider/mod.rs:7800-7836 → endpoints.rs:13,35,57,71). AppHandle, emit, tray нет.
- DTO: `EndpointLatency` (speedtest.rs:14) — без `rename_all`, поля однословные; `CustomEndpoint` (settings.rs:14) — camelCase; live settings — `serde_json::Value`.
- Тесты: 3 inline в speedtest.rs; интеграционных нет.

### Task 1: backend — роуты + тесты
- [x] `GET /api/:app/live-settings` → `read_live_provider_settings` (прямой вызов команды); `POST /api/endpoints/speedtest` (`{urls, timeoutSecs?}`); `GET /api/:app/providers/:id/endpoints`; `POST /api/:app/providers/:id/endpoints` (`{url}`); `DELETE /api/:app/providers/:id/endpoints` (`{url}` в body); `POST /api/:app/providers/:id/endpoints/last-used` (`{url}`).
- [x] `src-tauri/tests/web_api_endpoints.rs`: speedtest против локальной axum-заглушки возвращает `latency` и `status: 200`; недоступный URL → запись с `error`, а не 500; add → get содержит, повторный add не дублирует; remove удаляет; неизвестный provider → `{ok:false}`; live-settings читает подготовленный live-файл из temp HOME.

### Task 2: frontend
- [x] 6 case в `mapCommand`.
- [x] `EndpointSpeedTest`: в web подпись «замер выполняется с сервера cc-switch-web; задержка из вашего браузера может отличаться»; после цикла add/remove — всегда инвалидировать `["providers", appId]` (и при ошибке посередине), ошибку показать с указанием, какие URL сохранились.
- [x] Тесты: `tests/lib/endpointsApi.test.ts`; компонент — подпись видна в web, ошибка на втором add инвалидирует providers-query.
- [x] Снять WEB-00-гейт с speed test (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-21-endpoint-speedtest`, PR; DX-79 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Speed test по loopback-URL (`http://127.0.0.1:…`) измеряет сервисы на сервере, а не на машине пользователя: подпись должна покрывать и этот случай.
- Сотня URL в одном запросе: время ответа ограничено `timeoutSecs` сервиса, а не суммой (проверить, что тесты идут параллельно, как на desktop).

## Notes for next phase

- **Статус до merge (2026-10-26)**: ветка `web-parity/web-21-endpoint-speedtest` (worktree `.worktrees/web-21`), PR #12 (https://github.com/alxwlw/cc-switch-web/pull/12) открыт, DX-79 In Review (комментарий со ссылкой). T1 одобрена с r1: в r0 SHOULD-FIX DP-015, имя `timeoutSecs` не было закреплено тестом, и после удаления `rename_all` у `SpeedtestReq` тесты оставались зелёными; закрыто в `1919071b`. T2 одобрена с r0. Финальные гейты на `5c302fec`: fmt чисто; clippy — новых видов предупреждений нет (`await_holding_lock` в тестах, как у соседей); `cargo build --release --example server` ок; `cargo test --no-fail-fast` 4313 passed / 3 failed (baseline: omp_search_paths, 2× file_modes::switch_*; нестабильные bookkeeping в этот раз зелёные); `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3693 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice). В PR едет неподтолкнутый `5ea1f58c` (docs закрытия фазы 10 с локального main). После merge — `post-merge`: пересобрать server и `dist-web` (Rust менялся), перед install `cp cc-switch-web cc-switch-web.prev`; миграций БД нет; DX-79 → Done.
- **Роуты** (`web_api.rs`): `GET /api/:app/live-settings` → `ProviderService::read_live_settings`; `POST /api/endpoints/speedtest` (`{urls, timeoutSecs?}`); `GET|POST|DELETE /api/:app/providers/:id/endpoints` (`{url}` в body у POST и DELETE); `POST …/endpoints/last-used`. DB- и файловая работа идёт через `spawn_blocking`. Ошибки сервиса возвращаются в принятой форме `fail()`: 500 `{ok:false,error}`.
- **Speedtest**: дефолтный таймаут сервиса — 8 с на запрос (`speedtest.rs:8`), clamp 2–30 с, плюс warmup-запрос, то есть без аргумента около 16 с. Пробы идут через `join_all`; тест с `Barrier(3)` падает, если их сериализовать. Тест `timeoutSecs: 2` стоит около 4 с. Ошибки недоступного URL приходят по строкам, в исходном тексте сервиса (китайский), HTTP 200.
- **Поведение как на desktop, не чинили**: `update_endpoint_last_used` — no-op (в таблице нет колонки `last_used`), тест проверяет только «принято, список цел». Повторный add вставляет дубль строки (у `provider_endpoints` нет UNIQUE, `database/dao/providers.rs:527`), GET схлопывает дубли по нормализованному URL, DELETE удаляет все.
- **DELETE с body**: адаптер шлёт body для любого не-GET метода (`adapter.ts:~550`), это закреплено в `tests/lib/endpointsApi.test.ts`. Reverse proxy, который режет body у DELETE, сломает удаление endpoint — учесть, если перед cc-switch-web появится такой прокси.
- **Frontend**: `EndpointSpeedTest` теперь зовёт `useQueryClient()`, поэтому требует `QueryClientProvider`; он рендерится только из Claude/Codex/Gemini FormFields под `main.tsx` и `tray-panel/main.tsx`, где провайдер есть. Подпись `endpointTest.serverSideNote` показывается в web всегда, не только для loopback, и прямо называет `127.0.0.1`/`localhost`. При ошибке посередине `initialCustomUrls` выставляется в реально применённое, поэтому повтор не переотправляет уже сохранённые URL. `EditProviderDialog` (live-settings для текущего провайдера) гейта не имел: он и раньше ловил ошибку и откатывался на сохранённый конфиг, теперь в web просто получает данные.
- **NIT не делали**: T2 DP-313 — сообщение `endpointTest.savePartial` не различает добавленные и удалённые URL, при повторной попытке перечисляет только текущую попытку и не упоминает запрос, который упал на клиенте (например, по таймауту), но успел сохраниться на сервере. Правится только текст сообщения; тест — две неудачные попытки подряд с проверкой второго сообщения. T1: тест last-used не отличает no-op от handler с эффектом (сознательно, desktop тоже no-op).
- **Инвентаризация фазы 6**: строка endpoint speed test перенесена в «закрыто фазами 1–5, 7, 9, 10, 11».
