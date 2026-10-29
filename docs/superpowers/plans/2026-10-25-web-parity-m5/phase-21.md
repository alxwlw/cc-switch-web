# Phase 21 — WEB-13 / DX-71: Sessions images, Markdown export и streaming

Цель: в web картинки из сессий отображаются, экспорт в Markdown скачивается файлом, длинные транскрипты подгружаются прогрессивно. Terminal и reveal в файловом менеджере в web не переносятся.

Спека: § WEB-13. Оценка M (2–4 дн.).

⚠️ Отличие от спеки: для прогрессивной загрузки — NDJSON-поток (`application/x-ndjson`) поверх `fetch` вместо SSE. Поток одноразовый, привязан к одному запросу; `fetch` + `ReadableStream` проще EventSource (заголовки, POST, явные ошибки) и не смешивается с общей шиной событий фазы 8. Решение зафиксировать комментарием в DX-71.

Scope: `src-tauri/src/web_api.rs` (2 роута), `src-tauri/tests/web_api_sessions_extra.rs`, `src/lib/api/adapter.ts` (binary-ответ, NDJSON-стрим, локальный экспорт), тесты фронта.

Факты разведки:
- Фронт sessions.ts: `stream_session_messages {providerId, sourcePath, onChunk: Channel}` (:49-61); `get_session_image {providerId, sourcePath, image}` (:84-93) → ArrayBuffer, Blob собирает вызывающий с `image.mediaType` (query/sessions.ts:329-334); `export_session_markdown {defaultName, content}` (:103-108) → сохранённый путь или null; `reveal_session_path {path}` (:97). Уже в web: `list_sessions`, `get_session_messages`, `get_session_block_content`, `delete_session(s)` (adapter.ts:191-225, web_api.rs:597-684).
- Вызовы: `query/sessions.ts:140` (stream), `:329` (image); `SessionReader.tsx:428` (export); `SessionMarkdown.tsx`/`PathChip` — прямой `invoke`.
- Backend commands/session_manager.rs: :47 `stream_session_messages(…, onChunk: tauri::ipc::Channel<TranscriptChunk>)` → `load_transcript`, затем `Header{total,turns,cached,parseMs}`, `Messages{start,messages}` по `cache::chunk_ranges(&transcript.message_bytes)`, `Done{payloadBytes}`; при ошибке — `Error{message}` и Err. :99 `get_session_image(…) -> tauri::ipc::Response` → `content::validate_source` + `content::load_image(&ValidatedSource,&ImageRef) -> Vec<u8>` (content.rs:541): inline-картинки декодируются из base64 (лимит `MAX_IMAGE_BYTES`), `LocalFile` канонизируется и проверяется по allowed dirs. :232 `export_session_markdown<R>(app: AppHandle<R>, defaultName, content)` — native save dialog; контент приходит с фронта. :113 `reveal_session_path` — opener.
- DTO: `TranscriptChunk` — `tag="type"`, варианты snake_case, поля camelCase (model.rs:383-387); `ImageRef` — camelCase (:357), `ImageSource` — `tag="kind"` (:373).
- Фоллбек: `loadTranscript` (query/sessions.ts:~146) переходит на `loadTranscriptOnce` (`getMessages`), если `streamMessages` бросил до первого чанка. В web `stream_session_messages` не замаплен → `WebUnsupportedError` → фоллбек срабатывает (тестом не покрыто). Web-`Channel` — заглушка (web-shims/core.ts:10-16).
- Тесты: `tests/lib/sessionsApi.test.ts`, `tests/lib/sessionsQuery.test.tsx` (mock `streamMessages`/`getImage`), `tests/components/SessionReader.test.tsx` (:109, :405, :480); 19 тестов в content.rs, тесты cache.rs.

### Task 1: backend — роуты + тесты
- [x] `POST /api/sessions/image` (body `{providerId, sourcePath, image}`) → `validate_source` + `load_image` в `spawn_blocking` → тело = байты, `Content-Type = image.mediaType` (только `image/*`, иначе `application/octet-stream`); ошибка → JSON `{ok:false}` со статусом 4xx/5xx.
- [x] `GET /api/sessions/stream?providerId=&sourcePath=` → NDJSON: по одной строке на `TranscriptChunk` (`Header`, затем `Messages…`, затем `Done`; при ошибке — строка `Error` и конец потока); формирование чанков вынести из команды в общую функцию `transcript_chunks(&Loaded) -> impl Iterator<Item = TranscriptChunk>`, которую используют и Channel-команда, и роут.
- [x] `src-tauri/tests/web_api_sessions_extra.rs`: inline base64-картинка → байты и верный `Content-Type`; `LocalFile` вне allowed dirs → отказ; поток на подготовленной сессии — первая строка `header`, последняя `done`, сумма сообщений = `total`; несуществующий source → строка `error`.

