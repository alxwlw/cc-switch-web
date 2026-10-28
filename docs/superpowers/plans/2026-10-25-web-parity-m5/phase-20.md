# Phase 20 — WEB-27 / DX-85: deep-link config import через paste/upload

Цель: в web пользователь вставляет `ccswitch://…`-ссылку или загружает файл с ней, видит тот же диалог импорта, что на desktop (preview, merge, иконка, риски), и импортирует provider/prompt/MCP/skill. Регистрация `ccswitch://` в ОС и pending-очередь не переносятся.

Спека: § WEB-27. Оценка M (2–4 дн.).

Scope: `src-tauri/src/web_api.rs` (4 роута), `src-tauri/tests/web_api_deeplink.rs`, `src/lib/api/adapter.ts` (4 case), `src/components/DeepLinkImportDialog.tsx` (вход не из pending-очереди), новый `src/components/deeplink/DeepLinkPasteEntry.tsx`, тесты фронта.

Факты разведки:
- Фронт deeplink.ts:100-152: `parse_deeplink {url}` (`parseDeeplink` — вне deeplink.ts вызовов нет), `merge_deeplink_config {request}`, `fetch_deeplink_icon {url}` → `string|null`, `import_from_deeplink_unified {request}`. Legacy `import_from_deeplink` фронтовой обёртки не имеет.
- `DeepLinkImportDialog.tsx`: `:168` (merge), `:59` (icon), `:264` (import), `:188-198` (peek/ack pending), `:221` (`listen("deeplink-pending")`). Диалог запускается только из pending-очереди и события — для paste/upload нужна новая точка входа, вызывающая `parse_deeplink`.
- Backend commands/deeplink.rs, всё handle-free: :22 `parse_deeplink(url)` → `parse_deeplink_url`; :30 `merge_deeplink_config(request)` → `deeplink::parse_and_merge_config`; :40 `fetch_deeplink_icon(url) -> Option<String>` (async); :65 `import_from_deeplink_unified(State<AppState>, request) -> Result<Value,String>` диспетчеризует по `request.resource` в `import_{provider,prompt,mcp,skill}_from_deeplink(&AppState, req)`; :10/:16 peek/ack (deeplink/mod.rs:74/:79).
- DTO: `DeepLinkImportRequest` (deeplink/mod.rs:91) — camelCase. Ответ unified: `{type:"provider",id}`, `{type:"prompt",id}`, `{type:"mcp",importedCount,importedIds,failed}`, `{type:"skill",key}`.
- Silent no-op `peek_pending_deeplink`/`ack_pending_deeplink` — adapter.ts:~390-402 (ссылка спеки на :323-333 устарела).
- Тесты: `src-tauri/tests/deeplink_import.rs` (4), `src/deeplink/tests.rs` (79), inline в icon_fetch.rs; фронт — `DeepLinkImportDialog.test.tsx`, `src/utils/deeplinkRisk.test.ts`, `tests/utils/deepLinkConfigPreview.test.ts`.

### Task 1: backend — роуты + тесты
- [x] `POST /api/deeplink/parse` (`{url}`), `POST /api/deeplink/merge` (`{request}`), `POST /api/deeplink/icon` (`{url}`), `POST /api/deeplink/import` (`{request}`; блокирующий импорт — в `spawn_blocking`). Команды parse/merge/icon вызывать напрямую; import — через те же `import_*_from_deeplink(&AppState, …)`.
- [x] `src-tauri/tests/web_api_deeplink.rs`: parse валидной provider-ссылки → request; parse мусора → `{ok:false}` с текстом парсера; import provider → провайдер в DB; import MCP с одним битым сервером → `importedCount` и `failed`; icon против локальной заглушки → data URI; icon с неотвечающего хоста укладывается в таймаут сервиса.

### Task 2: frontend
- [x] 4 case в `mapCommand`.
- [x] `DeepLinkImportDialog`: принять готовый request пропсом (или через тот же store, что питает pending), чтобы диалог открывался не только из pending-очереди; desktop-путь через peek/`deeplink-pending` не меняется.
- [x] `DeepLinkPasteEntry` (web): поле «вставьте ccswitch://…» + загрузка `.txt`/`.url` файла со ссылкой → `parse_deeplink` → открыть диалог; ошибка парсинга — inline под полем. Точка входа — рядом с import/export в настройках и в меню добавления провайдера.
- [x] Тесты: paste валидной ссылки открывает диалог с preview; файл со ссылкой — то же; невалидная ссылка — ошибка без диалога; desktop-ветка (`DeepLinkImportDialog.test.tsx`) зелёная.
- [x] В таблице фазы 6: регистрация `ccswitch://` и pending-очередь → «desktop-only навсегда»; deep-link import → закрыт.

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-27-deeplink-paste`, PR; DX-85 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- `fetch_deeplink_icon` в web — серверный fetch произвольного URL из ссылки (SSRF внутрь tailnet и на loopback сервера). Проверить, что `icon_fetch` ограничивает схему, размер и запрещает loopback и приватные адреса; если нет — добавить в этой фазе с тестом на `http://127.0.0.1:…`.

