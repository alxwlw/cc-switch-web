# Phase 34 — WEB-11 / DX-69 (этап 2): автоматическая синхронизация в headless scheduler

Цель: auto-sync WebDAV/S3 работает в web-сервере так же, как на desktop. Воркеры стартуют из общего bootstrap, статус приходит во фронт через SSE (фаза 8), переключатель auto-sync в web включён. DX-69 закрывается этой фазой.

Спека: § WEB-11 (полный объём — L, 5–8 дн.; этот этап — остаток после фазы 33).

Scope: `src-tauri/src/services/webdav_auto_sync.rs`, `src-tauri/src/services/s3_auto_sync.rs` (сигнатура `start_worker`), `src-tauri/src/lib.rs:1262-1270` (вызов), `src-tauri/src/bootstrap.rs` (фаза 7, вызов в headless), `src-tauri/examples/server.rs`, `src-tauri/tests/web_api_sync.rs` (дополнить), `WebdavSyncSection.tsx` (снять disabled с auto-sync).

Факты разведки:
- `webdav_auto_sync::start_worker(db: Arc<Database>, app: tauri::AppHandle)` — `services/webdav_auto_sync.rs:141`; S3 — `s3_auto_sync.rs:137`. AppHandle используется ТОЛЬКО для `app.emit`: `webdav-sync-status-updated` (webdav_auto_sync.rs:~92) и `s3-sync-status-updated` (s3_auto_sync.rs:91), payload `{source, status, error}`. Повторный старт отсекается OnceLock `DB_CHANGE_TX`; цикл запускается через `tauri::async_runtime::spawn(run_worker_loop)`.
- Триггер — не таймер: SQLite change hook (`register_db_change_hook`, database/mod.rs:120 внутри `Database::init`) → `notify_db_changed(table)` (database/mod.rs:88-89) → пропуск при `AutoSyncSuppressionGuard` (его держит ручной download) и для не-config таблиц (`should_trigger_auto_sync_for_table`, services/sync_protocol.rs) → `try_send` в канал ёмкостью 1 → debounce `AUTO_SYNC_DEBOUNCE_MS`=1000 мс, максимум `MAX_AUTO_SYNC_WAIT_MS`=10 с → `run_auto_sync_upload`, только при `enabled && auto_sync`.
- Воркер только upload: download при старте или по таймеру нет и на desktop. «Восстановление при startup» из спеки = запуск воркеров при старте сервера; новое поведение не добавляем.
- Hook уже ставится в headless `Database::init`, но `notify_db_changed` там no-op: `DB_CHANGE_TX` не установлен, пока не вызван `start_worker`.
- Фронт: `App.tsx:496` (`webdav-sync-status-updated`) и `:512` (`s3-sync-status-updated`) через `useTauriEvent`; оба инвалидируют `["settings"]`, toast — только при `source=="auto" && status=="error"`. После фазы 8 shim `listen` живой, менять эти вызовы не нужно.
- Inline-тесты сервисов: debounce, suppression guard, `should_run_auto_sync`, source-grep «сервисный слой не зависит от commands».

### Task 1: воркеры без AppHandle
- [ ] `start_worker(db: Arc<Database>, events: <тип event bus из фазы 8>)` для обоих сервисов; `app.emit(name, payload)` → `events.emit(name, payload)` с теми же именами и payload. В `lib.rs:1262-1270` передать bus; desktop получает события через мост bus→`app.emit` из фазы 8.
- [ ] Spawn: в server.rs до bootstrap вызвать `tauri::async_runtime::set(tokio::runtime::Handle::current())`, чтобы воркеры жили в runtime сервера; `tauri::async_runtime::spawn` в воркерах оставить (на desktop поведение не меняется).
- [ ] Существующие inline-тесты зелёные; source-grep тест дополнить запретом `tauri::AppHandle` в `*_auto_sync.rs`.

### Task 2: запуск в headless bootstrap
- [ ] `bootstrap.rs`: шаг `start_auto_sync_workers(db, events)` после `Database::init`, идемпотентный (OnceLock). Ошибка старта не фатальна — пишется в `BootstrapReport`.
- [ ] Удалить из Notes фазы 7 пометку «webdav/s3 workers → WEB-11», если она там есть.

### Task 3: интеграционные тесты
- [ ] `tests/web_api_sync.rs` (заглушка WebDAV из фазы 33): auto-sync включён, изменение провайдера в DB → в пределах `MAX_AUTO_SYNC_WAIT_MS` заглушка получает upload, bus выдаёт `webdav-sync-status-updated {source:"auto", status:"success"}`. Заглушка отвечает 500 → событие со `status:"error"`, `update_webdav_sync_status` записал ошибку. Auto-sync выключен → upload не происходит.
- [ ] Ручной download (фаза 33) не вызывает auto-upload (suppression guard работает и через HTTP).
- [ ] `DB_CHANGE_TX` глобальный: тесты сериализованы через `test_mutex` из support.rs.

### Task 4: frontend
- [ ] `WebdavSyncSection`: снять disabled с переключателя auto-sync, поставленный в фазе 33.
- [ ] Тест: SSE-событие `webdav-sync-status-updated {source:"auto", status:"error"}` → toast + инвалидация `["settings"]` (mock EventSource из фазы 8).

### Task 5: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; ручная проверка на agentbox: изменить провайдер в web → файл появился на реальном WebDAV.
- [ ] Ветка `web-parity/web-11-sync-auto`, PR; DX-69 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Два процесса (desktop и web) на одном каталоге данных синхронизируют наперегонки. Вне scope, но в Notes зафиксировать, что каталог данных web-сервера должен быть отдельным (`CC_SWITCH_CONFIG_DIR`).
- Если `tauri::async_runtime::set` вызван после первого spawn, он паникует: тест старта сервера должен это ловить.

## Notes for next phase

- (заполнить по итогам)
