# Phase 19 — WEB-09 / DX-67: import/export конфигурации и sync live

Цель: в web экспорт конфигурации скачивается браузером, импорт загружается файлом с подтверждением и автоматическим backup, ручной `sync_current_providers_live` работает. Native save/open dialogs не переносятся.

Спека: § WEB-09. Оценка M (3–5 дн.).

Scope: `src-tauri/src/web_api.rs` (3 роута), `src-tauri/tests/web_api_import_export.rs`, `src/lib/api/settings.ts` (2 web-функции), `src/lib/api/adapter.ts`, `src/hooks/useImportExport.ts` (web-ветка), тесты фронта.

Факты разведки:
- Фронт settings.ts: `save_file_dialog {defaultName}` (:147), `open_file_dialog` (:151), `export_config_to_file {filePath}` (:155), `import_config_from_file {filePath}` (:159), `sync_current_providers_live` (:233). `ConfigTransferResult = {success, message, filePath?, backupId?}` (:10).
- Вызовы: `hooks/useImportExport.ts:52` (openFileDialog), `:85` (import), `:148` (saveFileDialog), `:158` (export); `utils/postChangeSync.ts:12` вызывает `syncCurrentProvidersLive` и бросает при `success == false`.
- Backend commands/import_export.rs: :32 `export_config_to_file(filePath, State)` → `db.export_sql(&Path)` (backup.rs:130); есть строковый вариант `export_sql_string` (:118). :53 `import_config_from_file(filePath, State)` → restore-lock + `skill_state_write_guard` → `db.import_sql(&Path) -> Result<String>` (:141) → `run_post_import_sync` → `success_payload_with_warning` (sync_support.rs:71); строковый вариант `import_sql_string(&str)` (:155). :82 `sync_current_providers_live(State)` → `ProviderService::sync_current_to_live(&AppState)`. :101/:117/:131 — dialog-команды (`AppHandle<R>`), в web не нужны.
- Ответ — `serde_json::Value` с camelCase-ключами (`filePath`, `backupId`).
- Restore-lock стал `pub(crate)` в фазе 18 — переиспользовать.
- Тесты: `tests/hooks/useImportExport.test.tsx`, `useImportExport.extra.test.tsx`, `tests/components/ImportExportSection.test.tsx`; unit — sync_support.rs:84. `src-tauri/tests/import_export_sync.rs` — про MCP/Codex live sync, не про SQL.
- Временный файл не нужен: строковые варианты уже есть.

### Task 1: backend — роуты + тесты
- [x] `GET /api/config/export` → `export_sql_string` → `{filename: "cc-switch-<YYYYMMDD-HHMMSS>.sql", content}`.
- [x] `POST /api/config/import` (body `{content}`; `DefaultBodyLimit` на этом роуте — 64 МиБ) → restore-lock фазы 18 → `skill_state_write_guard` → `import_sql_string` → `run_post_import_sync` → тот же `success_payload_with_warning` (с `backupId`).
- [x] `POST /api/providers/sync-live` → `sync_current_to_live` → `{success, message}` в форме desktop-команды.
- [x] `src-tauri/tests/web_api_import_export.rs`: export → import этого же содержимого в чистую DB восстанавливает провайдеров; ответ import содержит `backupId`, backup реально создан; импорт мусора → `{ok:false}`, DB не изменена; тело > 2 МиБ (сгенерированный дамп) проходит; sync-live переписывает live-файл текущего провайдера в temp HOME; import во время restore ждёт lock (сериализация).

