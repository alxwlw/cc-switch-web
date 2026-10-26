# Phase 14 — WEB-20 / DX-78: app-specific settings Claude и Codex

Цель: Claude plugin config и onboarding skip, а также Codex multi-agent probe, сжатие сессий, disk usage и восстановление unified history работают в web.

Спека: § WEB-20. Оценка M (2–4 дн.).

Scope: `src-tauri/src/web_api.rs` (9 роутов), `src-tauri/tests/web_api_app_settings.rs`, `src/lib/api/adapter.ts` (9 case), тесты фронта.

Факты разведки:
- Фронт settings.ts: Codex — `has_codex_unify_history_backup` (:44), `codex_forces_multi_agent_v2` (:49), `get_codex_session_compression` (:54), `set_codex_session_compression {enabled}` (:59), `get_codex_sessions_disk_usage` (:64), `restore_codex_unified_history` (:69); Claude — `apply_claude_plugin_config {official}` (:132-137), `apply_claude_onboarding_skip` (:139), `clear_claude_onboarding_skip` (:143). Диалогов нет.
- Вызовы: `CodexAuthSettings.tsx:40,46,60,77,112` (112 — restore), `StackSubagentFields.tsx:90`, `useSettings.ts:167,169,280,282,426,428`, `useProviderActions.ts:64`.
- Backend — все handle-free; State только у одной команды:
  - commands/settings.rs:178 `has_codex_unify_history_backup()` → `codex_history_migration::has_codex_official_history_unify_backup` (:429); :185 `codex_forces_multi_agent_v2()` → `codex_config::read_codex_config_text` + `codex_config_forces_multi_agent_v2`; :194 `get_codex_session_compression()` → `codex_session_compression::is_enabled()` (:28); :200 `set_codex_session_compression(state, enabled)` → `set_enabled(&Database, bool)` (:37); :216 `get_codex_sessions_disk_usage()` → `sessions_disk_usage() -> u64` (:67); :228 `restore_codex_unified_history()` → `restore_codex_official_history_from_backups` (codex_history_migration.rs:455).
  - commands/plugin.rs:24 `apply_claude_plugin_config(official)`, :40 `apply_claude_onboarding_skip()`, :46 `clear_claude_onboarding_skip()` → `claude_plugin::write_claude_config`/`clear_claude_config` (claude_plugin.rs:51,:90) и `claude_mcp::set_has_completed_onboarding`/`clear_has_completed_onboarding` (claude_mcp.rs:126).
- DTO `CodexUnifyHistoryRestoreResult` (settings.rs:243-251) — camelCase (`restoredJsonlFiles`, `restoredStateRows`, `skippedReason` — опускается при None); остальное — bool/u64.
- Codex side effects общего `save_settings` уже сделаны в web (web_api.rs:937-984) — не дублировать.
- Тесты: ~30 в codex_history_migration.rs (restore :1871, :1945), 4 в codex_session_compression.rs, `tests/components/CodexAuthSettings.test.tsx`; для Claude plugin-команд тестов нет.

### Task 1: backend — роуты + тесты
- [x] Codex: `GET /api/codex/unify-history-backup`, `GET /api/codex/forces-multi-agent-v2`, `GET/PUT /api/codex/session-compression` (`{enabled}`), `GET /api/codex/sessions-disk-usage`, `POST /api/codex/restore-unified-history`. Claude: `POST /api/claude/plugin-config` (`{official}`), `POST /api/claude/onboarding-skip`, `DELETE /api/claude/onboarding-skip`. Команды без tauri-типов вызывать напрямую; блокирующие (restore, disk usage, файловые записи) — в `spawn_blocking`.
- [x] `src-tauri/tests/web_api_app_settings.rs` (temp HOME): plugin-config `official:false` пишет ожидаемый Claude-конфиг, `official:true` — снимает (так в desktop-команде `commands/plugin.rs`; исходная формулировка плана была перевёрнута); onboarding skip ставит/снимает флаг в `~/.claude.json`; compression round-trip; disk usage на подготовленных сессиях возвращает их суммарный размер; restore с подготовленным backup → `restoredJsonlFiles > 0`; restore без backup → `skippedReason` присутствует и `{ok:true}`.

### Task 2: frontend
- [ ] 9 case в `mapCommand`.
- [ ] `tests/lib/appSettingsApi.test.ts`: URL/метод/body; `CodexAuthSettings.test.tsx` остаётся зелёным.
- [ ] Снять WEB-00-гейты с соответствующих controls (таблица фазы 6). В подписи restore в web — «восстанавливает историю Codex на сервере».

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-20-app-settings`, PR; DX-78 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Restore во время работающего Codex CLI на сервере: поведение как на desktop, но в web пользователь не видит процессов сервера — подтверждение в UI до restore (если на desktop его нет — добавить только для web).

## Notes for next phase

- (заполнить по итогам)
