# Phase 5 — WEB-18 / DX-76: OMO / OMO Slim live config

Цель: read local file, current provider id, disable current — работают в web для обоих вариантов (omo, omo-slim) через тонкие HTTP-wrappers; тесты переключения live config на серверном хосте.

Спека: § WEB-18. Оценка M (1–2 дн.).

Scope: `src-tauri/src/web_api.rs` (роуты), `src-tauri/tests/web_api_omo.rs`, `src/lib/api/adapter.ts` (6 case), `tests/lib/omoApi.test.ts`. Query-хуки (`src/lib/query/omo.ts`) и UI не меняем.

Факты разведки:
- Фронт omo.ts (17 строк): `omoApi.{readLocalFile,getCurrentOmoProviderId,disableCurrentOmo}` → `read_omo_local_file`, `get_current_omo_provider_id`, `disable_current_omo`; `omoSlimApi.{readLocalFile,getCurrentProviderId,disableCurrent}` → `read_omo_slim_local_file`, `get_current_omo_slim_provider_id`, `disable_current_omo_slim`. Все 6 не в `mapCommand` → `WebUnsupportedError`.
- Rust handle-free (commands/omo.rs:7-73): read → `OmoService::read_local_file(&STANDARD|&SLIM)` (services/omo.rs:575); current → `state.db.get_current_omo_provider("opencode", "omo"|"omo-slim")` :13-19; disable → итерация `db.get_all_providers("opencode")` + `db.clear_omo_provider_current(...)` + `OmoService::delete_config_file(variant)` :22-37. `OmoLocalFileData` — camelCase (services/omo.rs:15).
- OMO-роутов в web_api нет.

### Task 1: backend — роуты + тесты
- [x] `GET /api/omo/:variant/local-file`, `GET /api/omo/:variant/current-provider`, `POST /api/omo/:variant/disable`, где `variant ∈ {omo, omo-slim}` (маппинг на OmoVariant; неизвестный вариант → 400). Хендлеры: State + spawn_blocking где файловые операции.
- [x] `src-tauri/tests/web_api_omo.rs`: при подготовленном live-файле (temp HOME) read-local-file возвращает данные; current-provider — из seeded DB; disable — очищает current в DB и удаляет config-файл; idempotent повторный disable; неизвестный variant → 400.
- [x] nextest зелёный — nextest не установлен, гонялся `cargo test --no-fail-fast`: 4256 passed / 3 baseline; web_api_omo 8/8. Коммит `2ecdf57b`, review r0 approved.

### Task 2: frontend — adapter cases + тест
- [x] 6 case в `mapCommand` (omo/omo-slim × read/current/disable) на новые URL.
- [x] `tests/lib/omoApi.test.ts`: каждый метод — ожидаемый URL/метод; `pnpm typecheck && pnpm test:unit` зелёные; `tests/utils/omoConfig.test.ts` и `tests/components/OmoFormFields.*.test.ts` не сломаны. Коммит `e18414fd`, review r0 approved.

### Task 3: гейты, PR, Plane
- [x] полный набор гейтов: fmt/clippy/`build --release --example server` ok; cargo test 4254 passed / 5 failed (3 baseline + 2 flaky `forwarder::bookkeeping`, отдельно 16/16); typecheck/format:check/build:renderer/build:web ok; test:unit 3334 passed / 3 baseline.
- [x] ветка `web-parity/web-18-omo-live-config`, PR #6; DX-76 → In Review + комментарий со ссылкой. NIT ревью Task 1 (мёртвая подготовка в тесте disable) — `06e15025`.

## Notes for next phase

- **Статус 2026-10-25**: PR #6 открыт (https://github.com/alxwlw/cc-switch-web/pull/6), DX-76 In Review. Worktree `.worktrees/web-18` (node_modules — симлинк, удалить перед `git worktree remove`). В PR вошёл docs-коммит `d199418c` (закрытие фазы 4) — он был только в локальном main. После merge — `post-merge`: деплой на `cc-switch-web.service` (не забыть `cp cc-switch-web cc-switch-web.prev` перед install — см. Notes фазы 4), DX-76 Done, отметить фазу в индексе.
- **Вынос из команд**: `OmoService::current_provider_id` / `OmoService::disable_current` (`services/omo.rs:577,583`) — handle-free, принимают `&Database` + `&OmoVariant`; Tauri-команды обоих вариантов теперь делегируют им. Роуты `web_api.rs:144-149`, разбор варианта — `omo_variant()` (`:1097`), неизвестный → 400 с телом, закреплённым в тесте.
- **Семантика DB**: `set_omo_provider_current` сбрасывает current у противоположной категории (`providers.rs:563`) — два варианта никогда не current одновременно; `clear_omo_provider_current` фильтрует по своей категории. Поэтому в тесте disable «второй вариант не тронут» проверяется только по его файлу.
- **Пустой current**: `current-provider` без current-провайдера отдаёт `""` (как desktop), не `null`. `disable` отдаёт `null` (`Result<(), String>`); единственный потребитель `useDisableCurrent` значение не читает.
- **Фазе 6 (WEB-00, gating)**: OMO/OMO Slim controls в web теперь работают — гейтить их `isWeb` не нужно; в таблицу инвентаризации вносить как закрытые фазой 5.
- **Гейты под нагрузкой**: `pnpm test:unit` параллельно с cargo build дал 13 падений (App integration, AppsPage, ProviderList, SwitchModePanel — таймауты); без нагрузки — 3 baseline. Фронтовые тесты не гонять одновременно с Rust-сборкой.
- Имплементер Task 1 оборвался на истёкшем логине API посреди гейтов и был возобновлён SendMessage'ем с сохранённым контекстом — коммит и пробы уцелели. Его echo с `->500` оставил в `src-tauri/` мусорные файлы `500`/`STANDARD` (удалены, не коммитились): в брифах просить кавычки вокруг строк с `>`.
