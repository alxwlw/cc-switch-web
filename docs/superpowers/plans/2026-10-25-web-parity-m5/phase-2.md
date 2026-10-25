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
- [ ] `POST /api/:app/prompts/import-from-file` → `spawn_blocking` → `PromptService::import_from_file(&state, app_type)`; регистрация в `router()` рядом с прочими prompt-роутами (web_api.rs:68-94).
- [ ] `src-tauri/tests/web_api_prompts.rs`: harness — `web_api::router(Arc::new(create_test_state()?))` + `tower::ServiceExt::oneshot`; тесты: импорт из подготовленного live-файла (через `ensure_test_home`/`reset_test_fs`) возвращает `{ok:true,data:{id,…}}`; повторный импорт не дублирует; неизвестный app → 400.
- [ ] `cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci` (или `cargo test --manifest-path …`) — зелёный.

### Task 2: frontend — adapter case + тест
- [ ] `mapCommand`: `case "import_prompt_from_file": return { method: "POST", url: `${appBase(a.app)}/prompts/import-from-file` }` (стиль как у соседей, adapter.ts:44-308).
- [ ] `tests/lib/promptsApi.import.test.ts`: mock fetch → `promptsApi.importFromFile("claude")` бьёт в ожидаемый URL/метод, возвращает `data`; ошибка `{ok:false,error}` → throw.
- [ ] `pnpm typecheck && pnpm test:unit` — зелёные.

### Task 3: гейты, PR, Plane
- [ ] полный набор: `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, nextest ci, `pnpm typecheck && pnpm format:check && pnpm test:unit`.
- [ ] ветка `web-parity/web-31-prompt-import`, PR; Plane DX-89 → In Review + ссылка; комментарием в DX-89 зафиксировать расхождение со спекой (см. ⚠️ выше). Закрыть чекбоксы, Notes.

## Notes for next phase

- (заполнить по итогам)
