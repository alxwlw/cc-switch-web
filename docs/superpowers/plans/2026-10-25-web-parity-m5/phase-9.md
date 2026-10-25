# Phase 9 — WEB-07 / DX-65: Universal Providers CRUD, duplicate и sync

Цель: список, создание, редактирование, дублирование, удаление и синхронизация универсальных провайдеров в приложения работают в web. Частично неудачная синхронизация (DB записана, live-файл не записан) честно видна пользователю.

Спека: § WEB-07. Оценка M (2–3 дн.).

Scope: `src-tauri/src/commands/provider.rs:955-1028` (emit → bus), `src-tauri/src/web_api.rs` (5 роутов), `src-tauri/tests/web_api_universal.rs`, `src/lib/api/adapter.ts` (5 case), `src/components/universal/UniversalProviderPanel.tsx` (обработка частичной ошибки), тесты фронта.

Факты разведки:
- Фронт providers.ts:283-318: `get_universal_providers`, `get_universal_provider {id}`, `upsert_universal_provider {provider}`, `delete_universal_provider {id}`, `sync_universal_provider {id}`. Duplicate — это upsert с новым id + sync на фронте (`UniversalProviderPanel.tsx:179-180`), отдельной команды нет.
- UI: `UniversalProviderPanel.tsx:36` (getAll), `:58,:62` (create: upsert, затем sync только если `!editingProvider`), `:92-93` (save + sync), `:118` (delete), `:140` (sync); `AddProviderDialog.tsx:205` (upsert), `:220` (sync). Панель держит локальный `useState` и `loadProviders()` (`:34-50`) — React Query-ключа для универсальных провайдеров нет.
- Backend: `get_universal_providers` (:969), `get_universal_provider` (:976) — только `State<AppState>`; `upsert_universal_provider` (:984), `delete_universal_provider` (:999), `sync_universal_provider` (:1013) — `AppHandle` + `State<AppState>`, AppHandle нужен только для `emit_universal_provider_synced` (:955-967), событие `universal-provider-synced {action,id}`.
- Сервисы handle-free (`services/provider/mod.rs`): `list_universal` :8369, `get_universal` :8376, `upsert_universal(&AppState, UniversalProvider) -> Result<bool>` :8384, `delete_universal` :8396 (удаляет и дочерние `universal-{claude|codex|gemini}-{id}` в DB), `sync_universal_to_apps` :8423.
- Частичный отказ: `sync_universal_to_apps` сохраняет дочерние провайдеры в DB, потом пишет live-файлы; ошибки по приложениям собираются и возвращаются `Err("统一供应商已保存到数据库，但以下应用的配置文件未能写入…")` (:8509-8516), emit в этом случае пропускается. `AddProviderDialog.tsx:208-232` различает отказ upsert и sync (`toast.warning "addedButSyncFailed"`); `UniversalProviderPanel.tsx:56-75,:92-93,:179-180` держит upsert и sync в одном try/catch с общим toast и не вызывает `loadProviders()` при ошибке.
- DTO `UniversalProvider` (provider.rs:774) — без `rename_all`, поштучные rename (`providerType`, `baseUrl`, `apiKey`).
- `App.tsx:471-478` слушает `universal-provider-synced` → инвалидирует `["providers"]` и `updateTrayMenu`; после фазы 8 приходит через SSE.
- Тестов на универсальных провайдеров нет ни в Rust, ни во фронте.

### Task 1: emit через bus
- [ ] `emit_universal_provider_synced` переписать на `state.events.emit("universal-provider-synced", {action,id})`; desktop получает событие через мост фазы 8. Команды upsert/delete/sync перестают требовать `AppHandle`, если он больше ни для чего не нужен.

### Task 2: backend — 5 роутов + тесты
- [ ] `GET /api/universal-providers`, `GET /api/universal-providers/:id`, `PUT /api/universal-providers` (body `{provider}`), `DELETE /api/universal-providers/:id`, `POST /api/universal-providers/:id/sync`; блокирующее — в `spawn_blocking`; после успешных upsert/delete/sync — emit через bus, как в команде.
- [ ] `src-tauri/tests/web_api_universal.rs`: upsert → list содержит; sync → дочерние провайдеры в DB для каждого включённого приложения и live-файлы в temp HOME обновлены; delete удаляет дочерних; sync при недоступном для записи live-файле одного приложения → `{ok:false}` с текстом ошибки сервиса, а дочерний провайдер в DB при этом есть; bus выдаёт событие только на успехе.

### Task 3: frontend
- [ ] 5 case в `mapCommand`.
- [ ] `UniversalProviderPanel`: разделить upsert и sync, как в `AddProviderDialog` — upsert упал → ошибка; sync упал → `toast.warning` с текстом сервера и всё равно `loadProviders()`, чтобы панель показала сохранённое.
- [ ] Тесты: `tests/lib/universalProvidersApi.test.ts` (URL/метод/body); `tests/components/UniversalProviderPanel.test.tsx` — частичный отказ sync даёт warning и перезагрузку списка; duplicate = upsert нового id + sync.
- [ ] Снять WEB-00-гейт с универсальных провайдеров (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-07-universal-providers`, PR; DX-65 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Удаление универсального провайдера, чей дочерний провайдер сейчас активен в приложении: поведение должно совпадать с desktop (тест сверяет current provider после delete).

## Notes for next phase

- (заполнить по итогам)
