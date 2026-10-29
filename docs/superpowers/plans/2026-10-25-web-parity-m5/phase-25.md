# Phase 25 — WEB-02 / DX-60 (этап 1): managed OAuth state, device-code flows, accounts

Цель: OAuth-менеджеры живут в `AppState` (один экземпляр на процесс, общий для desktop DI и web). Device-code вход GitHub Copilot, Codex OAuth и xAI, а также список, статус, удаление аккаунтов, аккаунт по умолчанию и logout работают в web. Копирование кода работает и на http-origin tailnet. Browser-login через loopback callback в web недоступен — честно, с объяснением. DX-60 после фазы остаётся In Progress.

Спека: § WEB-02 (этап «device-code flows и account CRUD» — M, 3–5 дн.). Зависимости WEB-30 (фаза 1) и WEB-32 (фаза 7) закрыты.

⚠️ Решение по browser login (спека: «заменить loopback callback на безопасный tailnet flow либо явно ограничить UX device code»): ограничиваем device code. Codex browser-flow слушает `127.0.0.1:1455` сервера (`codex_oauth_auth/browser.rs:15,145-149`) — callback из браузера на другой машине туда не попадёт, а у Codex есть device-flow. Antigravity — loopback на случайном порту плюс macOS keychain (`agy_account/keychain.rs:79` `cfg!(target_os="macos")`), на Linux-сервере `start_login` уже возвращает unsupported (`agy_account/mod.rs:181`). Решение зафиксировать комментарием в DX-60.

Scope: `src-tauri/src/store.rs` (поля менеджеров), `src-tauri/src/lib.rs:1276-1318` (`app.manage` — обёртки над теми же Arc), `src-tauri/src/commands/auth.rs` (inner-функции от `&AppState`), `src-tauri/src/web_api.rs` (9 роутов + `platform` в capabilities), `src-tauri/tests/web_api_auth.rs`, `src/lib/api/adapter.ts` (9 case + локальный clipboard), `src/components/providers/forms/hooks/useManagedAuth.ts` (web: device code), тесты фронта.

Факты разведки:
- `AppState` (store.rs:9-30) = `{db, proxy_service, usage_cache, codex_oauth_manager: Arc<CodexOAuthManager>}`; Codex уже создаётся в `AppState::new` из `config::get_app_config_dir()` и в headless есть.
- Managed states (lib.rs): `CopilotAuthState(Arc<RwLock<CopilotAuthManager>>)` из `CopilotAuthManager::new(app_config_dir)` (1276-1288); `CodexOAuthState(Arc<CodexOAuthManager>)` — клон `AppState.codex_oauth_manager` (1291-1298); `XaiOAuthState(Arc<RwLock<XaiOAuthManager>>)` (1301-1310); `AgyAccountState(Arc<AgyAccountManager>)` (1313-1318); `KimiOAuthManager::default()` (1273, переносится в фазе 26). Всем нужен только каталог данных.
- Те же states берут: proxy forwarder через `app_handle.state::<…>()` (forwarder.rs:1427,1681,2107,2158,2227,3199,3249,3273 — фаза 28), `commands/provider.rs:521-522`, `stream_check.rs:17`, `misc.rs:4854`, `subscription.rs:24`. Два экземпляра разошлись бы по токенам — нужен ровно один.
- Фронт auth.ts: `auth_start_login {authProvider, githubDomain|null, targetAccountId|null, loginMethod?}` (l.46), `auth_poll_for_account {authProvider, deviceCode, githubDomain}` (l.60), `auth_cancel_login {authProvider, deviceCode}` (l.72), `auth_list_accounts` (l.82), `auth_get_status` (l.90), `auth_remove_account {authProvider, accountId}` (l.98), `auth_set_default_account` (l.106), `auth_logout` (l.114), `agy_account_is_running` (l.123). Провайдеры — `github_copilot | codex_oauth | xai_oauth | agy_oauth`; `LoginMethod` — `browser | device_code`.
- Вызовы: `useManagedAuth.ts` (start :125-131, poll :188, cancel :81, logout :256, remove :285, default :315, agyIsRunning :319, open_external :154, copyText :146); `ManagedAccountsGroup.tsx:199-210`; `XaiOAuthSection.tsx:93` (copyText); статусы — `ProviderCard.tsx:206`, `OpenTerminalDialog.tsx:131,140`, `AgyOAuthSection.tsx:193`, `useSidebarStatus.ts:46`.
- Backend commands/auth.rs — только `State<…>`, без AppHandle: l.130 start (4 manager-states), l.204 poll (+`AppState`; Codex poll берёт `proxy_service.lock_switch_for_app("codex")`), l.287 cancel (только codex и agy, прочие → Err), l.302 list, l.355 status, l.437 remove, l.500 set default, l.542 logout (`logout_codex_oauth_with_switch_lock(&AppState)`, `pub(crate)`). Менеджеры handle-free: Copilot `start_device_flow`/`poll_for_token` (copilot_auth.rs:556), Codex `start_device_flow`/`start_browser_flow`/`poll_for_token` (codex_oauth_auth.rs:374), xAI (xai_oauth_auth.rs:190), Agy `start_login`/`poll_login` (agy_account/mod.rs:144,178,~230).
- Токены: `<app_config_dir>/copilot_auth.json`, `codex_oauth_auth.json`, `xai_oauth_auth.json`, `agy_accounts.json`.
- DTO `ManagedAuthAccount`, `ManagedAuthStatus`, `ManagedAuthDeviceCodeResponse` (auth.rs:21-52) — snake_case (TS-типы тоже snake_case); аргументы команд — camelCase через `#[tauri::command(rename_all="camelCase")]`.
- Clipboard: `src/lib/clipboard.ts` — `invoke("copy_text_to_clipboard")`, затем `navigator.clipboard.writeText`; последний требует secure context, а http-origin tailnet им не является.
- Тесты: inline в copilot_auth.rs (25), xai_oauth_auth.rs (12), codex_oauth_auth/browser.rs (9), agy_account (23); в commands/auth.rs нет; фронт — `tests/hooks/useManagedAuth.test.tsx`, `AgyOAuthSection.test.tsx`, `ProviderCard.codexAccount.test.tsx`.

