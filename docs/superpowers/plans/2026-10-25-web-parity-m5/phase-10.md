# Phase 10 — WEB-06 / DX-64: Project Profiles

Цель: list/create/rename/resnapshot/delete/apply/clear профилей работают в web. После apply фронт получает те же события, что на desktop (`provider-switched` по каждому приложению scope и `profile-applied`) — через event bus фазы 8.

Спека: § WEB-06. Оценка M (2–3 дн.).

Scope: `src-tauri/src/commands/profile.rs` (DTO и сборка ответа → сервис, emit → bus), `src-tauri/src/services/profile.rs`, `src-tauri/src/web_api.rs` (6 роутов), `src-tauri/tests/web_api_profiles.rs`, `src/lib/api/adapter.ts` (6 case), тесты фронта.

Факты разведки:
- Фронт profiles.ts:57-108: `list_profiles`, `create_profile {name, scope}`, `update_profile {id, name?, resnapshot?, scope?}` (rename и resnapshot — один вызов), `delete_profile {id}`, `apply_profile {id, scope}` → `string[]` warnings, `clear_current_profile {scope}`; scope — `"claude" | "claude-desktop" | "codex"` (`ProfileScope::parse`, services/profile.rs:56).
- Хуки `src/lib/query/profiles.ts`: query `["profiles"]` (:17-20); create/update/delete/clear инвалидируют `["profiles"]` + `updateTrayMenu` (:26-110); apply (:114-140) инвалидирует `["profiles"]`, `["providers","claude"]`, `["providers","claude-desktop"]`, `["providers","codex"]`, `["mcp","all"]`, `["skills"]`. Слушатель `App.tsx:482-493` (`profile-applied`) дополнительно инвалидирует `proxyKeys.takeoverStatus` и `proxyKeys.status`; `PromptPanel.tsx:182`, `PiPromptPanel.tsx:134` перезагружаются по `profile-applied`.
- UI: `ProfileSwitcher.tsx:71-74`, `ProfileManageDialog.tsx:43-45`.
- Backend profile.rs: `list_profiles` :63 (собирает `current_ids` inline через `state.db.get_current_profile_id`), `create_profile` :82, `update_profile` :95, `delete_profile` :112, `clear_current_profile` :117 (`state.db.set_current_profile_id(scope.as_str(), None)`), `apply_profile(app: AppHandle, State<AppState>, id, scope)` :132-143. Сервис handle-free: `ProfileService::list` :245, `create` :251, `update` :274, `delete` :309, `apply(&AppState,&str,ProfileScope) -> Result<Vec<String>>` :334.
- `emit_profile_apply_events(app,&state,id,scope)` (profile.rs:~63-95): `provider-switched` по каждому приложению scope (payload `appType, proxyEnabled, autoFailoverEnabled, providerId` через `get_proxy_flags_sync`, `mode::current::is_proxy`, `provider_for`), затем `profile-applied {profileId, scope}`, затем `tray::refresh_tray_menu(app)`. Та же функция вызывается из `tray.rs:2872`.
- ⚠️ Doc-комментарий profile.rs:~125-130: apply обязан быть sync — `ProviderService::switch` использует `block_on` и паникует в потоке runtime. Web-хендлер — строго `spawn_blocking`.
- DTO `ProfileDto`, `CurrentProfileIds`, `ProfilesResponse` (profile.rs:9-55) — camelCase, живут в commands-слое.
- Тесты: `src-tauri/tests/profile_roundtrip.rs` (7 тестов), emit не покрыт; TS-тестов профилей не найдено.

### Task 1: вынос из commands-слоя
- [ ] DTO и сборку `ProfilesResponse` (с `current_ids`) перенести в `services/profile.rs` (`ProfileService::list_response(&AppState) -> Result<ProfilesResponse, AppError>`); команда `list_profiles` вызывает её.
- [ ] `emit_profile_apply_events` разделить: `profile_apply_events(&AppState, id, scope) -> Vec<(String, Value)>` (handle-free, вычисляет payload) + emit через `state.events`; tray-refresh остаётся в desktop-обёртке и в `tray.rs:2872`. `profile_roundtrip.rs` зелёный.

### Task 2: backend — роуты + тесты
- [ ] `GET /api/profiles`, `POST /api/profiles` (`{name, scope}`), `PATCH /api/profiles/:id` (`{name?, resnapshot?, scope?}`), `DELETE /api/profiles/:id`, `POST /api/profiles/:id/apply` (`{scope}`; `spawn_blocking`, затем emit событий через bus), `POST /api/profiles/clear-current` (`{scope}`). Неизвестный scope → 400.
- [ ] `src-tauri/tests/web_api_profiles.rs`: create → list (`currentIds` пуст); apply → warnings, current provider приложений scope сменился, live-файл в temp HOME обновлён, bus выдал `provider-switched` по каждому приложению scope и затем `profile-applied` (порядок как на desktop); resnapshot перезаписывает снимок; clear → `currentIds[scope] = null`; apply из async-контекста не паникует.

### Task 3: frontend
- [ ] 6 case в `mapCommand`.
- [ ] `tests/lib/profilesApi.test.ts`: URL/метод/body каждой функции. Тест на инвалидацию `proxyKeys.*` после SSE-события `profile-applied` (mock EventSource фазы 8).
- [ ] Снять WEB-00-гейт с `ProfileSwitcher`/`ProfileManageDialog` (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-06-profiles`, PR; DX-64 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Apply профиля, когда приложение scope в proxy takeover: `provider-switched` должен нести `proxyEnabled: true`, иначе фронт покажет direct-режим.

## Notes for next phase

- (заполнить по итогам)
