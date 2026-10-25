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
- [ ] `POST /api/deeplink/parse` (`{url}`), `POST /api/deeplink/merge` (`{request}`), `POST /api/deeplink/icon` (`{url}`), `POST /api/deeplink/import` (`{request}`; блокирующий импорт — в `spawn_blocking`). Команды parse/merge/icon вызывать напрямую; import — через те же `import_*_from_deeplink(&AppState, …)`.
- [ ] `src-tauri/tests/web_api_deeplink.rs`: parse валидной provider-ссылки → request; parse мусора → `{ok:false}` с текстом парсера; import provider → провайдер в DB; import MCP с одним битым сервером → `importedCount` и `failed`; icon против локальной заглушки → data URI; icon с неотвечающего хоста укладывается в таймаут сервиса.

### Task 2: frontend
- [ ] 4 case в `mapCommand`.
- [ ] `DeepLinkImportDialog`: принять готовый request пропсом (или через тот же store, что питает pending), чтобы диалог открывался не только из pending-очереди; desktop-путь через peek/`deeplink-pending` не меняется.
- [ ] `DeepLinkPasteEntry` (web): поле «вставьте ccswitch://…» + загрузка `.txt`/`.url` файла со ссылкой → `parse_deeplink` → открыть диалог; ошибка парсинга — inline под полем. Точка входа — рядом с import/export в настройках и в меню добавления провайдера.
- [ ] Тесты: paste валидной ссылки открывает диалог с preview; файл со ссылкой — то же; невалидная ссылка — ошибка без диалога; desktop-ветка (`DeepLinkImportDialog.test.tsx`) зелёная.
- [ ] В таблице фазы 6: регистрация `ccswitch://` и pending-очередь → «desktop-only навсегда»; deep-link import → закрыт.

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-27-deeplink-paste`, PR; DX-85 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- `fetch_deeplink_icon` в web — серверный fetch произвольного URL из ссылки (SSRF внутрь tailnet и на loopback сервера). Проверить, что `icon_fetch` ограничивает схему, размер и запрещает loopback и приватные адреса; если нет — добавить в этой фазе с тестом на `http://127.0.0.1:…`.

## Notes for next phase

- (заполнить по итогам)
