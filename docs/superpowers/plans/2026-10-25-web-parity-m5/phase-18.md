# Phase 18 — WEB-10 / DX-68: DB backups и restore

Цель: создание, список, restore, переименование и удаление backup-ов DB, а также список и очистка backup locations работают в web. Restore сериализован с import и sync так же, как на desktop. Вместо reveal в файловом менеджере показывается путь на сервере.

Спека: § WEB-10. Оценка M (2–4 дн.). Идёт раньше WEB-09: import в фазе 19 переиспользует restore-lock отсюда.

Scope: `src-tauri/src/commands/import_export.rs:19-26` (restore-lock → `pub(crate)`), `src-tauri/src/web_api.rs` (7 роутов), `src-tauri/tests/web_api_backups.rs`, `src/lib/api/adapter.ts` (7 case), `src/components/settings/BackupStorageSection.tsx` (web-вариант reveal), тесты фронта.

Факты разведки:
- Фронт settings.ts:433-465 (`backupsApi`): `create_db_backup` → filename, `list_db_backups` → `BackupEntry[]`, `restore_db_backup {filename}`, `rename_db_backup {oldFilename, newName}`, `delete_db_backup {filename}`, `list_backup_locations`, `delete_backup_location {id}` → освобождённые байты, `reveal_backup_location {id}` → bool. Вызовы: `hooks/useBackupManager.ts:15-78`, `BackupStorageSection.tsx:49` (reveal), `BackupListSection.tsx:414`. Диалогов нет.
- Backend commands/import_export.rs: :147 `create_db_backup(State<AppState>)` → `db.backup_database_file()`; :165 `list_db_backups()` → `Database::list_backups()` (database/backup.rs:967); :171 `restore_db_backup(State, filename)` — `run_with_database_restore_lock` (:19-26, приватный, оборачивает `sync_mutex()`) → `skill_state_write_guard()` → `db.restore_from_backup` (backup.rs:1004) → `run_post_import_sync(&AppState)` (commands/sync_support.rs:8); :200 rename → `Database::rename_backup` (:1093); :209 delete → `Database::delete_backup` (:1157); :217 `list_backup_locations()`, :226 `delete_backup_location(id)` (backup_storage.rs:191/:241); :237 `reveal_backup_location(id)` → `tauri_plugin_opener::reveal_item_in_dir` на `backup_storage::location_dir` (:276).
- Всё handle-free, кроме reveal. `crate::services::sync_protocol::sync_mutex` публичный; web_api.rs:877 уже использует похожий `session_sync_mutex`.
- DTO `BackupEntry` (backup.rs:108), `BackupLocation` (backup_storage.rs:19) — camelCase.
- Тесты: ~37 в database/backup.rs (restore :2769, :2850, :2880 — удаление заблокировано во время restore, :2968 — битый backup, :3014 — будущая schema; конкурентный rename :2529), tokio-тест порядка restore-lock (import_export.rs:252), 4 в backup_storage.rs. Фронтового теста `useBackupManager` нет.

### Task 1: общий restore-lock
- [ ] `run_with_database_restore_lock` → `pub(crate)` (или перенос в `commands/sync_support.rs`); restore-команда и будущий web import (фаза 19) используют одну функцию. Тест порядка lock (import_export.rs:252) зелёный.

- [ ] Периодический backup: фаза 7 оставила desktop-блок lib.rs:1381-1392 («codex models checks/periodic backup») вне bootstrap. Прочитать его; если это backup DB по расписанию — запускать его же из headless bootstrap (handle-free часть) с тестом «после старта планировщик зарегистрирован»; codex models checks — записать в Notes, куда относятся.

### Task 2: backend — роуты + тесты
- [ ] `POST /api/backups` → filename; `GET /api/backups`; `POST /api/backups/:filename/restore` (lock → guard → restore → post-import sync в `spawn_blocking`); `PATCH /api/backups/:filename` (`{newName}`); `DELETE /api/backups/:filename`; `GET /api/backup-locations` (к каждой записи — серверный путь `location_dir`); `DELETE /api/backup-locations/:id` → освобождённые байты.
- [ ] `src-tauri/tests/web_api_backups.rs`: create → list содержит; изменение провайдера → restore → провайдер вернулся и live-файл в temp HOME пересинхронизирован; restore битого backup → `{ok:false}`, DB не тронута; restore backup-а с будущей schema → `{ok:false}`; `filename` с `../` → 400; rename на занятое имя → `{ok:false}`; два параллельных restore + delete того же backup во время restore → delete отклонён (как в backup.rs:2880), restore-ы сериализованы.

### Task 3: frontend
- [ ] 7 case в `mapCommand`; `reveal_backup_location` в web не мапить.
- [ ] `BackupStorageSection`: в web вместо кнопки reveal — серверный путь location с копированием (`copyText`).
- [ ] Тесты: `tests/hooks/useBackupManager.test.tsx` (create/restore/rename/delete через adapter, mock fetch); компонент — в web путь виден, reveal скрыт.
- [ ] Снять WEB-00-гейты (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-10-db-backups`, PR; DX-68 → In Review + ссылка. Закрыть чекбоксы, Notes (имя общего restore-lock — для фазы 19).

Риски для ревью:
- Restore во время активного proxy takeover (после фаз 27–28): restore меняет DB под работающим proxy. Поведение должно совпадать с desktop; тест или явная заметка в Notes фазы 27.

## Notes for next phase

- (заполнить по итогам)
