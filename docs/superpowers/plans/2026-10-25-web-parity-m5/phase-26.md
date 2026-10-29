# Phase 26 — WEB-02 / DX-60 (этап 2): Kimi OAuth и official account cards

Цель: Kimi OAuth (статус, device-code вход, poll, cancel, удаление, квота, модели, выбор аккаунта) работает в web, а карточки официальных аккаунтов (Kimi, Codex managed account) показывают живые данные. DX-60 закрывается этой фазой.

Спека: § WEB-02 (остаток L после фазы 25).

⚠️ Уточнение к спеке: отдельного компонента «official account card» в коде нет. Это концепция карточки провайдера: `isOfficialAccount(appId, provider)` (`src/utils/providerCapabilities`, используется в `ProviderCard.tsx:121` для `appId==="kimi"` и `ModeDialog.tsx:164`) плюс Codex `resolveCodexOfficialIdentity(...) === "managed_account"` (`ProviderCard.tsx:195-220`). Данные приходят из `auth_get_status("codex_oauth")` (фаза 25) и Kimi-команд этой фазы.

Scope: `src-tauri/src/store.rs` (Kimi-менеджер), `src-tauri/src/lib.rs:1273`, `src-tauri/src/commands/kimi_oauth.rs`, `src-tauri/src/web_api.rs` (8 роутов), `src-tauri/tests/web_api_kimi.rs`, `src/lib/api/adapter.ts` (8 case), тесты фронта.

Факты разведки:
- Фронт kimiOAuth.ts:35-58: `kimi_oauth_status`, `kimi_oauth_start {region, accountId|null}`, `kimi_oauth_poll {flowId}`, `kimi_oauth_cancel {flowId}`, `kimi_oauth_remove {accountId}`, `kimi_oauth_quota {accountId}`, `kimi_oauth_models {accountId}`, `kimi_oauth_use_account {accountId}`.
- Вызовы: `hooks/useKimiOAuth.ts` (start :140, poll :92, cancel :48, remove :176, use :188, open_external :150); `KimiAccountQuotaFooter.tsx:45`; `lib/query/subscription.ts:258` (quota); `ProviderCard.tsx:121,206`.
- Backend commands/kimi_oauth.rs:9-79 — 8 команд, каждая `State<KimiOAuthManager>`; `kimi_oauth_remove` (l.43) ещё берёт `State<AppState>` и вызывает `db.delete_provider("kimi", id)`. Методы менеджера (services/kimi_oauth/mod.rs): `status` :647, `quota` :654, `models` :662 (таймаут 30 с, :495), `start` :666, `use_account` :945, а также poll/cancel/remove. AppHandle нет.
- `KimiOAuthManager::default()` (lib.rs:1273) — managed как сырой тип, `#[derive(Default)]` (mod.rs:163), `store: None`; каталог — `kimi_config::data_dir()` (kimi_config.rs:89: `$KIMI_CODE_HOME` или `~/.kimi-code`), учётные данные — `credential_path(home, oauth)` (mod.rs:170).
- Вход — device code: `/api/oauth/device_authorization` (mod.rs:~696), `user_code` + `verification_uri_complete`/`verification_uri` (mod.rs:712-716); слушателя нет.
- DTO `KimiOAuthStatus`, `KimiOAuthAccount`, `KimiLoginStart`, `KimiLoginPoll` (mod.rs:75-117) — camelCase; `KimiRegion` — kebab-case (mod.rs:26).
- Тесты: 21 inline в kimi_oauth/mod.rs; фронт — `KimiAccountLogin`, `KimiAccountQuotaFooter`, `KimiAddProviderDialog`, `KimiProviderForm` (`tests/components/`).

### Task 1: Kimi-менеджер в AppState
- [x] `AppState.kimi_oauth: Arc<KimiOAuthManager>`; lib.rs: `app.manage(KimiOAuthState(app_state.kimi_oauth.clone()))`; команды kimi_oauth.rs переходят на `State<AppState>` (или `State<KimiOAuthState>`) — тот же экземпляр. Inline-тесты зелёные.

