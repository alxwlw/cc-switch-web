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
- [ ] `AppState.kimi_oauth: Arc<KimiOAuthManager>`; lib.rs: `app.manage(KimiOAuthState(app_state.kimi_oauth.clone()))`; команды kimi_oauth.rs переходят на `State<AppState>` (или `State<KimiOAuthState>`) — тот же экземпляр. Inline-тесты зелёные.

### Task 2: backend — роуты + тесты
- [ ] `GET /api/kimi/oauth/status`, `POST /api/kimi/oauth/start` (`{region, accountId}`), `POST /api/kimi/oauth/poll` (`{flowId}`), `POST /api/kimi/oauth/cancel` (`{flowId}`), `DELETE /api/kimi/oauth/accounts/:accountId` (с `db.delete_provider("kimi", id)`, как в команде), `GET /api/kimi/oauth/accounts/:accountId/quota`, `GET /api/kimi/oauth/accounts/:accountId/models`, `POST /api/kimi/oauth/accounts/:accountId/use`.
- [ ] `src-tauri/tests/web_api_kimi.rs` (`KIMI_CODE_HOME` → temp): подготовленные учётные данные → status показывает аккаунт; use-account меняет активный; remove удаляет учётные данные и kimi-провайдера из DB; cancel неизвестного flowId → `{ok:false}`; неизвестный region → 400. Start/poll/quota/models против заглушки — если есть seam базового URL; иначе end-to-end вручную на agentbox с записью в Notes.

### Task 3: frontend
- [ ] 8 case в `mapCommand`.
- [ ] Device-code UX Kimi в web — как в фазе 25: `verification_uri_complete` через `open_external`, код копируется adapter-clipboard-ом и остаётся видимым.
- [ ] Карточки: Kimi (`isOfficialAccount`) и Codex managed account в web показывают статус, квоту и модели; если квота недоступна — текст ошибки сервера, а не пустая карточка.
- [ ] Тесты: `tests/lib/kimiOAuthApi.test.ts`; `KimiAccountQuotaFooter` в web рендерит квоту из adapter; существующие Kimi-тесты зелёные.
- [ ] Снять WEB-00-гейты Kimi и карточек (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; вход Kimi end-to-end на agentbox.
- [ ] Ветка `web-parity/web-02-kimi-cards`, PR; DX-60 → In Review + ссылка («этап 2 из 2»). Закрыть чекбоксы, Notes.

Риски для ревью:
- Kimi использует `~/.kimi-code` пользователя сервера — это те же учётные данные, что у Kimi CLI на сервере. Remove из web разлогинит и CLI: подтверждение в UI должно это сказать.

## Notes for next phase

- (заполнить по итогам)