## Notes for next phase

- **Статус до merge (2026-10-28)**: ветка `web-parity/web-27-deeplink-paste` (worktree `.worktrees/web-27`), PR #21 (https://github.com/alxwlw/cc-switch-web/pull/21) открыт; DX-85 In Review (комментарий со ссылкой). Ветка также несёт закрытие фазы 19 в плане (`69004512`).
  - Финальные гейты на `33fa90dd`: fmt/clippy (lib + example) чисто; `(umask 022; cargo test --no-fail-fast)` 4423 passed / 1 failed (baseline omp_search_paths); `cargo build --release --example server` ок; `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3811 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - Т1 одобрена с r0; NIT DP-018 (prompt/skill-ветки и ошибка merge не покрыты route-тестом — функция общая с desktop, ветки покрывает `deeplink_import`) и DP-011 (loopback-тест иконки не различает отказ по схеме и `resolve_public`) приняты как есть. Т2 одобрена с r1: SHOULD-FIX DP-018 — в пути через событие не были закреплены начальное значение активации и `shownRequestRef` (без него диалог не закрывался после web-импорта); добавлен тест, NIT про дубль merge-then-show закрыт общим `presentRequest`.
- **Решения контроллера** (journal p20): SSRF-hardening иконки не нужен — `icon_fetch` уже пропускает только https-домены с публичными адресами (CGNAT 100.64/10 = tailnet отсекается), закрепляет адрес, перепроверяет каждый redirect. Тесты «иконка с локальной заглушки → data URI» и «неотвечающий хост» через роут неосуществимы без сети или test seam — заменены проверкой, что живой listener на 127.0.0.1 не получает соединений. Test seam в production не добавляли.
- **Backend**: handle-free `import_deeplink_unified(&AppState, request)` (`commands/deeplink.rs:73`), Tauri `import_from_deeplink_unified` (:65) делегирует. Роуты `web_api.rs:384-387`, хендлеры с :2885; `icon` отдаёт голый `Option<String>` (отказ → `null`, не ошибка). Тесты — `src-tauri/tests/web_api_deeplink.rs` (9).
- **Frontend**: блок адаптера «Deep-link import via paste/upload (WEB-27)» (`adapter.ts:599`); peek/ack в web — silent no-op (:880). Диалог открывается событием `web-deeplink-open` (`src/lib/deeplinkOpen.ts`, `openDeepLinkRequest(request)`) — диалог смонтирован один раз в корне App, а точки входа лежат в Settings и AddProviderDialog. Обе ветки (pending и событие) идут через `presentRequest` (`DeepLinkImportDialog.tsx:149`). `DeepLinkPasteEntry` (только web): `DEEPLINK_FILE_LIMIT_BYTES` 256 КиБ, `extractDeepLink` берёт первый `ccswitch://`-токен (работает и Windows `.url`). Точки входа: `SettingsPage.tsx:487` (под import/export), `AddProviderDialog.tsx:574` (шаг выбора пресета; отдельного «меню добавления провайдера» нет). Переводы ja/tr/zh-TW — без носителя.
- **Таблица фазы 6**: строка deeplink — закрыт WEB-27; регистрация `ccswitch://` и pending-очередь — desktop-only навсегда; `parse_deeplink` в строке «мёртвое» помечен как используемый в web.
- **Окружение**: в этой сессии umask 0002 — golden-тесты `file_modes` (ждут 644) падают; под `umask 022` зелёные. Полный `cargo test` гонять как `(umask 022; cargo test …)`.
- **Post-merge**: пересобрать server и `dist-web` (Rust менялся); перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет. Smoke: `POST /api/deeplink/parse` с мусором → `ok:false` с текстом парсера; `POST /api/deeplink/parse {}` → 400; `POST /api/deeplink/icon` с `http://127.0.0.1/x.png` → `null`. Реальный import на живом хосте в smoke не делать (добавит провайдера/MCP). DX-85 → Done.
