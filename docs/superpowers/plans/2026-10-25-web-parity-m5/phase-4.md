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
- [x] `POST /api/env/conflicts/cleanup` (body `{conflicts:[EnvConflict]}`) → `spawn_blocking(env_manager::delete_env_vars)` → `ok(BackupInfo)`; `POST /api/env/restore` (body `{backupPath}`) → `restore_from_backup` → `ok(null)`.
- [x] `src-tauri/tests/web_api_env.rs` (6 тестов): cleanup на фиктивных sourcePath-файлах создаёт backup и удаляет переменные; restore восстанавливает; невалидный body → 400 (битый JSON) / 422 (не та форма — axum `Json` rejection); restore с несуществующим backupPath → 500 `{ok:false}` с дословной ошибкой сервиса. Unix-ветка реализована (см. Notes).
- [x] cargo test зелёный (кроме предсуществующих и flaky `forwarder::bookkeeping`, см. Notes). Коммит `0eb17d6b`, review r0 approved.

### Task 2: frontend — adapter + warning в UI
- [ ] `mapCommand`: `delete_env_vars` → POST cleanup с `{conflicts}`; `restore_env_backup` → POST restore с `{backupPath}`.
- [ ] `EnvWarningBanner.tsx`: в web-режиме (isWeb) явный текст, что переменные удаляются в OS user environment серверного хоста (не машины с браузером); показать backupPath после cleanup, если он возвращается UI-потоком.
- [ ] `tests/components/EnvWarningBanner.test.tsx` (или рядом): warning виден в web-режиме; flow cleanup→toast/backup не сломан. `pnpm typecheck && pnpm test:unit` зелёные.

### Task 3: гейты, PR, Plane
- [ ] полный набор гейтов.
- [ ] ветка `web-parity/web-22-env-cleanup`, PR; DX-80 → In Review + ссылка. Закрыть чекбоксы, Notes.

## Notes for next phase

- **Контракт ошибок роутов** (сверено с кодом, не с планом): битый JSON → 400, не та форма/нет поля → 422 — это дефолтный rejection axum `Json<T>`, как у остальных роутов `web_api.rs`; ошибка сервиса → 500 `{ok:false}` с дословным китайским текстом (`读取备份文件失败…`), как `Err(e.to_string())` Tauri-команды.
- **Unix-поведение env_manager**: `sourceType:"file"` правит rc-файл из `sourcePath` (берётся часть до `:`); `"system"` при delete — no-op, но в backup попадает, а restore такой записи падает; неизвестный тип — ошибка, backup остаётся. Сервис переписывает rc-файл без завершающего `\n` — существующее поведение, не меняли. `backup_dir()` = `dirs::home_dir()/.cc-switch/backups` → в тестах под test home. Тест-файл `cfg(not(windows))`.
- **Flaky вне диффа**: 3 теста `forwarder::bookkeeping` падают в полном `cargo test` нестабильно (1–4 падения от прогона к прогону на одном HEAD), в одиночку 17/17. Ревьюер воспроизвёл без связи с этой фазой. Итог Task 1: cargo test 4245 passed / 6 failed (3 baseline + 3 flaky).
- `clippy --all-targets` ругается `await_holding_lock` в тестах prompts/custom-icons (и в новом по тому же шаблону) — предсуществующее, CI гоняет clippy без `--all-targets`.
- NIT ревью (DP-024, отложен): в `web_api_env.rs` локальный `raw_post` для не-JSON ответов axum (`web_call` паникует на plain-text rejection). Перенести raw-вариант в `support.rs`, когда он понадобится второму тест-файлу.
