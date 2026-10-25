# Phase 4 — WEB-22 / DX-80: env conflicts cleanup с backup/restore

Цель: удаление конфликтующих env-переменных с backup и restore работают в web через два HTTP handler; в UI явное предупреждение, что изменяется OS user environment СЕРВЕРНОГО хоста. Detection уже поддержан (`GET /api/env/conflicts`).

Спека: § WEB-22. Оценка S (1–2 дн.).

Scope: `src-tauri/src/web_api.rs` (2 роута), `src-tauri/tests/web_api_env.rs`, `src/lib/api/adapter.ts` (2 case), `src/components/env/EnvWarningBanner.tsx` (server-host warning для web), тесты фронтовые.

Факты разведки:
- Фронт (env.ts): `checkEnvConflicts` — уже web-ok (adapter.ts:296-297); `deleteEnvVars(conflicts)` :24-28 и `restoreEnvBackup(backupPath)` :34-36 — не в `mapCommand` → `WebUnsupportedError`. Потребитель — `EnvWarningBanner.tsx:8` (deleteEnvVars), `App.tsx:37` (check*).
- Rust handle-free: `services/env_manager.rs` — `delete_env_vars(conflicts) -> Result<BackupInfo,String>` :21, `restore_from_backup(backup_path) -> Result<(),String>` :153; команды env.rs:7-22 — обёртки. DTO camelCase: `EnvConflict {varName,varValue,sourceType,sourcePath}` (env_checker.rs:6), `BackupInfo {backupPath}` (env_manager.rs:13); фронтовые типы совпадают (types/env.ts:8-29).
- ⚠️ `env_manager`/`env_checker` содержат winreg-ветки `#[cfg(target_os="windows")]` (env_manager.rs:8-9, env_checker.rs:14-17) — на Linux-сервере поведение unix-ветки проверить отдельно; тесты НЕ должны трогать реальное окружение пользователя: фикстуры — временные файлы sourcePath через `ensure_test_home`/`reset_test_fs`.
- Существующие env-роуты: `/api/env/tools` (POST), `/api/env/conflicts` (GET) — web_api.rs:137-138,1003-1033.

### Task 1: backend — cleanup/restore роуты + тесты
- [ ] `POST /api/env/conflicts/cleanup` (body `{conflicts:[EnvConflict]}`) → `spawn_blocking(env_manager::delete_env_vars)` → `ok(BackupInfo)`; `POST /api/env/restore` (body `{backupPath}`) → `restore_from_backup` → `ok(null)`.
- [ ] `src-tauri/tests/web_api_env.rs`: cleanup на фиктивных sourcePath-файлах создаёт backup и удаляет переменные; restore восстанавливает; невалидный body → 400; restore с несуществующим backupPath → `{ok:false}`. Проверить unix-поведение (если unix-ветка не реализована — зафиксировать в Notes и gates-поведение: честная ошибка, не паника).
- [ ] nextest зелёный.

### Task 2: frontend — adapter + warning в UI
- [ ] `mapCommand`: `delete_env_vars` → POST cleanup с `{conflicts}`; `restore_env_backup` → POST restore с `{backupPath}`.
- [ ] `EnvWarningBanner.tsx`: в web-режиме (isWeb) явный текст, что переменные удаляются в OS user environment серверного хоста (не машины с браузером); показать backupPath после cleanup, если он возвращается UI-потоком.
- [ ] `tests/components/EnvWarningBanner.test.tsx` (или рядом): warning виден в web-режиме; flow cleanup→toast/backup не сломан. `pnpm typecheck && pnpm test:unit` зелёные.

### Task 3: гейты, PR, Plane
- [ ] полный набор гейтов.
- [ ] ветка `web-parity/web-22-env-cleanup`, PR; DX-80 → In Review + ссылка. Закрыть чекбоксы, Notes.

## Notes for next phase

- (заполнить по итогам)
