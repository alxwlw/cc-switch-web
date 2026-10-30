# Phase 30 — WEB-14 / DX-72: OpenCode live import и default model

Цель: import провайдеров из live-конфига OpenCode на сервере, список live provider IDs и default model (чтение и запись) работают в web.

Спека: § WEB-14. Оценка M (1–2 дн.). CLI-discovery моделей сделан в фазе 29.

Scope: `src-tauri/src/web_api.rs` (4 роута), `src-tauri/tests/web_api_opencode.rs`, `src/lib/api/adapter.ts` (4 case), тесты фронта.

Факты разведки:
- Фронт providers.ts: `import_opencode_providers_from_live` (l.225, без аргументов, → count), `get_opencode_live_provider_ids` (l.232), `get_opencode_default_model` (l.237, → `string|null`), `set_opencode_default_model {model}` (l.241). Вызовы: `ProviderList.tsx:135` (live ids), `:140` (default model), `:214` (import); `App.tsx:1125`; `ProviderForm.tsx:1105`; `useOmoModelSource.ts:96`; `useProviderActions.ts:370` (set; обёртка :345).
- Backend commands/provider.rs, без AppHandle и событий: l.1034 `import_opencode_providers_from_live(State<AppState>) -> usize` → `services::provider::import_opencode_providers_from_live(&AppState)` (live.rs:1247); l.1040 `get_opencode_live_provider_ids()` → ключи `opencode_config::get_providers()` (opencode_config.rs:191); l.1046 `get_opencode_default_model()` → `opencode_config::get_default_model()` (:589); l.1051 `set_opencode_default_model(model)` → `opencode_config::set_default_model(&str)` (:597): валидирует `<provider>/<id>`, переписывает только верхнеуровневый `model`.
- Тот же импорт выполняется при старте desktop (lib.rs:948) и входит в additive live-импорты headless bootstrap фазы 7.
- В web: `opencode` уже в `mvp_apps` (web_api.rs:152), общие `/api/:app/providers/*` работают; import, live IDs и default model отсутствуют.
- Тесты: `services/provider/opencode_tests.rs` (import, l.86-188), `services/provider/mod.rs:4252,4280`, `opencode_config.rs:1148` (`set_default_model_only_touches_top_level_model`); фронт — `ProviderList.test.tsx:345,791`, `ProviderForm.opencodeBuiltin.test.tsx` (MSW `http://tauri.local/…`). Default model не покрыт.

### Task 1: backend — роуты + тесты
- [x] `POST /api/opencode/import-live` → count; `GET /api/opencode/live-provider-ids`; `GET/PUT /api/opencode/default-model` (`{model}`). Команды без State вызывать напрямую.
- [x] `src-tauri/tests/web_api_opencode.rs` (temp-каталог OpenCode): подготовленный `opencode.json` с двумя провайдерами → live ids — оба, import-live → 2, повторный import не дублирует; default model round-trip; `model: "без-слеша"` → `{ok:false}` с текстом валидации; set меняет только `model`, остальной JSON байт-в-байт тот же.