### Task 2: backend — роуты + тесты
- [x] `GET /api/kimi/oauth/status`, `POST /api/kimi/oauth/start` (`{region, accountId}`), `POST /api/kimi/oauth/poll` (`{flowId}`), `POST /api/kimi/oauth/cancel` (`{flowId}`), `DELETE /api/kimi/oauth/accounts/:accountId` (с `db.delete_provider("kimi", id)`, как в команде), `GET /api/kimi/oauth/accounts/:accountId/quota`, `GET /api/kimi/oauth/accounts/:accountId/models`, `POST /api/kimi/oauth/accounts/:accountId/use`.
- [x] `src-tauri/tests/web_api_kimi.rs` (`KIMI_CODE_HOME` → temp): подготовленные учётные данные → status показывает аккаунт; use-account меняет активный; remove удаляет учётные данные и kimi-провайдера из DB; cancel неизвестного flowId → `{ok:false}`; неизвестный region → 400. Start/poll/quota/models против заглушки — если есть seam базового URL; иначе end-to-end вручную на agentbox с записью в Notes.

### Task 3: frontend
- [x] 8 case в `mapCommand`.
- [x] Device-code UX Kimi в web — как в фазе 25: `verification_uri_complete` через `open_external`, код копируется adapter-clipboard-ом и остаётся видимым.
- [x] Карточки: Kimi (`isOfficialAccount`) и Codex managed account в web показывают статус, квоту и модели; если квота недоступна — текст ошибки сервера, а не пустая карточка.
- [x] Тесты: `tests/lib/kimiOAuthApi.test.ts`; `KimiAccountQuotaFooter` в web рендерит квоту из adapter; существующие Kimi-тесты зелёные.
- [x] Снять WEB-00-гейты Kimi и карточек (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`; вход Kimi end-to-end на agentbox.
- [x] Ветка `web-parity/web-02-kimi-cards`, PR; DX-60 → In Review + ссылка («этап 2 из 2»). Закрыть чекбоксы, Notes.

Риски для ревью:
- Kimi использует `~/.kimi-code` пользователя сервера — это те же учётные данные, что у Kimi CLI на сервере. Remove из web разлогинит и CLI: подтверждение в UI должно это сказать.

## Notes for next phase

- **Закрыта 2026-10-29**: PR #27 merged `acb117d8` → main; live на `cc-switch-web.service` с 14:19; DX-60 Done (этап 2 из 2).
  - Server и `dist-web` пересобраны из main. Перед restart, при остановленном unit, БД скопирована в `~/.cc-switch-web/backups-predeploy-p26-20261029-141917/`, прежний бинарник — `cc-switch-web.prev`. Миграций нет.
  - Smoke: консоль 200; `GET /api/kimi/oauth/status` → пустой список; `start {"region":"mars"}` → 400; `cancel {"flowId":"x"}` → `data: false`. Реальный вход Kimi на живом сервисе не запускали.
- **Статус до merge (2026-10-29)**: ветка `web-parity/web-02-kimi-cards` (worktree `.worktrees/web-02-kimi`), PR #27 (https://github.com/alxwlw/cc-switch-web/pull/27) открыт; DX-60 → In Review (комментарий со ссылкой, «этап 2 из 2»). Ветка также несёт закрытие фазы 25 в плане (`61087808`).
  - Финальные гейты на `754aec02`:
    - fmt и `clippy --lib --examples -D warnings` — чисто;
    - `(umask 022; cargo test --no-fail-fast)` — 4489 passed / 1 failed (baseline omp_search_paths);
    - `cargo build --release --example server` — ок;
    - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` — ок;
    - `test:unit` — 3896 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - Ревью: Т1 и Т2 одобрены с r0 (NIT-ы: тавтологичный `Arc::ptr_eq` в `store.rs` оставлен; route-тест удаления primary добавлен в Т3). Т3 — r0 changes_requested: SHOULD-FIX DP-019 (строки `phase-6.md:167,169` про удалённый `web.accountLogin`) и NIT DP-050 (секунда реального ожидания retry в тесте квоты); fix round 1 одобрен. Решения — в journal p26.
- **Отличия от плана**:
  - Квота карточки Codex managed account в web — не здесь, а в фазе 31 (роут `GET /api/subscription/codex-oauth/:accountId/quota` в её плане). Карточка Codex показывает статус через `auth_get_status` с фазы 25, гейтов на ней не было.
  - Обёртки `KimiOAuthState` нет: кроме 8 команд, Tauri-state Kimi никто не читал. Команды берут `State<AppState>`.
  - Ответ cancel неизвестного flow в web — `{ok:true, data:false}` (паритет с desktop `Ok(false)`); «`{ok:false}`» в плане означало `data: false`.
  - Device-code UX Kimi менять не пришлось: общий блок уже копирует код через `copyText`, а ссылка открывается через `open_external`. Автокопирования кода при старте у Kimi нет и в desktop.
- **Backend**: поле `AppState.kimi_oauth` — `store.rs:29` (создаётся в `:53`). `kimi_oauth_remove_inner` — `commands/kimi_oauth.rs:47`. Роуты — `web_api.rs:287-302`, хендлеры — `:2424-2497`; тела разбираются из `Json<Value>`, чтобы неверный `region`/`flowId` давал 400, а не 422 axum. Пустое тело или чужой content-type получают ответ axum (415/400) без конверта — так же у прочих Json-роутов. Тесты — `src-tauri/tests/web_api_kimi.rs` (7): просроченный access и refresh token позволяют проверить status без сети (`identify_accounts` ходит в сеть только с живым access token).
- **Frontend**: case — `adapter.ts:216-255`. Строка о CLI на сервере — `kimi.oauth.removeConfirmWeb` (en `:4994`), показывается в `KimiAccountLogin.tsx:192` только в web и только для не-primary. `WebAccountLoginNotice` и `web.accountLogin` удалены. Тесты: `tests/lib/kimiOAuthApi.test.ts`, web-кейсы в `KimiAccountQuotaFooter.test.tsx` и `KimiAccountLogin.test.tsx`, обновлены `AuthCenterPanelWeb`, `webSilencedQueries`, `WebProviderGating`.
- **Живая проверка** (release-сборка ветки, `127.0.0.1:18799`, копия БД, временные `HOME` и `KIMI_CODE_HOME`):
  - API против настоящего Kimi: start `global` → `kimi.ai/code/authorize_device`, `mainland-cn` → `kimi.com/code/…`; poll → ожидание; cancel → `true`, повторно → `false`; poll после отмены → `ended: "cancelled"`; `region: "mars"` → 400.
  - Chromium: карточка Kimi Code в Accounts; регион → код на экране, `window.open(…, "noopener,noreferrer")`, «Copy code» копирует; Cancel шлёт `/cancel`.
  - Подложенные аккаунты: квота «Quota unavailable · Sign-in expired» в Accounts и на карточках провайдеров Kimi; модели → 500 с текстом менеджера; удаление с web-строкой о CLI убирает учётные данные и узел в `config.toml`; use переключает `default_model`.
  - Ловушка: запрос квоты пытается обновить токен, и при отказе сервера Kimi (фиктивный refresh) менеджер обнуляет файл учётных данных. После этого status честно показывает `authenticated: false`, а use отвечает «Sign in to Kimi Code first». Поведение desktop, фазой не менялось. Для проверки use подкладывать учётные данные заново при закрытой странице.
  - У владельца нет аккаунта Kimi: полный вход, квота и модели на настоящем аккаунте end-to-end не проверены.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет.
  - Smoke: `GET /api/kimi/oauth/status` → пустой список (у пользователя сервера нет `~/.kimi-code`); `POST /api/kimi/oauth/start {"region":"mars"}` → 400; `POST /api/kimi/oauth/cancel {"flowId":"x"}` → `data: false`.
  - Реальный вход Kimi на живом сервисе запишет учётные данные в `~/.kimi-code` пользователя сервера — без владельца не запускать.
  - DX-60 → Done (этап 2 из 2).
- **Для следующих фаз**: квоты подписок Codex/xAI/Antigravity/CLI и `copilot_get_usage*` в web по-прежнему не замаплены — фаза 31.
