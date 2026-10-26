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
- [x] `POST /api/hermes/import-live`, `GET /api/hermes/live-provider-ids`, `GET /api/hermes/model-config`, `GET/PUT /api/hermes/memory/:kind` (`{content}`), `GET /api/hermes/memory-limits`, `PUT /api/hermes/memory/:kind/enabled` (`{enabled}`); неизвестный kind → 400.
- [x] `GET /api/hermes/web-ui?path=…`: та же проба `127.0.0.1:{port}/api/status` на сервере → `{reachable, url}`, где `url` = схема и хост из заголовка `Host` запроса + порт Hermes + `path` (нормализованный, начинается с `/`).
- [x] `src-tauri/tests/web_api_hermes.rs` (temp-каталог Hermes, как в hermes_roundtrip.rs): memory write → read возвращает записанное, файл записан атомарно (нет tmp-хвостов); enabled переключает только свой флаг; limits; import-live на подготовленном конфиге импортирует N провайдеров; web-ui при поднятой заглушке на `HERMES_WEB_PORT` → `reachable:true` и `url` с хостом из запроса; без заглушки → `reachable:false`.

### Task 2: frontend
- [x] adapter: 8 case; `launch_hermes_dashboard` не мапить.
- [x] `hermes.ts` `openWebUI` в web: запрос `web-ui`; `reachable` → `window.open(url)` (через `openExternalInBrowser` фазы 1); иначе toast «Hermes Web UI не отвечает на сервере (порт N); если он слушает только localhost, откройте его через tailscale serve или запустите на 0.0.0.0».
- [x] Кнопку запуска dashboard в web скрыть (терминал на сервере недоступен) — строка таблицы фазы 6 остаётся «desktop-only».
- [x] Тесты: `tests/lib/hermesApi.test.ts`; `openWebUI` в web открывает URL от сервера и показывает toast при `reachable:false`.

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-16-hermes`, PR; DX-74 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Заголовок `Host` подставляет любой клиент: `url` уходит только в `window.open` того же клиента, но `path` нужно нормализовать, чтобы не получить `//evil.example` (тест на `path=//x`).

## Notes for next phase

- **Статус до merge (2026-10-26)**: ветка `web-parity/web-16-hermes` (worktree `.worktrees/web-16`), PR #16 (https://github.com/alxwlw/cc-switch-web/pull/16) открыт; DX-74 In Review (комментарий со ссылкой).
  - Т1 одобрена с r1. r0: SHOULD-FIX DP-018, фильтр управляющих символов в `normalize_web_path` не был закреплён тестом, закрыто в `ba84aced` (кейсы `/%09/x`, `%0A//x`) вместе с NIT: голый IPv6 `::1` в `Host` → 400.
  - Т2 одобрена с r1. r0: SHOULD-FIX DP-018, снятые сверх брифа Hermes-гейты в `ProviderList`/`App.handleDuplicate` не были закреплены, закрыто в `c636bcc4` (3 теста ProviderList, 1 в App.test) вместе с NIT DP-019 (строки фазы 6).
  - Финальные гейты на `c636bcc4`: `cargo fmt --check` и `cargo clippy -- -D warnings` чисто. `cargo test --no-fail-fast`: 4363 passed / 3 failed — только baseline (omp_search_paths, 2× file_modes::switch_*). `cargo build --release --example server` ок. `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3731 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - `cargo clippy --all-targets -- -D warnings` красный на ~13 тестовых бинарниках, lib tests и `keychain.rs` — всё существующий код (в т.ч. `await_holding_lock` в `tests/bootstrap_runtime.rs`, `tests/web_api_omo.rs`); гейт фазы — `cargo clippy -- -D warnings`.
- **Post-merge**: пересобрать server и `dist-web` (Rust менялся); перед install — `cp cc-switch-web cc-switch-web.prev`. Миграций БД нет. Hermes на серверном хосте не установлен — smoke `GET /api/hermes/web-ui` ожидаемо даст `reachable:false`. DX-74 → Done.
- **Роуты** (`web_api.rs`, блок «Hermes (WEB-16)»): `POST /api/hermes/import-live`, `GET /api/hermes/live-provider-ids`, `GET /api/hermes/model-config`, `GET|PUT /api/hermes/memory/:kind` (`{content}`), `GET /api/hermes/memory-limits`, `PUT /api/hermes/memory/:kind/enabled` (`{enabled}`), `GET /api/hermes/web-ui?path=…`. Неизвестный kind → 400 на всех трёх memory-роутах.
  - `web-ui` отдаёт `{reachable, port, url}`; `port` — для текста toast. `url` = `http://` + хост из `Host` без порта (IPv6 только в скобках; пустой/невалидный `Host` → 400) + порт Hermes + путь: управляющие символы выброшены, все ведущие `/` и `\` срезаны, затем один `/` (`//x` → `/x`).
  - Схема всегда `http`: у Hermes нет своего TLS. Проба и порт вынесены из desktop-команды в `commands/hermes.rs` (`hermes_web_port()`, `probe_hermes_web()`, `pub(crate)`), поведение desktop не менялось.
- **Фронт**: 8 case в адаптере, `launch_hermes_dashboard` не мапится (тест это фиксирует). `hermesApi.openWebUI` в web: проба → `reachable` → `open_external(url)`; иначе `HermesWebUiUnreachableError(port)` → toast `hermes.webui.unreachableRemote` (во всех 5 локалях).
  - Кнопку запуска dashboard отдельно не прятали: confirm-диалог запуска (`App.tsx`, единственный вызывающий `launch_hermes_dashboard`) открывается только из `onOffline` хука `useOpenHermesWebUI`, а web-роут никогда не отдаёт `hermes_web_offline` — `reachable:false` уходит в toast до `onOffline` (тест). Кнопка «Web UI» в web видна и работает.
  - Сверх брифа сняты остальные Hermes-гейты WEB-00: view `hermesMemory`, вкладка memory, live-membership секции и import-live в `ProviderList`, live ids в `handleDuplicate`, причина «Remove unverifiable», 4 запроса `useHermes`. Все обслуживаются роутами Т1. Тесты: `tests/lib/hermesApi.test.ts`, `tests/components/ProviderList.test.tsx` (3 web hermes), `tests/integration/App.test.tsx`; 4 кейса `webSilencedQueries` удалены (заменены hermesApi-тестами).
- **Известные ограничения** (из concerns ревью, не findings):
  - Если Hermes на сервере слушает только `127.0.0.1`, проба даёт `reachable:true`, а `http://<host>:9119` из браузера недоступен — toast в этом случае не появится. Решение — `tailscale serve` или bind на `0.0.0.0`; спека этот случай не различает.
  - `window.open` вызывается после await пробы — Safari (окно user activation ~1 с) может заблокировать popup; тогда пользователь видит toast `openFailed`. На медленной пробе (до 1200 мс таймаут) риск выше.
- **Инвентаризация фазы 6**: строки Hermes помечены «закрыт WEB-16»; launch dashboard остаётся desktop-only.
