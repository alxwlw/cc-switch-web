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
- [x] `mapCommand`: `delete_env_vars` → POST cleanup с `{conflicts}`; `restore_env_backup` → POST restore с `{backupPath}`.
- [x] `EnvWarningBanner.tsx`: в web-режиме (isWeb) явный текст, что переменные удаляются в OS user environment серверного хоста — в confirm-диалоге рядом с backupNotice (единственный путь к удалению), ключ `env.confirm.webHostWarning` во всех 5 локалях; backupPath баннер уже показывал toast'ом — не меняли.
- [x] `tests/components/EnvWarningBanner.test.tsx` (3) + `tests/lib/envApi.test.ts` (2). typecheck/format/build:renderer/build:web зелёные; test:unit 3328 passed / 3 failed (baseline). Коммит `1259b58c`, review r0 approved.

### Task 3: гейты, PR, Plane
- [x] полный набор гейтов на `1259b58c`: typecheck, format:check, build:renderer, build:web, cargo fmt/clippy `-D warnings`, `build --release --example server` — зелёные; test:unit 3328 passed / 3 failed, cargo test 4248 passed / 3 failed — только предсуществующие (flaky `forwarder::bookkeeping` в этом прогоне не упал).
- [x] ветка `web-parity/web-22-env-cleanup`, PR #5; DX-80 → In Review + ссылка. Закрыть чекбоксы, Notes.

## Notes for next phase

- **Статус 2026-10-25**: PR #5 (alxwlw/cc-switch-web) открыт, ветка `web-parity/web-22-env-cleanup`, worktree `.worktrees/web-22`; DX-80 → In Review с комментарием (ссылка + контракт). Задачи 1–2 прошли implement→review (r0 approved, 0 открытых), задача 3 — контроллером. Ждёт merge → `post-merge`: backend изменён — пересобрать server (`cargo build --release --example server` → `~/.local/bin/cc-switch-web`, прежний — в `.prev`), `pnpm build:web` в main-checkout, restart `cc-switch-web.service`; DX-80 → Done.
- В PR вошёл docs-коммит `410a520d` (закрытие фазы 3): он был только в локальном main.
- Базовая линия гейтов после фазы: cargo test 4248 passed / 3 failed, test:unit 3328 passed / 3 failed — красные те же предсуществующие.

- **Контракт ошибок роутов** (сверено с кодом, не с планом): битый JSON → 400, не та форма/нет поля → 422 — это дефолтный rejection axum `Json<T>`, как у остальных роутов `web_api.rs`; ошибка сервиса → 500 `{ok:false}` с дословным китайским текстом (`读取备份文件失败…`), как `Err(e.to_string())` Tauri-команды.
- **Unix-поведение env_manager**: `sourceType:"file"` правит rc-файл из `sourcePath` (берётся часть до `:`); `"system"` при delete — no-op, но в backup попадает, а restore такой записи падает; неизвестный тип — ошибка, backup остаётся. Сервис переписывает rc-файл без завершающего `\n` — существующее поведение, не меняли. `backup_dir()` = `dirs::home_dir()/.cc-switch/backups` → в тестах под test home. Тест-файл `cfg(not(windows))`.
- **Flaky вне диффа**: 3 теста `forwarder::bookkeeping` падают в полном `cargo test` нестабильно (1–4 падения от прогона к прогону на одном HEAD), в одиночку 17/17. Ревьюер воспроизвёл без связи с этой фазой. Итог Task 1: cargo test 4245 passed / 6 failed (3 baseline + 3 flaky).
- `clippy --all-targets` ругается `await_holding_lock` в тестах prompts/custom-icons (и в новом по тому же шаблону) — предсуществующее, CI гоняет clippy без `--all-targets`.
- **`isWeb` в компонентах**: первым потребителем стал `EnvWarningBanner` — импорт `isWeb` из `@/lib/api/adapter` (`typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window)`); в тестах переключается замоканным getter'ом (образец — `tests/components/EnvWarningBanner.test.tsx`). Фазе 6 (WEB-00, gating) — брать тот же helper. NIT ревью Task 2 (отложен): импорт тянет `adapter.ts` в desktop-бандл; безвредно (без side effects, `isWeb` false под Tauri), но если гейтов станет много — вынести `isWeb` в отдельный маленький модуль.
- `restoreEnvBackup` в UI никто не вызывает: web-роут restore есть и покрыт тестами, кнопки restore нет ни в desktop, ни в web.
- NIT ревью (DP-024, отложен): в `web_api_env.rs` локальный `raw_post` для не-JSON ответов axum (`web_call` паникует на plain-text rejection). Перенести raw-вариант в `support.rs`, когда он понадобится второму тест-файлу.
