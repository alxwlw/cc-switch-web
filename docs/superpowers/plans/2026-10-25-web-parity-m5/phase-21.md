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
- [ ] `POST /api/sessions/image` (body `{providerId, sourcePath, image}`) → `validate_source` + `load_image` в `spawn_blocking` → тело = байты, `Content-Type = image.mediaType` (только `image/*`, иначе `application/octet-stream`); ошибка → JSON `{ok:false}` со статусом 4xx/5xx.
- [ ] `GET /api/sessions/stream?providerId=&sourcePath=` → NDJSON: по одной строке на `TranscriptChunk` (`Header`, затем `Messages…`, затем `Done`; при ошибке — строка `Error` и конец потока); формирование чанков вынести из команды в общую функцию `transcript_chunks(&Loaded) -> impl Iterator<Item = TranscriptChunk>`, которую используют и Channel-команда, и роут.
- [ ] `src-tauri/tests/web_api_sessions_extra.rs`: inline base64-картинка → байты и верный `Content-Type`; `LocalFile` вне allowed dirs → отказ; поток на подготовленной сессии — первая строка `header`, последняя `done`, сумма сообщений = `total`; несуществующий source → строка `error`.

### Task 2: frontend
- [ ] adapter: `Endpoint.responseType?: "json" | "arrayBuffer"`; `get_session_image` → POST image с `arrayBuffer`, без JSON-разбора при `res.ok`.
- [ ] adapter: `stream_session_messages` — special-case до `mapCommand`: `fetch` NDJSON, построчно `JSON.parse` и `args.onChunk.onmessage(chunk)`; ошибка до первого чанка → throw (фоллбек `loadTranscript` продолжает работать); `Error`-чанк → передать в `onChunk` и throw, как на desktop.
- [ ] adapter: `export_session_markdown` — локально: Blob `text/markdown`, скачивание с `defaultName`, вернуть `defaultName` (аналог сохранённого пути). `reveal_session_path` в web не мапить; `PathChip` в web — только копирование пути.
- [ ] Тесты: stream — чанки доходят в порядке, ошибка до первого чанка уводит в фоллбек `getMessages` (покрыть и сам фоллбек); image — ArrayBuffer и Blob с mediaType; export — download с именем.
- [ ] Снять WEB-00-гейты (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-13-sessions-media`, PR; DX-71 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- `sourcePath` из query — путь на сервере: `validate_source` обязан отсекать пути вне каталогов сессий так же, как для desktop (тест на `sourcePath=/etc/passwd`).
- Пользователь закрыл сессию на середине потока: сервер должен прекратить отправку без паники (обрыв соединения в тесте).

## Notes for next phase

- (заполнить по итогам)
