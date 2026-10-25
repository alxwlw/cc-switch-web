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
- [ ] `GET /api/config/export` → `export_sql_string` → `{filename: "cc-switch-<YYYYMMDD-HHMMSS>.sql", content}`.
- [ ] `POST /api/config/import` (body `{content}`; `DefaultBodyLimit` на этом роуте — 64 МиБ) → restore-lock фазы 18 → `skill_state_write_guard` → `import_sql_string` → `run_post_import_sync` → тот же `success_payload_with_warning` (с `backupId`).
- [ ] `POST /api/providers/sync-live` → `sync_current_to_live` → `{success, message}` в форме desktop-команды.
- [ ] `src-tauri/tests/web_api_import_export.rs`: export → import этого же содержимого в чистую DB восстанавливает провайдеров; ответ import содержит `backupId`, backup реально создан; импорт мусора → `{ok:false}`, DB не изменена; тело > 2 МиБ (сгенерированный дамп) проходит; sync-live переписывает live-файл текущего провайдера в temp HOME; import во время restore ждёт lock (сериализация).

### Task 2: frontend
- [ ] `settings.ts`: `exportConfigForDownload()` → `invoke("export_config_to_string")`, `importConfigFromText(content)` → `invoke("import_config_from_string", {content})`; adapter мапит их и `sync_current_providers_live` на новые роуты.
- [ ] `useImportExport` в web (`isWeb`): export — Blob + `<a download>` с `filename` от сервера, без `saveFileDialog`; import — `<input type="file" accept=".sql">` → `file.text()` → существующий шаг подтверждения → `importConfigFromText`; ошибки чтения файла и размера — в toast. Desktop-ветка не меняется.
- [ ] Тесты: `useImportExport` web-ветка — export запускает download с именем от сервера; import показывает подтверждение до отправки и передаёт текст файла; ошибка сервера даёт toast с его текстом.
- [ ] Снять WEB-00-гейты с import/export (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-09-import-export`, PR; DX-67 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Файл не UTF-8 (дамп с бинарными BLOB или другой кодировкой): `file.text()` испортит данные молча. Проверить, что `export_sql_string` выдаёт чистый UTF-8, и при ошибке парсинга на сервере показать понятный текст.
- Импорт дампа от более новой версии схемы: отказ с тем же текстом, что на desktop.

## Notes for next phase

- (заполнить по итогам)
