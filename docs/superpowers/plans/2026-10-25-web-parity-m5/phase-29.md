# Phase 29 — WEB-03 / DX-61: списки моделей от провайдера и CLI

Цель: получение списка моделей по конфигурации провайдера (OpenAI/Anthropic-совместимые API), через CLI на сервере (`agy`, `opencode`) и из OAuth-каталогов (Codex OAuth, xAI, GitHub Copilot) работает в web. Ошибки, таймауты и отсутствующий CLI доходят до UI тем же текстом, что на desktop.

Спека: § WEB-03. Оценка M (2–4 дн.). OAuth-каталоги опираются на менеджеры из `AppState` (фазы 25–26); `kimi_oauth_models` уже перенесён в фазе 26.

⚠️ Уточнение к спеке: OAuth-каталоги живут не в model_fetch.rs, а в `commands/codex_oauth.rs`, `xai_oauth.rs`, `copilot.rs`. Каталог Copilot спека не называет, но он использует те же формы провайдеров — включён сюда.

Scope: `src-tauri/src/commands/{codex_oauth,xai_oauth,copilot}.rs` (inner от `&AppState`), `src-tauri/src/web_api.rs` (7 роутов), `src-tauri/tests/web_api_models.rs`, `src/lib/api/adapter.ts` (7 case), тесты фронта.

Факты разведки:
- Фронт model-fetch.ts: `fetch_models_for_config {baseUrl, apiKey, isFullUrl?, modelsUrl?, customUserAgent?, apiFormat, requestHeaders}` (l.50), `get_agy_models {apiKey|null, baseUrl|null}` (l.70), `get_opencode_models` (l.88), `get_codex_oauth_models {accountId}` (l.97), `get_xai_oauth_models {accountId}` (l.106); copilot.ts:143/252 — `copilot_get_models`, `copilot_get_models_for_account {accountId}`.
- Вызовы `fetchModelsForConfig`: `ClaudeFormFields.tsx:337`, `ClaudeDesktopProviderForm.tsx:654`, `CodexFormFields.tsx:906`, `GeminiFormFields.tsx:97`, `HermesFormFields.tsx:153`, `KimiProviderForm.tsx:288`, `OpenClawFormFields.tsx:135`, `OpenCodeFormFields.tsx:262`, `PiProviderForm.tsx:1179`; `getAgyModels` — `GeminiFormFields.tsx:96`; `getOpenCodeModels` — `useOmoModelSource.ts:59`; Codex OAuth — `ClaudeFormFields.tsx:423`, `ClaudeDesktopProviderForm.tsx:592`; xAI — `ClaudeFormFields.tsx:459`, `CodexFormFields.tsx:890`, `ClaudeDesktopProviderForm.tsx:602`.
- `showFetchModelsError` (model-fetch.ts:112+) ищет в тексте ошибки «HTTP 401/403», «All candidates failed», «timeout»/«timed out», «Failed to parse» — текст должен дойти без изменений.
- Backend commands/model_fetch.rs, handle-free: l.28 `get_agy_models(apiKey, baseUrl)` (параметры буквально camelCase, `#[allow(non_snake_case)]`), логика inline, запускает `agy` (`agy_config::real_agy_bin`) с `tokio::time::timeout` 20 с; ошибки «Antigravity CLI not found; install agy first» и «Antigravity model discovery timed out». l.103 `get_opencode_models()` → `misc::run_detected_tool_command_with_timeout("opencode", &["models"], Some(20s), …)` (misc.rs:4242). l.177 `fetch_models_for_config(...)` → `services/model_fetch.rs:89 fetch_models(...)` (`FETCH_TIMEOUT_SECS=15`, `http_client::get_for_url`).
- OAuth-каталоги: `get_codex_oauth_models(account_id, State<CodexOAuthState>)` (codex_oauth.rs:92) → токен менеджера → `codex_oauth_models::fetch_models_with_token` (15 с); `get_xai_oauth_models(account_id, State<XaiOAuthState>)` (xai_oauth.rs:~91) — HTTP inline (`{XAI_API_BASE_URL}/models`, 15 с); `copilot_get_models(State<CopilotAuthState>)` (copilot.rs:~181) и `copilot_get_models_for_account` → `CopilotAuthManager::fetch_models*`.
- DTO `FetchedModel {id, ownedBy}`, `OpenCodeModelRef {providerId, modelId}` — camelCase; `CopilotModel` (copilot_auth.rs:259).
- Тесты: 4 в commands/model_fetch.rs (парсеры), 32 в services/model_fetch.rs, 5 в codex_oauth_models.rs; фронт мокает `fetch_models_for_config` (`KimiProviderForm.test.tsx`, `HermesFormFields.test.tsx`). Тестов на уровне хендлера (ошибки, таймаут, нет CLI) нет.

### Task 1: OAuth-каталоги от AppState
- [ ] `pub(crate) async fn *_inner(state: &AppState, account_id: Option<String>)` для Codex OAuth, xAI и двух Copilot-команд (менеджеры из `AppState`); команды вызывают inner.

### Task 2: backend — роуты + тесты
- [ ] `POST /api/models/fetch` (тело = аргументы `fetch_models_for_config`), `POST /api/models/agy` (`{apiKey, baseUrl}`), `GET /api/models/opencode`, `GET /api/models/codex-oauth?accountId=`, `GET /api/models/xai-oauth?accountId=`, `GET /api/models/copilot`, `GET /api/models/copilot/:accountId`. Ошибки — `fail(текст сервиса)`.
- [ ] `src-tauri/tests/web_api_models.rs`: fetch против axum-заглушки `/v1/models` → список; заглушка 401 → ошибка содержит «HTTP 401»; заглушка, отвечающая дольше таймаута → «timed out» (таймаут сервиса уменьшить через seam, если есть; иначе тест помечен `#[ignore]` с причиной); невалидный JSON → «Failed to parse»; `PATH` без `opencode` → ошибка `run_detected_tool…`; `PATH` с фейковым `opencode`, печатающим две модели → 2 `OpenCodeModelRef`; без `agy` → «Antigravity CLI not found; install agy first»; Codex OAuth без аккаунта → ошибка менеджера.

### Task 3: frontend
- [ ] 7 case в `mapCommand`.
- [ ] Тесты: `tests/lib/modelFetchApi.test.ts` (URL/метод/body); `showFetchModelsError` на ошибке, пришедшей из adapter, выбирает ту же ветку, что на desktop (401, timeout).
- [ ] В формах провайдеров в web подпись у кнопки CLI-моделей: «список берётся у CLI на сервере».
- [ ] Снять WEB-00-гейты с кнопок получения моделей (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-03-model-lists`, PR; DX-61 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- `apiKey` в теле POST к своему серверу — норма для tailnet-модели, но ключ не должен попасть в access-лог и в логи ошибок fetch (проверить маскирование в `services/model_fetch.rs` на ошибочных ветках).

## Notes for next phase

- (заполнить по итогам)
- `CC_SWITCH_MODEL_FETCH_TIMEOUT_SECS` (services/model_fetch.rs): per-attempt timeout for `fetch_models`, default 15 s when unset/non-positive/unparsable — test seam and manual override; companion test-only seams (never set on desktop) are `CC_SWITCH_AGY_SYSTEM_DIRS` / `CC_SWITCH_TOOL_SYSTEM_DIRS`, which redirect the hardcoded CLI search dirs so the `web_api_models` no-CLI tests stay hermetic.
