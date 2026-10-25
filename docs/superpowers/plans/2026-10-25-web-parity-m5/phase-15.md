# Phase 15 — WEB-16 / DX-74: Hermes config и memory

Цель: import из live-конфига, live provider IDs, model config и memory (чтение, запись, лимиты, включение) Hermes работают в web. Ссылка на Hermes Web UI открывает адрес сервера, а не `127.0.0.1` браузера. Запуск `hermes dashboard` в терминале в web недоступен.

Спека: § WEB-16. Оценка M (2–3 дн.). Зависимость WEB-30 закрыта фазой 1.

Scope: `src-tauri/src/web_api.rs` (8 роутов), `src-tauri/tests/web_api_hermes.rs`, `src/lib/api/adapter.ts`, `src/lib/api/hermes.ts` (web-ветка `openWebUI`), тесты фронта.

Факты разведки:
- Фронт hermes.ts:19-67: `get_hermes_model_config`, `open_hermes_web_ui {path|null}`, `launch_hermes_dashboard`, `get_hermes_memory {kind}`, `set_hermes_memory {kind, content}`, `get_hermes_memory_limits`, `set_hermes_memory_enabled {kind, enabled}`; `kind` — `"memory" | "user"`. providers.ts:254-275: `get_hermes_live_provider_ids`, `import_hermes_providers_from_live`.
- Вызовы: `useHermes.ts:52-55,63,71,79,118,146,171`; `App.tsx:944,1135,1881` (`launchDashboard`); `HermesMemoryPanel.tsx:270-320` (`openWebUI("/config")`); `ProviderList.tsx:220`.
- Backend commands/hermes.rs: :22 `import_hermes_providers_from_live(State<AppState>) -> usize` → `services::provider::import_hermes_providers_from_live(&AppState)`; :31 `get_hermes_live_provider_ids()` → `hermes_config::get_providers()`; :55 `get_hermes_model_config()`; :65/:70/:75/:80 memory → `hermes_config::read_memory` (:1147), `write_memory` (:1159, через `atomic_write`), `read_memory_limits` (:1216), `set_memory_enabled` (:1192). Все handle-free. `get_hermes_live_provider` (:40) на фронте не вызывается — не переносим.
- `open_hermes_web_ui(app: AppHandle, path)` (:99, async): проба `http://127.0.0.1:{HERMES_WEB_PORT|9119}/api/status` (reqwest, таймаут 1200 мс), затем `app.opener().open_url`. Переиспользуема только проба. `launch_hermes_dashboard()` (:132) открывает видимый терминал — вне scope.
- DTO: `MemoryKind` (hermes_config.rs:1126) — Deserialize, lowercase; `HermesMemoryLimits` (:1168), `HermesWriteOutcome` (:116) — camelCase; `HermesModelConfig` (:124) — без `rename_all` (отдаём как есть).
- Тесты: `src-tauri/tests/hermes_roundtrip.rs` (провайдеры, temp-каталог Hermes), unit-тесты memory в hermes_config.rs (:2334, :2373); фронтовых тестов hermesApi нет.

### Task 1: backend — роуты + тесты
- [ ] `POST /api/hermes/import-live`, `GET /api/hermes/live-provider-ids`, `GET /api/hermes/model-config`, `GET/PUT /api/hermes/memory/:kind` (`{content}`), `GET /api/hermes/memory-limits`, `PUT /api/hermes/memory/:kind/enabled` (`{enabled}`); неизвестный kind → 400.
- [ ] `GET /api/hermes/web-ui?path=…`: та же проба `127.0.0.1:{port}/api/status` на сервере → `{reachable, url}`, где `url` = схема и хост из заголовка `Host` запроса + порт Hermes + `path` (нормализованный, начинается с `/`).
- [ ] `src-tauri/tests/web_api_hermes.rs` (temp-каталог Hermes, как в hermes_roundtrip.rs): memory write → read возвращает записанное, файл записан атомарно (нет tmp-хвостов); enabled переключает только свой флаг; limits; import-live на подготовленном конфиге импортирует N провайдеров; web-ui при поднятой заглушке на `HERMES_WEB_PORT` → `reachable:true` и `url` с хостом из запроса; без заглушки → `reachable:false`.

### Task 2: frontend
- [ ] adapter: 8 case; `launch_hermes_dashboard` не мапить.
- [ ] `hermes.ts` `openWebUI` в web: запрос `web-ui`; `reachable` → `window.open(url)` (через `openExternalInBrowser` фазы 1); иначе toast «Hermes Web UI не отвечает на сервере (порт N); если он слушает только localhost, откройте его через tailscale serve или запустите на 0.0.0.0».
- [ ] Кнопку запуска dashboard в web скрыть (терминал на сервере недоступен) — строка таблицы фазы 6 остаётся «desktop-only».
- [ ] Тесты: `tests/lib/hermesApi.test.ts`; `openWebUI` в web открывает URL от сервера и показывает toast при `reachable:false`.

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-16-hermes`, PR; DX-74 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Заголовок `Host` подставляет любой клиент: `url` уходит только в `window.open` того же клиента, но `path` нужно нормализовать, чтобы не получить `//evil.example` (тест на `path=//x`).

## Notes for next phase

- (заполнить по итогам)