### Task 1: менеджеры в AppState
- [x] `AppState` получает `copilot_auth: Arc<RwLock<CopilotAuthManager>>`, `xai_oauth: Arc<RwLock<XaiOAuthManager>>`, `agy_account: Arc<AgyAccountManager>`; `AppState::new` создаёт их из `get_app_config_dir()`, как Codex.
- [x] lib.rs: `app.manage(CopilotAuthState(app_state.copilot_auth.clone()))` и аналоги — те же Arc, новых экземпляров нет. Остальные команды со `State<CopilotAuthState>` и т.п. не меняются.
- [x] Тест: `Arc::ptr_eq` между полем AppState и managed state после setup-хелпера (или unit на конструкторе); существующие тесты менеджеров зелёные.

### Task 2: auth-команды поверх AppState
- [x] В auth.rs вынести `pub(crate) async fn *_inner(state: &AppState, …)` для start/poll/cancel/list/status/remove/default/logout и `agy_account_is_running`; команды принимают `State<AppState>` и вызывают inner.
- [x] Web-роуты: `POST /api/auth/:provider/login` (`{githubDomain?, targetAccountId?, loginMethod?}`; `loginMethod == "browser"` → 400 «browser login недоступен в web-режиме: используйте device code»), `POST /api/auth/:provider/poll` (`{deviceCode, githubDomain?}`), `POST /api/auth/:provider/cancel` (`{deviceCode}`), `GET /api/auth/:provider/accounts`, `GET /api/auth/:provider/status`, `DELETE /api/auth/:provider/accounts/:accountId`, `PUT /api/auth/:provider/default` (`{accountId}`), `POST /api/auth/:provider/logout`, `GET /api/auth/agy/running`.
- [x] `/api/capabilities` отдаёт `platform: std::env::consts::OS`.
- [x] `src-tauri/tests/web_api_auth.rs`: подготовленные `copilot_auth.json`/`xai_oauth_auth.json`/`codex_oauth_auth.json` в temp config dir → accounts/status их показывают; set default меняет `default_account_id`; remove удаляет аккаунт и запись в файле; logout очищает; login `browser` → 400; неизвестный provider → 400; cancel для `github_copilot` → `{ok:false}` (как на desktop). Device-flow против заглушки — если у менеджеров есть seam базового URL; иначе end-to-end вручную на agentbox (Copilot) с записью в Notes.

