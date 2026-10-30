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
- [x] `pub(crate) async fn *_inner(state: &AppState, account_id: Option<String>)` для Codex OAuth, xAI и двух Copilot-команд (менеджеры из `AppState`); команды вызывают inner.

### Task 2: backend — роуты + тесты
- [x] `POST /api/models/fetch` (тело = аргументы `fetch_models_for_config`), `POST /api/models/agy` (`{apiKey, baseUrl}`), `GET /api/models/opencode`, `GET /api/models/codex-oauth?accountId=`, `GET /api/models/xai-oauth?accountId=`, `GET /api/models/copilot`, `GET /api/models/copilot/:accountId`. Ошибки — `fail(текст сервиса)`.
- [ ] `src-tauri/tests/web_api_models.rs`: fetch против axum-заглушки `/v1/models` → список; заглушка 401 → ошибка содержит «HTTP 401»; заглушка, отвечающая дольше таймаута → «timed out» (таймаут сервиса уменьшить через seam, если есть; иначе тест помечен `#[ignore]` с причиной); невалидный JSON → «Failed to parse»; `PATH` без `opencode` → ошибка `run_detected_tool…`; `PATH` с фейковым `opencode`, печатающим две модели → 2 `OpenCodeModelRef`; без `agy` → «Antigravity CLI not found; install agy first»; Codex OAuth без аккаунта → ошибка менеджера.

