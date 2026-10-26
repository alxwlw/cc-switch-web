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
- [ ] `GET /api/:app/live-settings` → `read_live_provider_settings` (прямой вызов команды); `POST /api/endpoints/speedtest` (`{urls, timeoutSecs?}`); `GET /api/:app/providers/:id/endpoints`; `POST /api/:app/providers/:id/endpoints` (`{url}`); `DELETE /api/:app/providers/:id/endpoints` (`{url}` в body); `POST /api/:app/providers/:id/endpoints/last-used` (`{url}`).
- [ ] `src-tauri/tests/web_api_endpoints.rs`: speedtest против локальной axum-заглушки возвращает `latency` и `status: 200`; недоступный URL → запись с `error`, а не 500; add → get содержит, повторный add не дублирует; remove удаляет; неизвестный provider → `{ok:false}`; live-settings читает подготовленный live-файл из temp HOME.

### Task 2: frontend
- [x] 6 case в `mapCommand`.
- [x] `EndpointSpeedTest`: в web подпись «замер выполняется с сервера cc-switch-web; задержка из вашего браузера может отличаться»; после цикла add/remove — всегда инвалидировать `["providers", appId]` (и при ошибке посередине), ошибку показать с указанием, какие URL сохранились.
- [x] Тесты: `tests/lib/endpointsApi.test.ts`; компонент — подпись видна в web, ошибка на втором add инвалидирует providers-query.
- [x] Снять WEB-00-гейт с speed test (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-21-endpoint-speedtest`, PR; DX-79 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Speed test по loopback-URL (`http://127.0.0.1:…`) измеряет сервисы на сервере, а не на машине пользователя: подпись должна покрывать и этот случай.
- Сотня URL в одном запросе: время ответа ограничено `timeoutSecs` сервиса, а не суммой (проверить, что тесты идут параллельно, как на desktop).

## Notes for next phase

- (заполнить по итогам)