### Task 2: frontend
- [x] adapter: `Endpoint.responseType?: "json" | "arrayBuffer"`; `get_session_image` → POST image с `arrayBuffer`, без JSON-разбора при `res.ok`.
- [x] adapter: `stream_session_messages` — special-case до `mapCommand`: `fetch` NDJSON, построчно `JSON.parse` и `args.onChunk.onmessage(chunk)`; ошибка до первого чанка → throw (фоллбек `loadTranscript` продолжает работать); `Error`-чанк → передать в `onChunk` и throw, как на desktop.
- [x] adapter: `export_session_markdown` — локально: Blob `text/markdown`, скачивание с `defaultName`, вернуть `defaultName` (аналог сохранённого пути). `reveal_session_path` в web не мапить; `PathChip` в web — только копирование пути.
- [x] Тесты: stream — чанки доходят в порядке, ошибка до первого чанка уводит в фоллбек `getMessages` (покрыть и сам фоллбек); image — ArrayBuffer и Blob с mediaType; export — download с именем.
- [x] Снять WEB-00-гейты (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-13-sessions-media`, PR #22 (https://github.com/alxwlw/cc-switch-web/pull/22); DX-71 → In Review + ссылка. Чекбоксы закрыты, Notes ниже.

Риски для ревью:
- `sourcePath` из query — путь на сервере: `validate_source` обязан отсекать пути вне каталогов сессий так же, как для desktop (тест на `sourcePath=/etc/passwd`).
- Пользователь закрыл сессию на середине потока: сервер должен прекратить отправку без паники (обрыв соединения в тесте).

## Notes for next phase

- **Закрыта 2026-10-29**: PR #22 merged `3b2c53c9` → main; live на `cc-switch-web.service`.
  - Server и `dist-web` пересобраны из main. Перед restart, при остановленном unit, БД скопирована в `~/.cc-switch-web/backups-predeploy-p21-20261029-000103/`. Миграций нет.
  - Проверено: `/api/health` → 200; `GET /api/sessions/stream` с `sourcePath=/etc/passwd` → 200 `application/x-ndjson`, одна строка `error` «outside provider roots»; `POST /api/sessions/image {}` → 422; поток по реальной сессии claude — первая строка `header`, последняя `done`, 161 из 161 сообщений. WARN/ERROR в journal нет, отдаваемый `index.html` совпадает со сборкой.
  - DX-71 Done (комментарий со ссылками file:line). Ветка `web-parity/web-13-sessions-media` удалена локально и на origin.
- **Статус до merge (2026-10-28)**: ветка `web-parity/web-13-sessions-media` (worktree `.worktrees/web-13`), PR #22 (https://github.com/alxwlw/cc-switch-web/pull/22) открыт; DX-71 In Review (комментарий со ссылкой). Ветка также несёт закрытие фазы 20 в плане (`9543df8c`).
  - Финальные гейты на `93aae9fc`: fmt/clippy (lib + example) чисто; `(umask 022; cargo test --no-fail-fast)` 4431 passed / 1 failed (baseline omp_search_paths); `cargo build --release --example server` ок; `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3821 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - Т1 одобрена с r0; NIT DP-011 (тест обрыва соединения проверяет только живучесть роутера, ленивую сериализацию не закрепляет) и DP-022 (новый тест-файл добавляет 17 `await_holding_lock` под `clippy --all-targets` через `test_mutex`; CI гоняет clippy без `--all-targets`, `ci.yml:174`) приняты как есть. Т2 одобрена с r1: SHOULD-FIX DP-011 — проверка `!res.ok` в потоке не была закреплена (тело 500 `"no"` и так валило `JSON.parse`), добавлен тест 500 с валидным JSON-телом; NIT про отсутствие `reader.cancel()` после error-чанка принят (сервер закрывает поток сразу после строки error).
- **Решения контроллера** (journal p21): NDJSON поверх `fetch`, а не SSE (см. ⚠️ выше). Ошибка загрузки транскрипта — 200 с одной строкой `error`, как Error-чанк на desktop: в `loadTranscript` чанк ставит `received=true`, поэтому ошибка показывается, а не уходит в фоллбек; фоллбек на `get_session_messages` — только при отказе до первого чанка (сеть, не-2xx, нет тела). Картинка: `Content-Type` = `mediaType` только для `image/*` + `nosniff`; рендер только через `<img>` с blob URL — открывать blob во вкладке нельзя (иначе `image/svg+xml` исполнится в origin приложения).
- **Backend**: `session_manager::transcript_chunks` (`session_manager/mod.rs:163`) — владеющий итератор поверх `Arc<Transcript>`, его же итерирует Channel-команда. Роуты `web_api.rs:193-194`, хендлеры `session_stream` :1448 (сериализация строки — `chunk_line` :1438), `session_image` :1485. Тесты — `src-tauri/tests/web_api_sessions_extra.rs` (8); для сырых ответов — `web_call_raw` (`tests/support.rs:148`), им же пользоваться для будущих бинарных/потоковых роутов.
- **Frontend**: `Endpoint.responseType` (`adapter.ts:26`) — первый не-JSON путь адаптера (ветка `arrayBuffer` :974); special-cases `stream_session_messages` (`streamSessionMessages` :894) и `export_session_markdown` (:953) — до `mapCommand`. Общий `downloadText(filename, content, mime)` — `src/lib/download.ts`; для будущих web-скачиваний брать его. Тесты — `tests/lib/adapter.sessionsMedia.test.ts` (10).
- **Таблица фазы 6**: строки sessions — stream, image, export закрыты WEB-13; `reveal_session_path` и `launch_session_terminal` — desktop-only навсегда, остаются незамапленными.
- **Post-merge**: пересобрать server и `dist-web` (Rust менялся); перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет. Smoke: `GET /api/sessions/stream?providerId=claude&sourcePath=/etc/passwd` → 200, одна строка `{"type":"error",…}`; `POST /api/sessions/image {}` → 4xx; поток по реальной сессии из `GET /api/sessions` — первая строка `header`, последняя `done`. DX-71 → Done.