### Task 3: frontend
- [x] 7 case в `mapCommand`.
- [ ] Тесты: `tests/lib/modelFetchApi.test.ts` (URL/метод/body); `showFetchModelsError` на ошибке, пришедшей из adapter, выбирает ту же ветку, что на desktop (401, timeout).
- [ ] В формах провайдеров в web подпись у кнопки CLI-моделей: «список берётся у CLI на сервере».
- [ ] Снять WEB-00-гейты с кнопок получения моделей (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-03-model-lists`, PR; DX-61 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- `apiKey` в теле POST к своему серверу — норма для tailnet-модели, но ключ не должен попасть в access-лог и в логи ошибок fetch (проверить маскирование в `services/model_fetch.rs` на ошибочных ветках).

## Notes for next phase

- **Закрыта 2026-10-30**: PR #30 merged `68f03f92` → main; live на `cc-switch-web.service` с 13:46; DX-61 Done.
  - Server и `dist-web` пересобраны из main. Перед restart, при остановленном unit, БД скопирована в `~/.cc-switch-web/backups-predeploy-p29-20261030-134318/`, прежний бинарник — `cc-switch-web.prev`. Миграций нет. Нюанс деплоя: cargo кладёт example server в `src-tauri/target/release/examples/server`, не в `target/release/examples/`; первый `cp` мимо, доустановлен после stop (бинарник от 13:45, `Text file busy` без stop).
  - Smoke: консоль 200; `GET /api/models/opencode` — живой JSON со списком CLI; fetch против реального провайдера — 28 моделей; кнопка fetch-models в web и подпись у agy — ок; паник в журнале нет.
- **Статус до merge (2026-10-30)**: ветка `web-parity/web-03-model-lists` (worktree `.worktrees/web-03-models`), PR #30 (https://github.com/alxwlw/cc-switch-web/pull/30) открыт; DX-61 → In Review (комментарий со ссылкой). Чекбоксы Т1–Т4 закрыты, Notes заполнены (`docs(plans)` — WEB-03 T4).
  - Финальные гейты:
    - `cargo fmt --check` — чисто (RC=0);
    - `cargo clippy -- -D warnings` — чисто (RC=0);
    - `(umask 022; cargo test --no-fail-fast)` — lib `test result: FAILED. 4080 passed; 1 failed; 10 ignored` (baseline `commands::misc::tests::omp_search_paths_include_the_bun_global_bin`); все 50 интеграционных бинарников ok;
    - `cargo build --release --example server` — ок (`Finished release profile [optimized] target(s) in 3m 02s`);
    - `pnpm typecheck` — чисто; `pnpm format:check` — `All matched files use Prettier code style!`;
    - `prettier --check tests/**` — 14 файлов, все падают и на main (файлы фазы среди них нет; `HermesFormFields.test.tsx` проверен против main через `git show main:…`);
    - `pnpm test:unit` — `Test Files 2 failed | 315 passed (317)`, `Tests 3 failed | 3948 passed (3951)` (baseline: 2× UsageDashboard, rememberedChoice);
    - `pnpm build:renderer` / `pnpm build:web` — ок (`✓ built in 6.6s`).
  - Ревью:
    - Т1 approved r0 (inners verbatim, desktop wiring intact).
    - Т2 changes_requested r0 → approved r1 (no-CLI тесты проходили только на хосте с чистыми system dirs; fix: hermetic seam через env).
    - Т3 changes_requested r0 → approved r1 (подпись literal-Russian → `t(provider.form.agy.cliServerNote)`).
    - Т4: полный `test:unit` нашёл 3 красных фронта-теста от ungate Т3 (`WebProviderGating` ×2, `useOmoModelSource.web` ×1) — исправлены в `e83e5ef9`, полный прогон после — только baseline.
  - Решения/отклонения от плана:
    - reqwest-таймаут: текст `Request timed out: {e}` (`services/model_fetch.rs:149`) — содержит `timed out`, ветка `showFetchModelsError` та же, что на desktop; править текст сервиса не пришлось.
    - `CC_SWITCH_MODEL_FETCH_TIMEOUT_SECS` (`services/model_fetch.rs:65`): per-attempt timeout для `fetch_models`, default 15 s при unset/non-positive/unparsable — seam для тестов и ручной override; companion test-only seams `CC_SWITCH_AGY_SYSTEM_DIRS` / `CC_SWITCH_TOOL_SYSTEM_DIRS` перенаправляют жёстко зашитые CLI search dirs, чтобы no-CLI тесты были герметичны; строка-документация — в Notes ниже (оставлена из Т2).
    - Подпись у CLI-кнопки agy — через `t("provider.form.agy.cliServerNote", defaultValue "список берётся у CLI на сервере")` (`GeminiFormFields.tsx:216`); opencode идёт хуком без кнопки — подписи нет.
- **Backend**:
  - inners: `get_codex_oauth_models_inner` (`commands/codex_oauth.rs:101`), `get_xai_oauth_models_inner` (`commands/xai_oauth.rs:100`), `copilot_get_models_inner(state, Option<accountId>)` (`commands/copilot.rs:196`); команды — тонкие делегаты, desktop-путь не менялся.
  - Роуты — `web_api.rs:308-315`, хендлеры `models_fetch/models_agy/models_opencode/models_codex_oauth/models_xai_oauth/models_copilot/models_copilot_for_account` — `web_api.rs:2603-2691`. Ошибки — `fail(текст сервиса)` дословно.
  - Тесты — `src-tauri/tests/web_api_models.rs` (13 тестов: fetch ok / HTTP 401 / timeout / parse, agy/opencode CLI через PATH, codex без аккаунта); 13/13 зелёных.
- **Frontend**: 7 case — `adapter.ts:1142-1179`; тесты `tests/lib/modelFetchApi.test.ts` (URL/метод/body + envelope); `showFetchModelsError` на adapter-ошибке выбирает ветки 401/timeout. Гейты кнопок сняты (`ModelInputWithFetch`, `FetchedModelPicker`, 6 форм провайдеров, `useOmoModelSource.ts:61` убран `&& !isWeb`); строка :111 таблицы фазы 6 отмечена закрытой.
- **Известные ограничения**:
  - OAuth data plane вживую не проверен: в конфиге сервера нет Codex/xAI/Copilot OAuth-аккаунтов (аналогично фазе 28).
  - Живой проверки `/api/models/*` не было; покрыты интеграционными тестами со stub-сервером (fetch/401/timeout/parse) и фейковыми CLI через PATH.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Миграций нет.
  - Smoke без takeover: `POST /api/models/fetch` против реального провайдера; `GET /api/models/opencode` на хосте с opencode CLI; web: кнопка fetch-models в форме провайдера (401/timeout доходят тем же текстом), подпись у agy.
  - DX-61 → Done после merge.
