# Phase 2 — WEB-31 / DX-89: import prompt из файла

Цель: `promptsApi.importFromFile(app)` работает в web — тонкий HTTP-wrapper над серверным импортом из live-конфига. Попутно создаём отсутствующий harness для axum-тестов `web_api` (пригодится фазам 3–5).

Спека: § WEB-31. Оценка S (1–2 дн.).

⚠️ Расхождение со спекой: аудит предписывал «`<input type=file>` + upload вместо native dialog», но разведка показала: `import_prompt_from_file` (prompt.rs:55-62 → `PromptService::import_from_file`, services/prompt.rs:231-262) НЕ открывает диалог — он читает live-файл приложения на хосте и upsert-ит `Prompt { id: "imported-{timestamp}", enabled: false }`. Настоящий паритет = HTTP-wrapper к тому же сервису (файлы живут на серверном хосте). Browser-upload был бы другой функцией, не паритетом. Решение зафиксировать комментарием в DX-89.

Scope: `src-tauri/src/web_api.rs` (роут + хендлер), `src-tauri/tests/web_api_prompts.rs` (новый; harness по образцу `tests/support.rs::create_test_state`, oneshot через tower), `src/lib/api/adapter.ts` (case `import_prompt_from_file`), тест `tests/lib/promptsApi.import.test.ts`.

Факты разведки:
- `import_prompt_from_file` не в `mapCommand` → `WebUnsupportedError` (adapter.ts:335-336). Фронт: prompts.ts:51-53 `invoke("import_prompt_from_file", { app })`; вызовы usePromptActions.ts:289-291, PromptPanel.tsx:364, PiPromptPanel.tsx:267.
- Роуты prompts уже есть: `/api/:app/prompts…`, `/api/:app/prompt-content`, `/api/:app/prompt-location` (web_api.rs:68-94); паттерн хендлера `async fn(State(state): State<SharedState>, Path(app): Path<String>) -> ApiResult`, blocking через `spawn_blocking` (web_api.rs:190-195,213-227), `app_type!` макро (web_api.rs:53-60), ответы `ok/fail/bad_request` (web_api.rs:35-51).
- Rust-тестов для web_api нет вообще; ближайший образец — `src-tauri/tests/provider_commands.rs` + `support.rs` (`create_test_state`, `ensure_test_home`, `reset_test_fs`, `test_mutex`).

### Task 1: backend — роут + хендлер + axum-тест
- [x] `POST /api/:app/prompts/import-from-file` → `spawn_blocking` → `PromptService::import_from_file(&state, app_type)`; регистрация в `router()` рядом с прочими prompt-роутами (web_api.rs:68-94).
- [x] `src-tauri/tests/web_api_prompts.rs`: harness — `web_api::router(Arc::new(create_test_state()?))` + `tower::ServiceExt::oneshot`; тесты: импорт из подготовленного live-файла (через `ensure_test_home`/`reset_test_fs`) возвращает `{ok:true,data:{id,…}}`; повторный импорт не дублирует; неизвестный app → 400.
- [x] `cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci` (или `cargo test --manifest-path …`) — зелёный.

### Task 2: frontend — adapter case + тест
- [x] `mapCommand`: `case "import_prompt_from_file": return { method: "POST", url: `${appBase(a.app)}/prompts/import-from-file` }` (стиль как у соседей, adapter.ts:44-308).
- [x] `tests/lib/promptsApi.import.test.ts`: mock fetch → `promptsApi.importFromFile("claude")` бьёт в ожидаемый URL/метод, возвращает `data`; ошибка `{ok:false,error}` → throw.
- [x] `pnpm typecheck && pnpm test:unit` — зелёные.

### Task 3: гейты, PR, Plane
- [x] полный набор: `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, nextest ci, `pnpm typecheck && pnpm format:check && pnpm test:unit`.
- [x] ветка `web-parity/web-31-prompt-import`, PR; Plane DX-89 → In Review + ссылка; комментарием в DX-89 зафиксировать расхождение со спекой (см. ⚠️ выше). Закрыть чекбоксы, Notes.

## Notes for next phase

- **Закрыта 2026-10-25**: PR #3 merged `3a1252b5` → main; live на `cc-switch-web.service` — server пересобран (`cargo build --release --example server` → `~/.local/bin/cc-switch-web`, прежний бинарник — `~/.local/bin/cc-switch-web.prev`), `pnpm build:web` в main-checkout, restart unit; новый роут отвечает в проде. DX-89 Done. Worktree `.worktrees/web-31`, локальная и remote ветки удалены. Задачи 1–2 прошли implement→review (r0 approved), задача 3 — контроллером.
- В PR вошёл и docs-коммит `d57abcbf` (фазы 9–37): он был только в локальном main. После merge main и origin/main сойдутся.
- **Axum-harness для web_api** (для фаз 3–8 и далее): `web_router()` и `web_call(&router, method, uri, Option<json>) -> (StatusCode, Value)` в `src-tauri/tests/support.rs`; образец использования — `src-tauri/tests/web_api_prompts.rs`. Порядок обязателен: `test_mutex()` → `reset_test_fs()` → `ensure_test_home()` → подготовить live-файлы → `web_router()` (DB открывается из `HOME` в момент вызова). `web_call` клонирует router, так что несколько вызовов в одном тесте идут по одному state.
- **Известные ограничения harness (ревью DP-018, сознательно не сделано)**: `web_router()` прячет `Arc<AppState>` — тест не может засеять DB напрямую или подписаться на event bus фазы 8; `web_call` паникует на не-JSON ответе (`expect("json body")`) — download/SSE-роуты. Первая фаза, которой это нужно, добавляет `web_router_with(state: Arc<AppState>)` и raw-bytes вариант `web_call`.
- Контракт импорта: `data` — строка id (`imported-<unix seconds>`), не объект — так же у сервиса и desktop-команды (бриф ошибался). Повторный импорт не дедуплицируется: в ту же секунду перезаписывает запись, в другую — создаёт новую.
- `tower` (feature `util`, даёт `oneshot`) и `http-body-util` — обычные зависимости `src-tauri/Cargo.toml`, доступны и тестам; `Cargo.toml` этой фазой не менялся.
- Локально нет `cargo-nextest` — гейт гоняется как `cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast`.
- Предсуществующие красные на main@d57abcb (не наши, воспроизведены ревьюером на базе): Rust — `commands::misc::tests::omp_search_paths_include_the_bun_global_bin` (misc.rs:10208), `file_modes::switch_creates_expected_files_and_modes`, `file_modes::switch_keeps_private_modes_of_existing_files` (`--test golden`); frontend — те же 3 из Notes фазы 1. Итог фазы: cargo test 4238 passed / 3 failed, test:unit 3318 passed / 3 failed.
- `cargo fmt` на main был грязным (`web_api.rs`, `examples/server.rs`) — починено этим PR; fmt-гейт теперь чистый.