### Task 2: frontend
- [x] 4 case в `mapCommand`.
- [x] Тесты: `tests/lib/opencodeApi.test.ts`; `ProviderList` в web показывает live-ids и default model через adapter.
- [x] Снять WEB-00-гейты (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-14-opencode-live`, PR; DX-72 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- `opencode.json` с комментариями/JSONC: set не должен терять комментарии, если desktop их сохраняет (проверить поведение `set_default_model` на фикстуре с комментарием).

## Notes for next phase

- **Статус до merge (2026-10-30)**: ветка `web-parity/web-14-opencode-live` (worktree `.worktrees/web-14-opencode`), PR → main (открывается в T3 STEP 4; URL — в T3-отчёте и комментарии DX-72); DX-72 → In Review + ссылка. Чекбоксы Т1–Т3 закрыты, Notes заполнены (`docs(plans)` — WEB-14 T3).
  - Финальные гейты на `df191a5f` (код; docs-коммит T3 сверху):
    - `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — чисто (RC=0);
    - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — чисто (RC=0);
    - `(umask 022; cargo test --manifest-path src-tauri/Cargo.toml --no-fail-fast)` — lib `test result: FAILED. 4080 passed; 1 failed; 10 ignored` (baseline `commands::misc::tests::omp_search_paths_include_the_bun_global_bin`, host-PATH-зависим, доказан на базе в фазе 29); все остальные интеграционные бинарники ok;
    - `cargo build --release --example server` — ок (`Finished release profile [optimized] target(s) in 2m 40s`; артефакт `src-tauri/target/release/examples/server`);
    - `pnpm typecheck` — чисто (RC=0); `pnpm format:check` — `All matched files use Prettier code style!` (RC=0);
    - `prettier --check tests/**` — 14 файлов с предупреждениями, все с main; файлы фазы (`tests/lib/opencodeApi.test.ts`, `tests/components/ProviderList.test.tsx`, `tests/components/WebProviderGating.test.tsx`, `tests/hooks/useOmoModelSource.web.test.tsx`, `tests/hooks/useProviderActions.test.tsx`, `tests/components/ProviderForm.opencodeBuiltin.test.tsx`) среди них нет (`git diff --name-only aa578f91..HEAD` vs список — пересечений ноль);
    - `pnpm test:unit` — `Test Files 2 failed | 316 passed (318)`, `Tests 3 failed | 3957 passed (3960)` (baseline: 2× UsageDashboard — `draws the request log…`, `wires the tabs…` — + rememberedChoice `keeps working when storage is unavailable`);
    - `pnpm build:renderer` — ок (`✓ built in 6.80s`); `pnpm build:web` — ок (`✓ built in 7.97s`).
  - Ревью:
    - Т1 approved r0 (routes/tests verified, 500-vs-400 per convention).
    - Т2 approved r0 (137/6 green, probes red-restored, claude-desktop gate intact).
- **Backend**:
  - Роуты — `web_api.rs:434-442`, хендлеры `opencode_import_live/opencode_live_provider_ids/opencode_default_model_get/opencode_default_model_set` — `web_api.rs:3516-3552` (форма с openclaw-аналогов; `import` зовёт сервис с `&state` напрямую, остальные — команды/`opencode_config` без State; ошибки — `fail()`/текст сервиса).
  - Тесты — `src-tauri/tests/web_api_opencode.rs` (6 тестов: live ids — оба провайдера; import-live → 2, повтор без дублей; default round-trip + missing→null; `без-слеша` → `{ok:false}` с текстом `OpenCode model must be "<provider>/<model>"`; write с комментарием — комментарии и остальной JSON нетронуты); 6/6 зелёных.
- **Frontend**: 4 case — `adapter.ts:910-920` (`POST /api/opencode/import-live`, `GET /api/opencode/live-provider-ids`, `GET/PUT /api/opencode/default-model`, PUT-тело `{model}`); тесты `tests/lib/opencodeApi.test.ts` (URL/метод/body + envelope) + web-покрытие live-ids/default-model (`ProviderList.test.tsx`, `useProviderActions.test.tsx`, `useOmoModelSource.web.test.tsx`, `ProviderForm.opencodeBuiltin.test.tsx`, `WebProviderGating.test.tsx`). Гейты сняты только для opencode (`ProviderList.tsx` — `WEB_LIVE_MEMBERSHIP_APPS=[]`, `WEB_LIVE_IMPORT_APPS=["claude-desktop"]`; `App.tsx:1118+` — live-ids без `isWeb`; `ProviderForm.tsx`, `useOmoModelSource.ts` — убран `&& !isWeb`); claude-desktop в `WEB_LIVE_IMPORT_APPS` нетронут (WEB-19, фаза 32). Строки 114/115 таблицы фазы 6 отмечены закрытыми для opencode (`phase-6.md`).
- **Отклонения от плана**: нет beyond Rulings T1–T3 (journal p30).
- **Известные ограничения**:
  - Живой проверки `/api/opencode/*` на сервере не было; покрыты harness-тестами с temp-каталогом OpenCode (без сети).
  - Default-model UI идёт через `presentation.ts` (`membershipUnknown` без setDefault) — chip/меню появляются сами после снятия opencode из membership-логики; отдельного UI-теста chip нет.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Миграций не ожидается, но VERIFY перед рестартом.
  - Smoke без takeover: `POST /api/opencode/import-live` → count; `GET /api/opencode/live-provider-ids`; default-model round-trip в web (GET/PUT); live ids + default model видны в web UI.
  - DX-72 → Done после merge.