### Task 3: frontend
- [x] adapter: 9 case; `copy_text_to_clipboard` — локально: `navigator.clipboard.writeText` при `window.isSecureContext`, иначе скрытая `<textarea>` + `document.execCommand("copy")`; неудача → throw (UI оставляет код видимым для ручного копирования).
- [x] `useManagedAuth` в web: `loginMethod` всегда `device_code`, выбор browser скрыт; `verification_uri` открывается через `open_external` (фаза 1). Antigravity при `platform != "macos"` — секция входа заменена текстом «вход Antigravity требует macOS keychain на сервере».
- [x] Тесты: adapter clipboard (secure context → `writeText`; иначе `execCommand`); `useManagedAuth` web — start шлёт `device_code`, poll до аккаунта; `AgyOAuthSection` на linux-сервере показывает пояснение.
- [x] Снять WEB-00-гейты с Auth Center для Copilot/Codex/xAI (таблица фазы 6); Agy на Linux → «desktop-only (macOS keychain)».

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`; вход Copilot device code end-to-end на agentbox.
- [x] Ветка `web-parity/web-02-oauth-device`, PR; DX-60 остаётся In Progress, комментарий со ссылкой на PR, «этап 1 из 2» и решением по browser login.

Риски для ревью:
- Poll Codex берёт `lock_switch_for_app("codex")`: долгий poll из web не должен блокировать переключение провайдера Codex дольше, чем на desktop.
- Токены в `*_auth.json` выдаются только через статусные DTO, которые desktop отдаёт фронту; новые роуты не должны отдавать access/refresh token (тест: в JSON ответа нет полей `token`).

## Notes for next phase

- **Статус до merge (2026-10-29)**: ветка `web-parity/web-02-oauth-device` (worktree `.worktrees/web-02`), PR #26 (https://github.com/alxwlw/cc-switch-web/pull/26) открыт. DX-60 переведена из Backlog в In Progress; там комментарий со ссылкой, «этап 1 из 2» и решением по browser login. Ветка также несёт закрытие фазы 24 в плане (`5d0b3a9a`).
  - Финальные гейты на `3b7310eb`:
    - fmt и `clippy --lib --examples -D warnings` — чисто;
    - `(umask 022; cargo test --no-fail-fast)` — 4481 passed / 1 failed (baseline omp_search_paths);
    - `cargo build --release --example server` — ок;
    - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` — ок;
    - `test:unit` — 3886 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
    - `266e3397` (только текст `web.accountLogin` в 5 локалях) проверен затронутыми тестами, `format:check` и `build:web`.
  - Ревью: Т1 и Т2 одобрены с r0, без замечаний. Т3 — r0 changes_requested: SHOULD-FIX DP-020 (устаревшие комментарии «auth скрыт в web» в `Sidebar.tsx`, `navigation.ts`, `AuthSettingsPanel.tsx` и `phase-6.md`) и два NIT; fix round 1 одобрен. Решения — в journal p25.
- **Отличия от плана**:
  - Вход Antigravity в web закрыт на любой ОС сервера (login `agy_oauth` → 400), а не только на Linux. Причина — loopback callback на сервере.
  - Antigravity показывается по `platform` сервера: хук `useAgyAccountsAvailable()` (`src/hooks/useServerCapabilities.ts:22`) в desktop отдаёт `isMac()`, в web — `platform === "macos"`. Он применён в `AuthCenterPanel`, `ProviderForm` и `ProviderCard`. На не-macOS сервере карточки Agy нет, как в desktop на Linux. Пояснение «требует macOS keychain» из плана не понадобилось. На macOS-сервере кнопку входа заменяет строка `authCenter.agy.webLoginUnavailable`, а list/default/remove/logout работают.
  - Kimi остаётся закрытым до фазы 26: в web Accounts вместо его карточки `WebAccountLoginNotice`. Это единственный пользователь компонента; текст `web.accountLogin` теперь называет Kimi Code (`266e3397`, найдено в живой проверке: общий текст читался так, будто недоступен любой вход).
  - Seam базового URL у менеджеров нет, поэтому device flow не тестируется против заглушки; он проверен вручную (ниже).
