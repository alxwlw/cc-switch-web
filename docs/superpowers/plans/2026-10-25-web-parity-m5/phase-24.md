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
- [ ] `bootstrap::init_global_http_client(db: &Database) -> Result<(), String>` — тело lib.rs:1320-1348 без изменений логики (включая сброс битого URL); desktop setup вызывает её же; headless bootstrap — сразу после `Database::init`. Удалить из Notes фазы 7 пометку «global proxy client → WEB-23», если она там есть.
- [ ] Тест: DB с сохранённым proxy → после bootstrap `get_current_proxy_url()` возвращает его; DB с битым URL → значение в DB сброшено, клиент прямой.

### Task 2: backend — роуты + тесты
- [ ] `GET /api/global-proxy` → url; `PUT /api/global-proxy` (`{url}`; пустая строка = выключить — как в команде); `POST /api/global-proxy/test` (`{url}`); `GET /api/global-proxy/status`; `POST /api/global-proxy/scan`. Команды без State вызывать напрямую; get/set — через DB и `http_client`, как в команде.
- [ ] `src-tauri/tests/web_api_global_proxy.rs`: set валидного URL → status показывает его, get возвращает; set невалидного → 400/`{ok:false}`, прежнее значение не изменилось; test с URL локальной HTTP-proxy-заглушки → `success:true` (цели проверки подменить через тестовый seam, без выхода в интернет); scan находит слушающий порт из списка, поднятый в тесте (если порт занят — тест пропускается с пометкой).

### Task 3: frontend
- [ ] 5 case в `mapCommand`.
- [ ] `GlobalProxySettings` в web: подпись «proxy применяется к исходящим запросам сервера cc-switch-web; сканирование ищет proxy на сервере (127.0.0.1 сервера)».
- [ ] Тесты: `tests/lib/globalProxyApi.test.ts`; компонент — подпись в web.
- [ ] Снять WEB-00-гейт (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; на agentbox — после set proxy и restart сервиса статус сохраняется.
- [ ] Ветка `web-parity/web-23-global-proxy`, PR; DX-81 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- URL proxy с логином и паролем (`http://user:pass@host:port`): status и логи отдают его замаскированным (`mask_url`), а не открытым текстом.
- Loopback-цели (local gateway WEB-01, локальные сервисы) должны идти мимо proxy (`get_for_url` → direct client) — не сломать это при init в bootstrap.

## Notes for next phase

- (заполнить по итогам)
