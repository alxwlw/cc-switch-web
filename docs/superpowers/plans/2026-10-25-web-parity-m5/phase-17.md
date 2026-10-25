# Phase 17 — WEB-17 / DX-75: Pi / Oh My Pi state, sessions и prompts

Цель: текущее состояние Pi и OMP, обновление usage-script, обнаружение Pi-сессий, native-файлы SYSTEM/APPEND и slash-command шаблоны работают в web. Конфликт ревизий (файл изменён снаружи) приходит во фронт с тем же текстом, что на desktop, и отдельным HTTP-статусом.

Спека: § WEB-17. Оценка M (3–5 дн.).

⚠️ Уточнение к спеке: «динамические Pi/OMP команды» — это только `get_${app}_current_state` (pi.ts:26-45); usage-script — одна статическая команда `update_pi_provider_usage_script` с `app` в аргументах. Revision/conflict уже реализованы в сервисе (`ensure_revision` → `AppError::Conflict`); новый DTO не нужен — нужен проброс конфликта через HTTP.

Scope: `src-tauri/src/commands/omp.rs` (`current_state` → `pub(crate)`), `src-tauri/src/web_api.rs` (10 роутов), `src-tauri/tests/web_api_pi.rs`, `src/lib/api/adapter.ts`, тесты фронта.

Факты разведки:
- Фронт pi.ts:26-45: `get_pi_current_state` / `get_omp_current_state` (шаблон `` `get_${app}_current_state` ``), `update_pi_provider_usage_script {app, id, usageScript}`, `get_pi_session_discovery`; prompts.ts:63-112: `get_pi_prompt_file {kind}`, `replace_pi_prompt_file {kind, expectedRevision, content}`, `delete_pi_prompt_file {kind, expectedRevision}`, `list_pi_prompt_templates`, `upsert_pi_prompt_template {slug, originalSlug|null, expectedRevision, content}`, `delete_pi_prompt_template {slug, expectedRevision}`.
- Вызовы: `App.tsx:1141`, `useProviderActions.ts:311`, `SessionManagerPage.tsx:191`, `PiNativePromptResources.tsx:96,166,341,345,357,378` (рендерится в `PiPromptPanel.tsx:25`).
- Backend: commands/pi.rs :11 `get_pi_current_state(State<AppState>)` → `PiStateService::current(&AppState)` (pi_state.rs:20, `pub(crate)`; внутри `futures::executor::block_on(state.proxy_service.lock_switch_for_app("pi"))`); :16 `update_pi_provider_usage_script` → `ProviderService::update_pi_usage_script(&AppState, AppType, &str, UsageScript)` (provider/mod.rs:6489, `pub(crate)`); :27 `get_pi_session_discovery()` → `session_manager::providers::pi::session_discovery()`. commands/omp.rs :23 `get_omp_current_state(State<AppState>)` → приватная `current_state(&AppState)` (:6), тот же `block_on`.
- commands/prompt.rs:89-133 — 6 async-команд без State над `PiPromptFileService::{read :109, replace :114, delete :129}` и `PiPromptTemplateService::{list :152, upsert :191, delete :233}`. `revision` — хэш содержимого, sentinel `MISSING_REVISION="missing"` (pi_prompt_files.rs:15); `ensure_revision` (:278) → `AppError::Conflict("<label> changed outside CC Switch: <path>")`; команды сплющивают ошибку в String.
- DTO: `PiCurrentState` — camelCase; `PiPromptFileKind` — snake_case (`system_override`, `system_append`); `PiPromptFileSnapshot`, `PiPromptTemplate` — camelCase; `PiSessionDiscovery` — `tag="status"`, snake_case.
- ⚠️ `block_on` внутри current state: из async-хендлера только через `spawn_blocking`, иначе паника/дедлок в runtime.
- Тесты: 6 unit в pi_prompt_files.rs, тест в omp.rs; фронт — `PiNativePromptResources.test.tsx`, `PiPromptPanel.test.tsx`, MSW-хендлер `tests/msw/handlers.ts:386`.

### Task 1: backend — роуты + тесты
- [ ] `GET /api/:app/current-state` (`app ∈ {pi, omp}`, иначе 400; `spawn_blocking`), `PUT /api/:app/providers/:id/usage-script` (`{usageScript}`; `app ∈ {pi, omp}`), `GET /api/pi/session-discovery`, `GET/PUT/DELETE /api/pi/prompt-files/:kind` (`{expectedRevision, content}` / `{expectedRevision}`), `GET /api/pi/prompt-templates`, `PUT /api/pi/prompt-templates/:slug` (`{originalSlug, expectedRevision, content}`), `DELETE /api/pi/prompt-templates/:slug` (`{expectedRevision}`).
- [ ] Хендлеры prompt-роутов вызывают сервисы (не команды), чтобы видеть `AppError::Conflict` → HTTP 409 с `{ok:false, error: <тот же текст>}`; прочие ошибки → 500.
- [ ] `src-tauri/tests/web_api_pi.rs` (temp agent dir, как `TestAgentDir`): read missing → `revision:"missing"`; replace с верной ревизией → новая ревизия; replace со старой ревизией после внешней правки файла → 409 и текст «changed outside CC Switch»; шаблоны: upsert, rename через `originalSlug`, delete с неверной ревизией → 409; current-state pi/omp → 200 из async-контекста без паники; usage-script сохраняется в провайдере.

### Task 2: frontend
- [ ] adapter: case для `get_pi_current_state`, `get_omp_current_state` и остальных 8 команд. `invoke` бросает `Error(payload.error)` и при 409 — проверить, что `PiNativePromptResources` показывает тот же conflict-UI, что на desktop (он разбирает текст ошибки).
- [ ] Тесты: `tests/lib/piApi.test.ts`; компонент — 409 от сервера даёт conflict-состояние.
- [ ] Снять WEB-00-гейты (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-17-pi-omp`, PR; DX-75 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Две вкладки web правят один prompt-файл: вторая запись с устаревшей ревизией обязана получить 409, а не перезаписать (тест — два replace подряд с одной ревизией).

## Notes for next phase

- (заполнить по итогам)
