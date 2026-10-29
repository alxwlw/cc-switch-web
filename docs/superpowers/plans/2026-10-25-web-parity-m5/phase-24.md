# Phase 24 — WEB-23 / DX-81: global upstream/outbound proxy

Цель: URL глобального outbound proxy читается, сохраняется, тестируется, показывается его статус, локальные proxy сканируются — всё из web. Сохранённый proxy применяется к HTTP-клиенту при старте сервера. С этой фазы все исходящие запросы следующих фаз (OAuth, модели, usage) идут через настроенный proxy. Local gateway (WEB-01) не затрагивается.

Спека: § WEB-23. Оценка M (2–3 дн.). Открывает эпик proxy + OAuth: глобальный клиент используют OAuth-менеджеры, model fetch, balance, coding plan, speedtest, skills, WebDAV/S3.

Scope: `src-tauri/src/bootstrap.rs` (шаг init клиента), `src-tauri/src/lib.rs:1320-1348` (переключить на общий шаг), `src-tauri/src/web_api.rs` (5 роутов), `src-tauri/tests/web_api_global_proxy.rs`, `src/lib/api/adapter.ts` (5 case), `src/components/settings/GlobalProxySettings.tsx` (server-host подписи), тесты фронта.

Факты разведки:
- Фронт globalProxy.ts: `get_global_proxy_url` (l.41), `set_global_proxy_url {url}` (l.52), `test_proxy_url {url}` (l.67), `get_upstream_proxy_status` (l.76), `scan_local_proxies` (l.85). Хуки `useGlobalProxy.ts` (`useGlobalProxyUrl` l.24, `useSetGlobalProxyUrl` l.35); UI — `GlobalProxySettings.tsx:73`, монтируется в `SettingsPage.tsx:404`.
- Backend commands/global_proxy.rs, логика inline, AppHandle нет: l.15 `get_global_proxy_url(State<AppState>)` → `db.get_global_proxy_url()` (dao/settings.rs:147); l.35 `set_global_proxy_url(State, url)` → `http_client::validate_proxy` → `db.set_global_proxy_url` (:155) → `http_client::apply_proxy`; l.89 `test_proxy_url(url)` (async) — свой `reqwest::Client` с таймаутом 10 с, HEAD на httpbin.org, google.com, api.anthropic.com; все упали → `Ok(success:false)`, Err только на пустой/невалидный URL; l.164 `get_upstream_proxy_status()` → `http_client::get_current_proxy_url()`; l.213 `scan_local_proxies()` — TCP connect на 127.0.0.1 к портам 7890, 7891, 1080, 8080, 8888, 3128, 10808, 10809 (`PROXY_PORTS`), таймаут 100 мс — в web сканируется loopback СЕРВЕРА.
- DTO `ProxyTestResult`, `UpstreamProxyStatus`, `DetectedProxy` — camelCase.
- Клиент: `proxy/http_client.rs:13` `static GLOBAL_CLIENT: OnceCell<RwLock<Client>>` (+ `DIRECT_CLIENT`, `CURRENT_PROXY_URL`). `get()` без init логирует GP-004 и строит фоллбек-клиент с системным proxy — headless работает, но без сохранённого proxy. `init()` (l.52) при повторном вызове уходит в `apply_proxy`.
- Init на desktop — lib.rs:1320-1348: `db.get_global_proxy_url()` → `http_client::init(proxy_url)`; ошибка → `set_global_proxy_url(None)` + `init(None)`. Нужна только `Database` — выносится как есть. server.rs init не вызывает.
- Тесты: в global_proxy.rs нет; 11 в http_client.rs; `tests/components/GlobalProxySettings.test.tsx` мокает хуки.

### Task 1: init клиента в bootstrap
- [x] `bootstrap::init_global_http_client(db: &Database) -> Result<(), String>` — тело lib.rs:1320-1348 без изменений логики (включая сброс битого URL); desktop setup вызывает её же; headless bootstrap — сразу после `Database::init`. Удалить из Notes фазы 7 пометку «global proxy client → WEB-23», если она там есть.
- [x] Тест: DB с сохранённым proxy → после bootstrap `get_current_proxy_url()` возвращает его; DB с битым URL → значение в DB сброшено, клиент прямой.

### Task 2: backend — роуты + тесты
- [x] `GET /api/global-proxy` → url; `PUT /api/global-proxy` (`{url}`; пустая строка = выключить — как в команде); `POST /api/global-proxy/test` (`{url}`); `GET /api/global-proxy/status`; `POST /api/global-proxy/scan`. Команды без State вызывать напрямую; get/set — через DB и `http_client`, как в команде.
- [x] `src-tauri/tests/web_api_global_proxy.rs`: set валидного URL → status показывает его, get возвращает; set невалидного → 400/`{ok:false}`, прежнее значение не изменилось; test с URL локальной HTTP-proxy-заглушки → `success:true` (цели проверки подменить через тестовый seam, без выхода в интернет); scan находит слушающий порт из списка, поднятый в тесте (если порт занят — тест пропускается с пометкой).

