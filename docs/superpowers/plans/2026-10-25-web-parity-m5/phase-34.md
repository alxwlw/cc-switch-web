# Phase 34 — WEB-11 / DX-69 (этап 2): автоматическая синхронизация в headless scheduler

Цель: auto-sync WebDAV/S3 работает в web-сервере так же, как на desktop. Воркеры стартуют из общего bootstrap, статус приходит во фронт через SSE (фаза 8), переключатель auto-sync в web включён. DX-69 закрывается этой фазой.

Спека: § WEB-11 (полный объём — L, 5–8 дн.; этот этап — остаток после фазы 33).

Scope: `src-tauri/src/services/webdav_auto_sync.rs`, `src-tauri/src/services/s3_auto_sync.rs` (сигнатура `start_worker`), `src-tauri/src/lib.rs:1262-1270` (вызов), `src-tauri/src/bootstrap.rs` (фаза 7, вызов в headless), `src-tauri/examples/server.rs`, `src-tauri/tests/web_api_sync.rs` (дополнить), `WebdavSyncSection.tsx` (снять disabled с auto-sync).

Факты разведки:
- `webdav_auto_sync::start_worker(db: Arc<Database>, events: EventHub)` — `services/webdav_auto_sync.rs`; S3 — `s3_auto_sync.rs` (сигнатура с `2a4e38cb`, фаза 8; на desktop события идут через мост bus→`app.emit`). Воркеры не знают про `AppHandle`: они эмитят `events.emit` — `webdav-sync-status-updated` и `s3-sync-status-updated`, payload `{source, status, error}`. Повторный старт отсекается OnceLock `DB_CHANGE_TX`; цикл запускается через `tauri::async_runtime::spawn(run_worker_loop)`.
- Триггер — не таймер: SQLite change hook (`register_db_change_hook`, database/mod.rs:120 внутри `Database::init`) → `notify_db_changed(table)` (database/mod.rs:88-89) → пропуск при `AutoSyncSuppressionGuard` (его держит ручной download) и для не-config таблиц (`should_trigger_auto_sync_for_table`, services/sync_protocol.rs) → `try_send` в канал ёмкостью 1 → debounce `AUTO_SYNC_DEBOUNCE_MS`=1000 мс, максимум `MAX_AUTO_SYNC_WAIT_MS`=10 с → `run_auto_sync_upload`, только при `enabled && auto_sync`.
- Воркер только upload: download при старте или по таймеру нет и на desktop. «Восстановление при startup» из спеки = запуск воркеров при старте сервера; новое поведение не добавляем.
- Hook уже ставится в headless `Database::init`, но `notify_db_changed` там no-op: `DB_CHANGE_TX` не установлен, пока не вызван `start_worker`.
- Фронт: `App.tsx:496` (`webdav-sync-status-updated`) и `:512` (`s3-sync-status-updated`) через `useTauriEvent`; оба инвалидируют `["settings"]`, toast — только при `source=="auto" && status=="error"`. После фазы 8 shim `listen` живой, менять эти вызовы не нужно.
- Inline-тесты сервисов: debounce, suppression guard, `should_run_auto_sync`, source-grep «сервисный слой не зависит от commands».

### Task 1: воркеры без AppHandle
- [x] (`2a4e38cb`, фаза 8; вызов `lib.rs:849-857`) `start_worker(db: Arc<Database>, events: <тип event bus из фазы 8>)` для обоих сервисов; `app.emit(name, payload)` → `events.emit(name, payload)` с теми же именами и payload. В `lib.rs:1262-1270` передать bus; desktop получает события через мост bus→`app.emit` из фазы 8.
- [x] Spawn: в server.rs до bootstrap вызвать `tauri::async_runtime::set(tokio::runtime::Handle::current())`, чтобы воркеры жили в runtime сервера; `tauri::async_runtime::spawn` в воркерах оставить (на desktop поведение не меняется). `set` — первым делом в `main`, до `server_log::init`; тест `tests/server_runtime_bootstrap.rs` (свой процесс: `set` + bootstrap через `spawn_blocking`, плюс source-проверка порядка).
- [x] Существующие inline-тесты зелёные; source-grep тест дополнить запретом `tauri::AppHandle` в `*_auto_sync.rs` (`worker_does_not_depend_on_the_tauri_app_handle`, запрещён `AppHandle` вообще).
- [x] Notes фазы 33, пункт a: `run_auto_sync_upload` читает настройки и проверяет `should_run_auto_sync` внутри `run_with_sync_lock` (оба воркера); тесты `lock_order_tests`.

