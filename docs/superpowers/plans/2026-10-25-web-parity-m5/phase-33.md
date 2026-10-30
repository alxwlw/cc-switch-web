# Phase 33 — WEB-11 / DX-69 (этап 1): WebDAV/S3 ручной режим

Цель: test connection / save settings / upload / download / remote info для WebDAV и S3 работают в web через тонкие HTTP-wrappers. Автоматическая синхронизация — фаза 34; DX-69 после этой фазы остаётся In Progress.

Спека: § WEB-11 (ручной режим — M, 3–4 дн.).

Scope: `src-tauri/src/web_api.rs` (10 роутов), `src-tauri/tests/web_api_sync.rs` (новый, с WebDAV-заглушкой на axum), `src/lib/api/adapter.ts` (10 case), `tests/lib/syncApi.test.ts`, снятие WEB-00-гейта с `WebdavSyncSection`.

Факты разведки:
- Фронт settings.ts:163-230 — 10 функций: `webdav_test_connection {settings, preserveEmptyPassword}`, `webdav_sync_upload`, `webdav_sync_download`, `webdav_sync_save_settings {settings, passwordTouched}`, `webdav_sync_fetch_remote_info` + те же пять с префиксом `s3_*`. UI: `WebdavSyncSection.tsx:480` (test), `:507` (save), `:532` (повторный test); монтируется в `SettingsPage.tsx:494-495` (`config=settings.webdavSync`, `s3Config=settings.s3Sync`). В web все 10 сейчас → `WebUnsupportedError`.
- Команды handle-free: `commands/webdav_sync.rs` :105 test (без State), :122 upload (`state.db`), :134 download (+ `run_post_import_sync(&AppState)` из `commands/sync_support` в `spawn_blocking`), :171 save (без State), :197 remote info (без State). S3-двойники — `commands/s3_sync.rs` :100, :117, :128, :163, :184. Сервисы `services/webdav_sync.rs` (`check_connection` :49, `upload` :59, `download` :105, `fetch_remote_info` :159, `run_with_sync_lock`) и `services/s3_sync.rs` (:31, :38, :82, :123). Результаты команд — нетипизированный `serde_json::Value`.
- Настройки хранятся через глобальный `settings_store()` (`settings.rs`: `get_webdav_sync_settings` :1507, `set_webdav_sync_settings` :1512, `update_webdav_sync_status` :1519, S3 — ~:1530) в `settings.json` каталога данных; DTO `WebDavSyncSettings` (:132), `S3SyncSettings` (:207), `WebDavSyncStatus` (:107) — camelCase. Пароль хранится открытым текстом.
- `get_settings`/`save_settings` уже есть в web (adapter.ts:313-319); `merge_settings_for_save` (commands/settings.rs:15-20) сохраняет `webdav_sync` — общий save настройки синхронизации не затирает.
- Тесты: inline `#[cfg(test)]` в `commands/webdav_sync.rs` (~:208) и `commands/s3_sync.rs` (:192); в `src-tauri/tests/` по sync ничего нет.

### Task 1: backend — 10 роутов
- [ ] Роуты `POST /api/sync/:backend/test` (body `{settings, preserveEmptyPassword}`), `POST /api/sync/:backend/upload`, `POST /api/sync/:backend/download`, `PUT /api/sync/:backend/settings` (body `{settings, passwordTouched}`), `GET /api/sync/:backend/remote-info`; `backend ∈ {webdav, s3}`, иначе 400. Хендлеры вызывают те же функции, что и команды (вынести общее тело команды в `pub(crate) fn`, если оно сейчас inline в `#[tauri::command]`); download — с тем же `run_post_import_sync`.
- [ ] Ответ хендлера = значение, которое возвращает команда, через `ok(...)`; ошибка команды (`Err(String)`) → `fail`.

### Task 2: тесты backend
- [ ] `src-tauri/tests/web_api_sync.rs`: заглушка WebDAV на axum в том же тесте, отвечающая на методы, которые реально вызывает `services/webdav_sync.rs` (посмотреть по коду: PUT/GET/PROPFIND/MKCOL), с хранением в памяти.
- [ ] Кейсы: `test` против заглушки → ok; `test` против закрытого порта localhost → `{ok:false}` за время меньше таймаута сервиса; save → get_settings возвращает сохранённое, `passwordTouched:false` сохраняет прежний пароль; upload → заглушка получила архив; download с заглушки → провайдеры восстановлены в DB и `run_post_import_sync` отработал (live-файл в temp HOME обновлён); неизвестный backend → 400.
- [ ] S3: только error-path (недоступный endpoint → `{ok:false}`, save/get round-trip). Happy path S3 (SigV4) проверить вручную на agentbox и записать результат в Notes.

### Task 3: frontend
- [ ] 10 case в `mapCommand` с теми же camelCase-полями body.
- [ ] Снять WEB-00-гейт с `WebdavSyncSection` (строка таблицы инвентаризации фазы 6). Переключатель auto-sync до фазы 34 оставить disabled с подписью «автосинхронизация в web — в следующем релизе».
- [ ] `tests/lib/syncApi.test.ts`: URL/метод/body для каждой из 10 функций; ошибка `{ok:false}` → throw. `pnpm typecheck && pnpm test:unit` зелёные.

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-11-sync-manual`, PR; DX-69 остаётся In Progress, комментарий со ссылкой на PR «этап 1 из 2». Закрыть чекбоксы, Notes.

Риски для ревью:
- Пароль WebDAV/S3 не должен уходить в браузер открытым текстом через `GET /api/settings`: проверить, как desktop маскирует (на это указывают `preserveEmptyPassword`/`passwordTouched`), и повторить то же в web.
- Параллельные upload/download из двух вкладок: `run_with_sync_lock` должен сериализовать их и в web (тест на два одновременных запроса).

## Notes for next phase

- (заполнить по итогам)