### Task 3: frontend
- [x] 5 case в `mapCommand`.
- [x] `GlobalProxySettings` в web: подпись «proxy применяется к исходящим запросам сервера cc-switch-web; сканирование ищет proxy на сервере (127.0.0.1 сервера)».
- [x] Тесты: `tests/lib/globalProxyApi.test.ts`; компонент — подпись в web.
- [x] Снять WEB-00-гейт (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`; на agentbox — после set proxy и restart сервиса статус сохраняется.
- [x] Ветка `web-parity/web-23-global-proxy`, PR; DX-81 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- URL proxy с логином и паролем (`http://user:pass@host:port`): status и логи отдают его замаскированным (`mask_url`), а не открытым текстом.
- Loopback-цели (local gateway WEB-01, локальные сервисы) должны идти мимо proxy (`get_for_url` → direct client) — не сломать это при init в bootstrap.

## Notes for next phase

- **Статус до merge (2026-10-29)**: ветка `web-parity/web-23-global-proxy` (worktree `.worktrees/web-23`), PR #25 (https://github.com/alxwlw/cc-switch-web/pull/25) открыт; DX-81 In Review (комментарий со ссылкой). Ветка также несёт закрытие фазы 23 в плане (`62b59023`).
  - Финальные гейты на `2a8e9b19`:
    - fmt и `clippy --lib --examples -D warnings` — чисто (`--all-targets` даёт 87 baseline `await_holding_lock` в тестах lib, они есть и на main);
    - `(umask 022; cargo test --no-fail-fast)` — 4472 passed / 1 failed (baseline omp_search_paths);
    - `cargo build --release --example server` — ок;
    - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` — ок;
    - `test:unit` — 3866 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - Ревью: Т1, Т2 одобрены с r0. Т3 — r0 changes_requested: SHOULD-FIX DP-020, устаревшие комментарии «снимет WEB-23» в `SettingsPage.tsx`/`Sidebar.tsx`; fix round 1 одобрен. NIT Т2 DP-017 принят как есть: тесты сбрасывают proxy только на успешном пути, без Drop-guard. Решения — в journal p24.
- **Отличия от плана**:
  - status в web отдаёт URL через `mask_url`, а GET — как есть: форма раскладывает его на username и password. Desktop-команда status не менялась.
  - Тело desktop `set_global_proxy_url` вынесено в `apply_global_proxy_url` (`commands/global_proxy.rs:41`), модуль `commands::global_proxy` стал `pub(crate)`.
  - Тест loopback-обхода не добавлялся: `get_for_url` не менялся, его покрывают существующие `test_is_loopback_url`/`test_upstream_proxy_for_skips_loopback` в `http_client.rs`.
  - Пометка фазы 7 про WEB-23 — в её scope-тексте, а не в Notes; оставлена как история.
- **Backend**:
  - `bootstrap::init_global_http_client` — `bootstrap.rs:749`; headless-шаг отчёта `global_http_client` — `:807`; desktop — `lib.rs:907`.
  - Перенос блока из `.setup` позволил rustfmt переформатировать весь closure: отдельный коммит `292b79f9` (style).
  - Роуты — `web_api.rs:258-263`, хендлеры — `:2274-2320`. Seam `global_proxy_test_targets_test_hook` — `:2327`, static — `commands/global_proxy.rs:76`.
  - Тесты: `src-tauri/tests/bootstrap_global_proxy.rs` (2), `src-tauri/tests/web_api_global_proxy.rs` (5). Это отдельные бинари: глобальные OnceCell в `http_client` не сбрасываются.
  - «Битый» сохранённый URL для тестов — `ftp://…`: `Database` его не валидирует, а `http_client::init` отклоняет схему.
- **Frontend**:
  - Case — `adapter.ts:149-166`. `setGlobalProxyUrl` пробрасывает `Error` как есть (раньше в web был лишний префикс «Error: »).
  - Подпись — `GlobalProxySettings.tsx:161`, ключ `settings.globalProxy.serverSideNote`.
  - `WEB_HIDDEN_SETTINGS_SECTIONS` пуст: fallback на `general` оставлен, но без теста, пока список пуст.
- **Живая проверка** (сборка ветки на `127.0.0.1:18799`, копия БД, маленький forward proxy с логом CONNECT):
  - GP-004 больше нет;
  - set с логином и паролем → status и логи замаскированы;
  - test → `CONNECT httpbin.org:443` с `Proxy-Authorization`;
  - поиск skills.sh идёт через proxy;
  - после рестарта сервера proxy применён;
  - в Chromium: Network видна, подпись на месте; Test, Scan (нашёл proxy на 7890), Clear → Save и toast ошибки `ftp://` работают.
  - Порты из `PROXY_PORTS` на agentbox свободны, scan живого хоста вернёт пустой список. Оба тестовых proxy (18798 и 7890) сразу после старта получили по два `GET /` от неизвестного клиента, ещё до запросов сервера; к серверу это не относится.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет.
  - Smoke:
    - в journal при старте `[GlobalProxy] Initialized: direct connection` и нет GP-004;
    - `GET /api/global-proxy` → `null`;
    - `GET /api/global-proxy/status` → `enabled:false`;
    - `POST /api/global-proxy/scan` → `[]`;
    - `PUT` с `ftp://x:1` → 400.
  - Реальный proxy на живом хосте не включать: сохранённый URL применится ко всем исходящим запросам сервиса.
  - DX-81 → Done.
- **Для следующих фаз (OAuth, модели, usage)**: общий клиент — `crate::proxy::http_client::get()`/`get_for_url()`, в headless он теперь инициализирован с сохранённым proxy. Свой `reqwest::Client` в новых сервисах обходит proxy.
