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
- [ ] `POST /api/opencode/import-live` → count; `GET /api/opencode/live-provider-ids`; `GET/PUT /api/opencode/default-model` (`{model}`). Команды без State вызывать напрямую.
- [ ] `src-tauri/tests/web_api_opencode.rs` (temp-каталог OpenCode): подготовленный `opencode.json` с двумя провайдерами → live ids — оба, import-live → 2, повторный import не дублирует; default model round-trip; `model: "без-слеша"` → `{ok:false}` с текстом валидации; set меняет только `model`, остальной JSON байт-в-байт тот же.

### Task 2: frontend
- [ ] 4 case в `mapCommand`.
- [ ] Тесты: `tests/lib/opencodeApi.test.ts`; `ProviderList` в web показывает live-ids и default model через adapter.
- [ ] Снять WEB-00-гейты (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-14-opencode-live`, PR; DX-72 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- `opencode.json` с комментариями/JSONC: set не должен терять комментарии, если desktop их сохраняет (проверить поведение `set_default_model` на фикстуре с комментарием).

## Notes for next phase

- (заполнить по итогам)