### Task 2: запуск в headless bootstrap
- [x] `bootstrap.rs`: шаг `start_auto_sync_workers(db, events)` идемпотентный (OnceLock), шаг отчёта `auto_sync_workers`. Отклонение от плана (решение контроллера): запуск ПОСЛЕ `run_post_db_bootstrap`, не сразу после `Database::init` — как `lib.rs:849` на desktop; иначе записи bootstrap в БД запускали бы upload при каждом старте. `start_worker` возвращает `()`, шаг всегда `Done`; тест `auto_sync_workers_start_after_every_post_db_step` (`tests/bootstrap.rs`). `examples/server.rs` уже логирует все failed-шаги отчёта — не менялся.
- [x] Notes фазы 7: пометки «webdav/s3 workers → WEB-11» там нет; `phase-7.md:11` — исторический список «НЕ трогаем» в scope той фазы, не меняется (Ruling T2 в журнале plan-state).

### Task 3: интеграционные тесты
- [x] Новый бинарь `tests/web_api_sync_auto.rs` (отклонение от плана: `web_api_sync.rs` ровно 800 строк, Ruling T3), заглушка из `tests/support/dav_stub.rs`. Auto-sync включён, запись в `providers` через `state.db` → в пределах `MAX_AUTO_SYNC_WAIT_MS` заглушка получает `db.sql`/`skills.zip`/`manifest.json` (в db.sql новый ключ), hub выдаёт `webdav-sync-status-updated` и `s3-sync-status-updated` `{source:"auto", status:"success"}`; `GET /api/settings` показывает `lastSyncAt` без `lastError`. Заглушка отвечает 500 на PUT (`DavStub::fail_puts_with`) → событие `status:"error"` с текстом ошибки, `GET /api/settings` отдаёт тот же `lastError` и `lastErrorSource:"auto"`; после восстановления следующая запись → success, ошибка снята. Оба бэкенда в каждом тесте. Закрывает forward F1 из T1 (`Ok(true)` → success): проба `uploaded.map(|_| false)` в каждом воркере красная.
- [x] Auto-sync выключен (WebDAV `enabled` без `autoSync`, S3 `autoSync` без `enabled`) → за 11,5 с (> `MAX_AUTO_SYNC_WAIT_MS`) ни события, ни PUT; положительный контроль в том же тесте (оба включены → upload и success на той же заглушке).
- [x] Ручной download (фаза 33) через HTTP, отдельно для WebDAV и S3 (guard каждого бэкенда не покрывает чужой воркер, поэтому auto включён только у проверяемого): локальное состояние сдвинуто вторым соединением без hook, restore реально возвращает ключ; запись через hook во время download не даёт upload (проба: убрать `AutoSyncSuppressionGuard::new()` из обеих команд → оба теста красные); после download ожидание > `MAX_AUTO_SYNC_WAIT_MS` без событий и PUT; следующая запись через то же соединение даёт upload (hook пережил restore: `Backup` пишет страницы в то же соединение, `update_hook` остаётся).
- [x] `DB_CHANGE_TX` глобальный: вместо `test_mutex` (отклонение) один общий `Fixture` на бинарь, воркеры запускаются один раз через `bootstrap::start_auto_sync_workers`, тесты `#[serial]`, каждый начинается со сброса настроек и ожидания затишья на hub.

### Task 4: frontend
- [x] `WebdavSyncSection`: снять disabled с переключателя auto-sync, поставленный в фазе 33. (оба переключателя, WebDAV и S3; подпись и ключ `web.syncAutoLater` удалены из 5 локалей)
- [x] Тест: SSE-событие `webdav-sync-status-updated {source:"auto", status:"error"}` → toast + инвалидация `["settings"]` (mock EventSource из фазы 8). (`tests/hooks/useSyncStatusEvents.test.tsx`: два слушателя вынесены из `App.tsx` в хук `useSyncStatusEvents`, тест идёт через настоящий web-shim `listen` + FakeES, не через `emitTauriEvent`; проводку в `App` держит `tests/integration/App.test.tsx`)

### Task 5: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; ручная проверка на agentbox: изменить провайдер в web → файл появился на реальном WebDAV.
- [ ] Ветка `web-parity/web-11-sync-auto`, PR; DX-69 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Два процесса (desktop и web) на одном каталоге данных синхронизируют наперегонки. Вне scope, но в Notes зафиксировать, что каталог данных web-сервера должен быть отдельным (`CC_SWITCH_CONFIG_DIR`).
- Если `tauri::async_runtime::set` вызван после первого spawn, он паникует: тест старта сервера должен это ловить.

## Notes for next phase

- (заполнить по итогам)
