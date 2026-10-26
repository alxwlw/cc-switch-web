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
- [x] `emit_universal_provider_synced` переписать на `state.events.emit("universal-provider-synced", {action,id})`; desktop получает событие через мост фазы 8. Команды upsert/delete/sync перестают требовать `AppHandle`, если он больше ни для чего не нужен.

### Task 2: backend — 5 роутов + тесты
- [x] `GET /api/universal-providers`, `GET /api/universal-providers/:id`, `PUT /api/universal-providers` (body `{provider}`), `DELETE /api/universal-providers/:id`, `POST /api/universal-providers/:id/sync`; блокирующее — в `spawn_blocking`; после успешных upsert/delete/sync — emit через bus, как в команде.
- [x] `src-tauri/tests/web_api_universal.rs`: upsert → list содержит; sync → дочерние провайдеры в DB для каждого включённого приложения и live-файлы в temp HOME обновлены; delete удаляет дочерних; sync при недоступном для записи live-файле одного приложения → `{ok:false}` с текстом ошибки сервиса, а дочерний провайдер в DB при этом есть; bus выдаёт событие только на успехе.

### Task 3: frontend
- [x] 5 case в `mapCommand`.
- [x] `UniversalProviderPanel`: разделить upsert и sync, как в `AddProviderDialog` — upsert упал → ошибка; sync упал → `toast.warning` с текстом сервера и всё равно `loadProviders()`, чтобы панель показала сохранённое.
- [x] Тесты: `tests/lib/universalProvidersApi.test.ts` (URL/метод/body); `tests/components/UniversalProviderPanel.test.tsx` — частичный отказ sync даёт warning и перезагрузку списка; duplicate = upsert нового id + sync.
- [x] Снять WEB-00-гейт с универсальных провайдеров (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-07-universal-providers`, PR; DX-65 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Удаление универсального провайдера, чей дочерний провайдер сейчас активен в приложении: поведение должно совпадать с desktop (тест сверяет current provider после delete).

## Notes for next phase

- **Статус до merge (2026-10-26)**: ветка `web-parity/web-07-universal-providers` (worktree `.worktrees/web-07`), PR #10 (https://github.com/alxwlw/cc-switch-web/pull/10) открыт, DX-65 In Review (комментарий со ссылкой). Задачи 1–3 прошли implement→review, все одобрены с r0 (fix round не понадобился); NIT ревью T3 (DP-011 название теста правки, DP-021 комментарий) поправил контроллер в `9842f41`; T4 — контроллер. Финальные гейты на `9842f41`: fmt/clippy чисто, `cargo build --release --example server` ок, cargo test 4298 passed / 3 failed (baseline: omp_search_paths, 2× file_modes::switch_*); `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок, `test:unit` 3673 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice). В PR едет неподтолкнутый `e6201d51` (docs закрытия фазы 8 с локального main). После merge — `post-merge`: пересобрать server и `dist-web` (Rust менялся), `cp cc-switch-web cc-switch-web.prev` перед install; миграций БД нет; DX-65 → Done.
- **Backend API** (`commands/provider.rs`): `pub fn upsert_universal_provider_emit / delete_universal_provider_emit / sync_universal_provider_emit(&AppState, …) -> Result<bool, AppError>` — вызывают сервис и эмитят `universal-provider-synced {action,id}` в `state.events` только на успехе. Tauri-команды — тонкие обёртки без `AppHandle`. `UniversalProvider` реэкспортирован из корня crate (нужен интеграционным тестам).
- **Роуты** (`web_api.rs`): `GET /api/universal-providers`, `GET /api/universal-providers/:id` (нет id → `data: null`), `PUT /api/universal-providers` (body `{provider}`), `DELETE /api/universal-providers/:id`, `POST /api/universal-providers/:id/sync`; всё в `spawn_blocking`. Частичный sync → 500 `{ok:false,error}` с текстом сервиса дословно; adapter бросает `payload.error` при любом `ok !== true`, статус не важен.
- **Поведение сервиса, которое стоит знать** (пинится тестами `tests/web_api_universal.rs`):
  - `sync_universal_to_apps` переписывает live-файл приложения только если дочерний `universal-<app>-<id>` — текущий провайдер этого приложения; иначе пишет только DB. Тест частичного отказа поэтому делает дочерних current и подменяет `~/.claude/settings.json` каталогом.
  - `delete_universal` (`services/provider/mod.rs:8396`) всегда `Ok(true)` — ветки not-found нет; отказ delete в тесте вызывается SQLite-триггером.
  - Удаление универсального провайдера, чей дочерний активен: дочерний удаляется, у приложения current = `None`, live-файл не трогается. Так же в desktop (тест сверяет оба пути). Продуктовый вопрос — не меняли.
- **Frontend**: `UniversalProviderPanel` гоняет create (`handleSave` без `editingProvider`), `handleSaveAndSync` и duplicate через один `upsertThenSync`: upsert упал → `toast.error`, sync не вызывается; sync упал → `toast.warning` (`universalProvider.addedButSyncFailed`, текст сервера в `description`) и `loadProviders()`. Кнопка sync: ошибка с текстом сервера, список не перезагружается (sync пишет только дочерних, сами универсальные записи не меняет). Правка — только upsert, как было. Реальная `UniversalProviderFormModal` закрывается через `onClose` сразу после `onSave`/`onSaveAndSync` при любом исходе (`:229-230`, `:316-319`) — возвращаемый `ok` влияет только на `editingProvider` панели.
- **Пробел (как в desktop)**: при частичном sync и при отказе кнопки sync дочерние строки в DB меняются, но `universal-provider-synced` не эмитится (emit после `?`), поэтому основной список `["providers"]` устаревает до следующего refetch. Дёшево закрыть на фронте — инвалидировать `["providers"]` в catch sync; не делали, чтобы не расходиться с desktop.
- **NIT не делали**: DP-017 — std `Mutex` guard через `.await` во всех тестах `web_api_universal.rs` (паттерн соседних `web_api_*`; `clippy --all-targets` и так красный на baseline); формулировка `addedButSyncFailed` («добавлен, но…») для saveAndSync существующего провайдера — новых i18n-ключей не заводили.
- **Инвентаризация фазы 6**: строка `provider › universal` перенесена в «закрыто фазами 1–5, 7, 9». `get_universal_provider` теперь замаплен, но UI его по-прежнему не вызывает (строка «мёртвое» не менялась).