### Task 2: frontend
- [x] `settings.ts`: `exportConfigForDownload()` → `invoke("export_config_to_string")`, `importConfigFromText(content)` → `invoke("import_config_from_string", {content})`; adapter мапит их и `sync_current_providers_live` на новые роуты.
- [x] `useImportExport` в web (`isWeb`): export — Blob + `<a download>` с `filename` от сервера, без `saveFileDialog`; import — `<input type="file" accept=".sql">` → `file.text()` → существующий шаг подтверждения → `importConfigFromText`; ошибки чтения файла и размера — в toast. Desktop-ветка не меняется.
- [x] Тесты: `useImportExport` web-ветка — export запускает download с именем от сервера; import показывает подтверждение до отправки и передаёт текст файла; ошибка сервера даёт toast с его текстом.
- [x] Снять WEB-00-гейты с import/export (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-09-import-export`, PR; DX-67 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Файл не UTF-8 (дамп с бинарными BLOB или другой кодировкой): `file.text()` испортит данные молча. Проверить, что `export_sql_string` выдаёт чистый UTF-8, и при ошибке парсинга на сервере показать понятный текст.
- Импорт дампа от более новой версии схемы: отказ с тем же текстом, что на desktop.

## Notes for next phase

- **Статус до merge (2026-10-27)**: ветка `web-parity/web-09-import-export` (worktree `.worktrees/web-09`), PR #20 (https://github.com/alxwlw/cc-switch-web/pull/20) открыт; DX-67 In Review (комментарий со ссылкой). Ветка также несёт закрытие фазы 18 в плане (`081b63bb`).
  - Финальные гейты на `25f79fc0`: fmt/clippy (lib + example) чисто; `cargo test --no-fail-fast` 4414 passed / 1 failed (baseline omp_search_paths); `cargo build --release --example server` ок; `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3796 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice). `cargo-nextest` на хосте не установлен — вместо `nextest --profile ci` гонялся `cargo test`.
  - Т1 одобрена с r1: SHOULD-FIX DP-018 — `skill_state_write_guard` в `import_config_with` не был покрыт тестом (мутант выживал), добавлен lib-тест `sql_import_waits_for_skill_state_write_guard` (100 мс негативное ожидание: у std `RwLock` нет способа увидеть заблокированного писателя). Т2 одобрена с r0 и переоткрыта контроллером (`fix --force`, решение в journal) по NIT DP-024 — риск плана «не-UTF-8 молча портится»: `file.text()` заменён на `TextDecoder("utf-8", {fatal: true})`, одобрена с r1. Принят как есть NIT DP-018 (сброс `webContentRef` в `clearSelection` ненаблюдаем).
- **Handle-free ядра** в `commands/import_export.rs`: `import_config_with(AppState, import: F)` :57 (restore-lock → `skill_state_write_guard` → `import(&db)` → `run_post_import_sync` → `success_payload_with_warning` с `backupId`) и `sync_current_providers_live_core(AppState)` :95; desktop `import_config_from_file` :83 и `sync_current_providers_live` :111 делегируют им. Импорт из строки для фаз 20 (deep-link config import) и 33 (WebDAV/S3) — через `import_config_with` с замыканием над `import_sql_string`, а не новым путём мимо lock-а.
- **Роуты** (`web_api.rs:377-382`, хендлеры с :2831): `GET /api/config/export` → `{filename, content}`; `POST /api/config/import` `{content}` (`CONFIG_IMPORT_BODY_LIMIT` 64 МиБ только на роуте) → desktop-payload; `POST /api/providers/sync-live` → `{success, message}`. Ошибки импорта — `AppError::Localized`/`Message` → HTTP 500 с дословным текстом (как ошибки restore фазы 18); 400 — только нет `content`. Тесты — `src-tauri/tests/web_api_import_export.rs` (9).
- **Экспорт — чистый UTF-8**: невалидный UTF-8-текст и BLOB-ячейки пишутся как `CAST(X'..')` (`dump_with_blob_and_invalid_text_exports_as_utf8_and_reimports`), поэтому round trip через `file.text()`/`TextDecoder` безопасен; BOM в начале файла `TextDecoder` по умолчанию срезает.
- **Frontend**: `settingsApi.exportConfigForDownload()` / `importConfigFromText(content)` (`settings.ts:170/175`) → команды-псевдонимы `export_config_to_string` / `import_config_from_string`, их и `sync_current_providers_live` мапит блок «Config import/export & manual live sync (WEB-09)» (`adapter.ts:586`). `useImportExport` web-ветка: `pickSqlFile()` (скрытый `<input type="file" accept=".sql">`), лимит `WEB_IMPORT_LIMIT_MIB = 64` по размеру файла (JSON-тело чуть больше из-за экранирования — дамп у самой границы получит 413), строгий decode, Blob + `<a download>`. i18n: `configDownloaded`, `importFileReadFailed`, `importFileTooLarge`, `importFileNotUtf8` во всех 5 локалях. Toast ошибки web-экспорта — `settings.exportFailed` + `: <текст сервера>` (в `exportFailedError` нет `{{message}}`).
- **Снятые гейты**: блок import/export в настройках › data виден в web (location WEB-26 и cloud sync WEB-11 скрыты, `WebShellGating.test.tsx`); `useSettings` больше не пропускает `sync_current_providers_live` в web (`useSettings.web.test.tsx` теперь проверяет вызов). Строки `phase-6.md` :97, :155 помечены.
- **Post-merge**: пересобрать server и `dist-web` (Rust менялся); перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет. Smoke: `GET /api/config/export` → 200 с `filename` `cc-switch-*.sql` и непустым `content`; `POST /api/config/import` с `{}` → 400; `POST /api/config/import` с мусором → `ok:false`, БД не изменилась. Деструктивный импорт и `sync-live` на живом хосте в smoke не делать (sync-live переписывает live-конфиги CLI). DX-67 → Done.