- **Backend**:
  - Поля `AppState`: `store.rs:22-26`. `managed_auth_states()` — `:56`, возвращает struct `ManagedAuthStates` с четырьмя обёртками-клонами. Desktop `.setup` — `lib.rs:866`.
  - `AppState::new` в production вне setup есть ещё в `import_export.rs:99` (`sync_current_providers_live_core`): там создаётся временный state с лишними экземплярами менеджеров. Он короткоживущий и токенов не трогает, оставлен как был.
  - inner-функции: `commands/auth.rs:143-603`. `ensure_auth_provider` и `AUTH_PROVIDER_AGY_OAUTH` стали `pub(crate)`.
  - Роуты: `web_api.rs:273-285`, хендлеры — `:2305-2430`. `/api/auth/agy/running` уживается рядом с `:provider` (matchit 0.7). Коды ответов: неизвестный provider, `loginMethod: browser`, login `agy_oauth` и cancel для провайдеров, кроме codex/agy, → 400; прочие ошибки inner → 500. Например, отказ «нет подписки Copilot» при poll → 500 с текстом менеджера (китайский, как в desktop).
  - Тесты: `src-tauri/tests/web_api_auth.rs` (7, отдельный бинарь, `#[serial]`). Файлы токенов кладутся до сборки router: менеджеры читают их в `new()`.
- **Frontend**:
  - Case в `adapter.ts:173-213`, базовый путь — `:46`. `copy_text_to_clipboard` обрабатывается до `mapCommand` (`:1207`), реализация — `copyTextInBrowser` (`:1042`).
  - `useManagedAuth` в web всегда шлёт `device_code`; выбор способа входа в `CodexOAuthSection` в web скрыт.
  - `WEB_HIDDEN_VIEWS` пуст (`navigation.ts:66`), механизм оставлен.
  - Тесты: `tests/lib/authWebApi.test.ts`, `tests/components/AuthCenterPanelWeb.test.tsx`, web-кейсы в `tests/hooks/useManagedAuth.test.tsx`, обновлены gating-тесты.
- **Живая проверка** (сборка ветки, копия БД, временный HOME):
  - На `127.0.0.1:18799`: Accounts открыт, карточки Copilot/ChatGPT/xAI с входом, Agy нет (linux), выбора способа входа Codex нет. После перезагрузки view Accounts восстанавливается.
  - Copilot device code с владельцем:
    - Код выдан, вкладка github.com/login/device открылась.
    - Poll получил OAuth-токен и пользователя `alxwlw`.
    - Затем «用户未订阅 Copilot»: у аккаунта нет подписки. UI показал «Sign-in didn't finish» с причиной, файл токенов не создан.
    - Полный вход с сохранением аккаунта требует аккаунта с подпиской Copilot.
  - Подложенные аккаунты `octo-a`/`octo-b`: «Set as default» и удаление с подтверждением меняют UI и `copilot_auth.json`. `POST …/logout` удаляет файл. С одним аккаунтом меню карточки «выйти из всех» не показывается — так же и в desktop.
  - На `http://100.123.155.62:18799` (`isSecureContext: false`, `navigator.clipboard` нет): код скопирован через `execCommand` дважды (автокопия при старте и кнопка), textarea удалена.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет.
  - Smoke:
    - `GET /api/capabilities` → `platform: "linux"`;
    - `GET /api/auth/github_copilot/status` → пустой список (в `~/.cc-switch-web` нет файлов токенов);
    - `POST /api/auth/codex_oauth/login {"loginMethod":"browser"}` → 400;
    - `GET /api/auth/agy/running` → `false`.
  - Реальный вход на живом сервисе не запускать без владельца: аккаунт сразу станет доступен всем пресетам.
  - DX-60 остаётся In Progress (этап 2 — фаза 26).
- **Для фазы 26 (Kimi, official cards)**: `KimiOAuthManager::default()` ещё в `lib.rs:858`. Его — в `AppState` по образцу T1 (поле, клон в `managed_auth_states`). Kimi-гейты: `src/components/providers/forms/hooks/useKimiOAuth.ts:25`, `src/components/KimiAccountQuotaFooter.tsx:49`, заглушка в `AuthCenterPanel.tsx:85` (`WebAccountLoginNotice` — после фазы 26 у него не останется пользователей). Квоты аккаунтов (`get_codex_oauth_quota`, `copilot_get_usage*`, `get_xai_oauth_quota`, `agy_account_quota`) в web всё ещё не замаплены — в Accounts на подложенных аккаунтах строк квот не было.
