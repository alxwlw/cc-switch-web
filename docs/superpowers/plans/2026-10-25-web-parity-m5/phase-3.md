# Phase 3 — WEB-08 / DX-66: пользовательская библиотека иконок

Цель: list/add/rename/delete кастомных иконок работают в web через DB-backed HTTP handlers. Native file picker не нужен — иконка приходит из браузера как data URI (IconPicker.tsx:267-274, readAsDataUrl в utils/customIcon.ts:121-137 уже работают).

Спека: § WEB-08. Оценка S (1–2 дн.).

Scope: `src-tauri/src/web_api.rs` (4 роута), `src-tauri/tests/web_api_custom_icons.rs`, `src/lib/api/adapter.ts` (4 case), `tests/lib/customIconsApi.test.ts`. UI (`IconPicker`, `CustomIconNameDialog`, query-хуки `src/lib/query/customIcons.ts`) не меняем.

Факты разведки:
- `customIconsApi` (customIcons.ts:17-33): `get_custom_icons` / `add_custom_icon {name,data}` / `rename_custom_icon {id,name}` / `delete_custom_icon {id}` — все четыре не в `mapCommand` → `WebUnsupportedError`.
- Rust-логика handle-free: `services/provider/icon.rs` — `list_library_icons(db)` :261, `add_library_icon(db,…)` :267, `rename_library_icon(db,…)` :309, `delete_library_icon(db,id)` :328; команды custom_icon.rs:9-35 — лишь обёртки над `&state.db`.
- DTO уже camelCase: `database/dao/custom_icons.rs:16-27` (`CustomIconEntry {id,name,data,createdAt}`), `AddCustomIconResult {icon,existed}`; хранение — settings-таблица, ключ `custom_icons`.

### Task 1: backend — 4 роута + тесты
- [x] `GET /api/custom-icons` → list; `POST /api/custom-icons` (body `{name,data}`) → add; `PUT /api/custom-icons/:id` (body `{name}`) → rename; `DELETE /api/custom-icons/:id` → delete. Хендлеры по паттерну env_conflicts/tool_versions, blocking-часть через `spawn_blocking`, ошибки → `fail`/`bad_request`.
- [x] `src-tauri/tests/web_api_custom_icons.rs` (harness из фазы 2): add→list→rename→delete happy path; data URI сохраняется как есть. ~~add с дублем имени/пустым именем → 400; delete несуществующего → ошибка в `{ok:false}`~~ — неверно, фактический контракт сервиса (подтверждён ревью r0): дубль имени разрешён; дубль данных → 200 `{icon: <исходная запись>, existed: true}`, имя не меняется; пустое имя / невалидный PNG / rename несуществующего id → 500 `fail` с текстом сервиса дословно (`bad_request` в `web_api.rs` — только для невалидного app id); delete несуществующего → no-op, `{ok:true}`.
- [x] nextest зелёный (`cargo test`: web_api_custom_icons 4/4; полный прогон — только 3 предсуществующих красных).

### Task 2: frontend — adapter cases + тест
- [ ] 4 case в `mapCommand` (`get_custom_icons`, `add_custom_icon`, `rename_custom_icon`, `delete_custom_icon`) с теми же camelCase-полями в body.
- [ ] `tests/lib/customIconsApi.test.ts`: каждый метод — URL/метод/body через mock fetch; существующие `tests/components/IconPicker.test.tsx`, `CustomIconNameDialog.test.tsx`, `ProviderIcon.customIcon.test.tsx`, `tests/utils/customIcon.test.ts` не ломаются.
- [ ] `pnpm typecheck && pnpm test:unit` зелёные.

### Task 3: гейты, PR, Plane
- [ ] полный набор гейтов (fmt/clippy/nextest + typecheck/format:check/test:unit).
- [ ] ветка `web-parity/web-08-custom-icons`, PR; DX-66 → In Review + ссылка. Закрыть чекбоксы, Notes.

## Notes for next phase

- (заполнить по итогам)
