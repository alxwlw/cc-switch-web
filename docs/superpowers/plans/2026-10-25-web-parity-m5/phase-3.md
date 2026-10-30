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
- [x] 4 case в `mapCommand` (`get_custom_icons`, `add_custom_icon`, `rename_custom_icon`, `delete_custom_icon`) с теми же camelCase-полями в body.
- [x] `tests/lib/customIconsApi.test.ts`: каждый метод — URL/метод/body через mock fetch; существующие `tests/components/IconPicker.test.tsx`, `CustomIconNameDialog.test.tsx`, `ProviderIcon.customIcon.test.tsx`, `tests/utils/customIcon.test.ts` не ломаются.
- [x] `pnpm typecheck && pnpm test:unit` зелёные.

### Task 3: гейты, PR, Plane
- [x] полный набор гейтов (fmt/clippy/nextest + typecheck/format:check/test:unit).
- [x] ветка `web-parity/web-08-custom-icons`, PR; DX-66 → In Review + ссылка. Закрыть чекбоксы, Notes.

## Notes for next phase

- **Закрыта 2026-10-25**: PR #4 merged `c756f945` → main; live на `cc-switch-web.service` — server пересобран (`cargo build --release --example server` → `~/.local/bin/cc-switch-web`, прежний бинарник — `~/.local/bin/cc-switch-web.prev`), `pnpm build:web` в main-checkout, restart unit; `GET /api/custom-icons` в проде отвечает `{ok:true}`. DX-66 Done. Worktree `.worktrees/web-08`, локальная и remote ветки удалены; ledgers прогона перенесены в `.claude/plans/2026-10-25-web-parity-m5-p3` main-checkout. Задачи 1–2 прошли implement→review (r0 approved, 0 открытых), задача 3 — контроллером.
- В PR вошёл docs-коммит `4bad1165` (закрытие фазы 2): он был только в локальном main.
- **Ожидания плана об ошибках сверять с сервисом, а не угадывать.** План фазы 3 предполагал 400/`{ok:false}` там, где сервис даёт `existed:true`, `fail` (500) и no-op `ok:true`; implementer зафиксировал фактический контракт, ревью подтвердило, Task 1 переписан. В `web_api.rs` `bad_request` — только внутри `app_type!` (невалидный app id); любая ошибка сервиса — `fail(e.to_string())`, что совпадает с `Err(e.to_string())` Tauri-команды. Брифам следующих фаз — проверять это по коду сервиса.
- Тест на add требует настоящий декодируемый PNG (сервис полностью декодирует картинку) — `web_api_custom_icons.rs` собирает его через crate `png` (уже в зависимостях).
- Адаптер: path-параметры кодируются `enc()` (encodeURIComponent), axum `Path` их декодирует; delete возвращает `data:true`, `customIconsApi.remove` его игнорирует.
- Базовая линия гейтов после фазы: cargo test 4242 passed / 3 failed, test:unit 3323 passed / 3 failed — красные те же предсуществующие (см. Notes фаз 1–2).
- Свежий worktree без `node_modules`: implementer поставил симлинк `node_modules -> ../../node_modules` (untracked: `.gitignore` `node_modules/` не матчит симлинк). Перед `git worktree remove` — удалить симлинк.
