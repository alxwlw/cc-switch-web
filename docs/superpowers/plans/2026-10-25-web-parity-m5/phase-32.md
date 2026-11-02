# Phase 32 — WEB-19 / DX-77: Claude Desktop integration и official cards

Цель: в web работают статус Claude Desktop, default routes, import провайдеров из Claude, создание официальных seed-провайдеров (Claude Desktop, Codex, GrokBuild) и route mode Claude Desktop через local proxy. UI прямо говорит, что речь об установке Claude Desktop на сервере, а не на машине пользователя.

Спека: § WEB-19. Оценка M (3–5 дн.). Route mode опирается на фазы 27–28.

⚠️ Уточнение к спеке: от WEB-01 зависит только полный route mode. Шесть команд — DB-only или чтение файлов; на диск Claude Desktop пишет не эти команды, а apply провайдера (switch/takeover/route resync) через `write_direct`/`apply_provider`, уже доступные в web.

Scope: `src-tauri/src/web_api.rs` (6 роутов), `src-tauri/tests/web_api_claude_desktop.rs`, `src/lib/api/adapter.ts` (6 case), `src/components/apps/AppsPage.tsx`, `src/components/providers/mode/*` (server-host подписи), тесты фронта.

Факты разведки:
- Фронт providers.ts:160-180 (без аргументов): `import_claude_desktop_providers_from_claude`, `ensure_claude_desktop_official_provider`, `ensure_codex_official_provider`, `ensure_grokbuild_official_provider`, `get_claude_desktop_status`, `get_claude_desktop_default_routes`; тип `ClaudeDesktopStatus` (providers.ts:~212-229). Вызовы: `AppsPage.tsx:182`, `useDesktopConfigMessages.ts:14`, `ClaudeDesktopProviderForm.tsx:322`, `ProviderList.tsx:223`, `lib/query/mutations.ts:47-58`.
- Backend commands/provider.rs, без AppHandle (регистрация lib.rs:1529-1534): :279 `get_claude_desktop_status(state)` (async) → `claude_desktop_config::get_status(&Database, proxy_running)` (claude_desktop_config.rs:215), `proxy_running = state.proxy_service.is_running()`; :288 `get_claude_desktop_default_routes()` → `default_proxy_routes()` (:305); :294 `import_claude_desktop_providers_from_claude(state) -> usize` — копирует Claude-провайдеров в таблицу claude-desktop, затем `ensure_official_seed_by_id`; :348/:359/:367 `ensure_{claude_desktop|codex|grokbuild}_official_provider(state) -> bool` → `Database::ensure_official_seed_by_id` (dao/providers.rs:846).
- Файлы при apply: `<Claude dir>/claude_desktop_config.json`, `<Claude-3p dir>/claude_desktop_config.json`, `<Claude-3p>/configLibrary/00000000-0000-4000-8000-000000157210.json` (профиль «CC Switch», PROFILE_ID claude_desktop_config.rs:22), `<Claude-3p>/configLibrary/_meta.json`. Gateway token — DB-настройка `claude_desktop_gateway_token`; префикс пути proxy — `/claude-desktop` (:28-29).
- Linux поддерживается: `is_supported_platform()` (:1487) — macOS, Windows, Linux; на Linux — `$XDG_CONFIG_HOME` (если абсолютный) или `~/.config`, затем `{Claude, Claude-3p}`, во Flatpak всегда `~/.config` (:1525-1549). `installed` определяется по каталогу данных (`detect_installed`, :1471).
- Семантика web: файлы пишутся на сервере, а клиент Claude Desktop обычно работает на машине пользователя — без подписи это ловушка.
- DTO `ClaudeDesktopStatus` (:102), `ClaudeDesktopDefaultRoute` (:48-52, `supports1m`) — camelCase.
- Тесты: in-file `mod tests` claude_desktop_config.rs (:1629+, включая `detect_installed_reads_data_dir_or_app_bundle` и linux config dir).

### Task 1: backend — роуты + тесты
- [x] `GET /api/claude-desktop/status`, `GET /api/claude-desktop/default-routes`, `POST /api/claude-desktop/import-from-claude`, `POST /api/official-seeds/:app/ensure` (`app ∈ {claude-desktop, codex, grokbuild}`, иначе 400).
- [x] `src-tauri/tests/web_api_claude_desktop.rs` (temp HOME, `XDG_CONFIG_HOME` → temp): без каталога Claude → `installed:false`, `supported:true`; с каталогом → `installed:true`; status при запущенном proxy (фаза 27) → `proxyRunning:true`; import копирует N Claude-провайдеров и создаёт официальный seed; ensure — идемпотентен (`true`, затем `false`); takeover Claude Desktop в route mode через роуты фазы 27 → профиль `configLibrary/…157210.json` создан с proxy URL и префиксом `/claude-desktop`, выход из takeover → файлы восстановлены.

### Task 2: frontend
- [x] 6 case в `mapCommand`.
- [x] `AppsPage`/карточки Claude Desktop в web: подпись «Claude Desktop на сервере cc-switch-web»; при `installed:false` — «Claude Desktop не найден на сервере; настройки применятся, когда он появится там», без тревожного вида.
- [x] Тесты: `tests/lib/claudeDesktopApi.test.ts`; `AppsPage` в web показывает server-host подпись и состояние `installed:false`.
- [x] Снять WEB-00-гейты Claude Desktop (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-19-claude-desktop`, PR; DX-77 → In Review + ссылка (Plane обновляет контроллер). Закрыть чекбоксы, Notes.

Риски для ревью:
- Route mode Claude Desktop ведёт клиента на `127.0.0.1:<порт proxy>` сервера: если Claude Desktop стоит на другой машине, он туда не достучится. UI route mode в web должен это сказать (и подсказать про client host из фазы 27), а тест — зафиксировать, что URL строится из настроенного client host.

## Notes for next phase

- **Закрыта 2026-11-02**: PR #33 merged `a3e77441` → main; live на `cc-switch-web.service` с 21:25; DX-77 Done.
  - Server и `dist-web` пересобраны из main. БД забэкаплена в `~/.cc-switch-web/backups-predeploy-p32-20261102-212547/`. Миграций нет, `user_version` 23.
  - Smoke: консоль 200; `claude-desktop/status` → `supported:true, installed:false`; `default-routes` → 4; `official-seeds/nope/ensure` → 400; Apps page показывает подпись «Claude Desktop on the cc-switch-web server» и строку not found; паник в журнале нет. Import и ensure на живой БД не запускались, takeover Claude Desktop вживую не проверялся.
- **Статус до merge (2026-11-02)**: ветка `web-parity/web-19-claude-desktop` (worktree `.worktrees/web-19-claude-desktop`), PR → main (URL — в T3-отчёте и комментарии DX-77); DX-77 → In Review + ссылка (контроллер). Чекбоксы Т1–Т3 закрыты, Notes заполнены (`docs(plans)` — WEB-19 T3). Merge делает владелец.
  - Финальные гейты на `2f5d8171` (код; docs-коммит T3 сверху):
    - `cargo fmt --check --manifest-path src-tauri/Cargo.toml` — чисто (RC=0);
    - `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings` — чисто (`Finished dev profile [unoptimized + debuginfo] target(s) in 1.24s`, RC=0);
    - `(umask 022; cargo test --no-fail-fast --manifest-path src-tauri/Cargo.toml)` — lib `test result: FAILED. 4091 passed; 1 failed; 10 ignored` (единственный fail — baseline `commands::misc::tests::omp_search_paths_include_the_bun_global_bin`, host-PATH-зависим), остальные 55 результатов `test result: ok`, в т.ч. `web_api_claude_desktop` — `ok. 7 passed`;
    - `cargo build --release --manifest-path src-tauri/Cargo.toml --example server` — ок (`Finished release profile [optimized] target(s) in 3m 20s`; артефакт `src-tauri/target/release/examples/server`, 29641816 байт);
    - `pnpm typecheck` — чисто (RC=0); `pnpm format:check` — `All matched files use Prettier code style!` (RC=0);
    - `pnpm test:unit` — `Test Files 2 failed | 320 passed (322)`, `Tests 3 failed | 3990 passed (3993)` (ровно baseline: 2× UsageDashboard — `draws the request log…`, `wires the tabs…` — + rememberedChoice `keeps working when storage is unavailable`; `App.test.tsx` в финальном прогоне зелёный);
    - `pnpm build:renderer` — ок (`✓ built in 7.55s`); `pnpm build:web` — ок (`✓ built in 6.99s`).
  - Живая проверка ветки (release `examples/server` на 127.0.0.1:18799, `CC_SWITCH_CONFIG_DIR` = sqlite-backup копия БД, temp HOME/`XDG_CONFIG_HOME`, `WEB_DIST` ветки; `cc-switch-web.service`, реальные `~/.cc-switch-web` и HOME не тронуты; сервер остановлен, порт свободен):
    - curl: `GET /api/claude-desktop/status` → `supported:true, installed:false` (пути профиля — под temp HOME), после `mkdir <HOME>/.config/Claude` → `installed:true`; `GET /api/claude-desktop/default-routes` → 4 маршрута camelCase (`routeId`, `envKey`, `supports1m`); `POST /api/claude-desktop/import-from-claude` → `data:2`, повтор → `0`; `POST /api/official-seeds/nope/ensure` → 400 `official seed is only available for claude-desktop, codex, grokbuild: nope`; `POST /api/official-seeds/{codex,grokbuild,claude-desktop}/ensure` → `data:false` (на копии боевой БД seed'ы уже есть; переход `true`→`false` покрыт тестом `ensure_official_seed_is_idempotent_for_each_app`, не живым прогоном);
    - UI (Playwright, страница Apps при `installed:false`): под строкой Claude Desktop — «Claude Desktop on the cc-switch-web server · Claude Desktop was not found on the server; settings will apply once it appears there.» в секции «Not installed», статус «Not connected»; состояние `installed:true` в UI не открывали (покрыто `AppsPage.test.tsx`). Takeover/apply на этой проверке не запускались.
  - Ревью:
    - Т1 approved r0 (2 NIT, оба оставлены открытыми — см. ограничения).
    - Т2 changes_requested r0 (F1 DP-515: `AppsPage.tsx` `isError`-ветка сворачивала сгруппированный список при неудачном refetch с сохранёнными данными, плюс менялось desktop-поведение pending/error) → approved r1 (fix r1 — `rowInstalled` возвращает `null` только при `desktopStatus === undefined`, в desktop pending/error как на main; +4 теста, 3 пробы красные; survivor `AppsPage.tsx:199` equivalent).
- **Backend**:
  - Роуты — `web_api.rs:552-564` (регистрация рядом с proxy-блоком), хендлеры `:4534` `claude_desktop_status` (`proxy_service.is_running()` + `claude_desktop_config::get_status`, через `blocking_api`), `:4543` `claude_desktop_default_routes`, `:4547` `claude_desktop_import_from_claude`, `:4557` `ensure_official_seed` (`app ∈ {claude-desktop, codex, grokbuild}`, иначе `bad_request` 400 без записи). Тела ответов — стандартный `{ok,data}`.
  - `commands/provider.rs:294-302`: тело import вынесено в handle-free `import_claude_desktop_providers_from_claude_db(&Database)`, Tauri-команда — однострочный вызов; desktop-поведение не менялось (68 lib-тестов `claude_desktop` зелёные).
  - Тесты — `src-tauri/tests/web_api_claude_desktop.rs` (7 тестов, `#[serial]`): status без/с каталогом Claude, `proxyRunning` следует за start/stop, default-routes camelCase, import (direct + route скопированы, skipped, pre-existing сохранён, официальный seed добавлен, повтор → 0), ensure идемпотентен для трёх app + 400 для `claude`/`gemini`/`claude_desktop`/`nope`, route mode через `PUT /api/proxy/takeover/claude-desktop`: профиль `configLibrary/…157210.json` с URL `http://<client host>:<port>/claude-desktop` (риск плана: URL из настроенного client host, смена host перезаписывает, пустой host → `127.0.0.1`), выход → `deploymentMode:"1p"`, connection-поля профиля очищены, запись `_meta.json` снята; мутационные пробы T1 — 7 групп красные.
- **Frontend**:
  - `adapter.ts:1279-1293` — 6 case в `mapCommand` (GET status, GET default-routes, POST import-from-claude, POST `official-seeds/{claude-desktop,codex,grokbuild}/ensure`); тест — `tests/lib/claudeDesktopApi.test.ts` (7).
  - Гейты сняты: статус в `AppsPage.tsx:183-212` (`rowInstalled`, `:193-199`), `useDesktopConfigMessages.ts` (опрос 5 с в web), `ClaudeDesktopProviderForm.tsx` (default-routes, official-пресет, model-mapping), `GrokBuildProviderForm.tsx` (official-пресет), `ProviderList.tsx`/`ProviderEmptyState.tsx` (`importUnavailable`, `WEB_LIVE_IMPORT_APPS` удалены); строки 114/116 таблицы фазы 6 и три примечания закрыты (`phase-6.md`).
  - Server-host подписи (через `t()`, 5 локалей en/zh/zh-TW/ja/tr): `appsPage.desktopServerNote` и `appsPage.desktopNotFoundOnServer` (`AppsPage.tsx:361-366`, без тревожного вида), `web.desktopRouteHost` — в `ModeDialog.tsx:228` (вход в route/stack) и `SwitchModePanel.tsx:621` (панель; текст отсылает к Настройки → Локальная маршрутизация, client host фазы 27); pending/error статуса → плоский список без групп.
  - Тесты — `AppsPage.test.tsx` (71), `ModeDialog.test.tsx` (10), `ClaudeDesktopProviderForm`/`GrokBuildProviderForm`/`ProviderList`/`WebProviderGating`/`webSilencedQueries` — «скрыто в web» превращены в «работает в web».
- **Отклонения от плана**: нет beyond Rulings журнала (`.claude/plans/2026-10-25-web-parity-m5-p32/journal.md`): (T1) import-логика вынесена из команды в handle-free функцию по сквозному правилу, URL route-профиля строится из client host (тест фиксирует); (T2 fix r1) desktop-поведение `AppsPage` не меняем, подпись `web.desktopRouteHost` остаётся и в панели, и в диалоге входа. Фактические поправки брифа: `import_claude_desktop_providers_from_live` не существует (реальная — `…_from_claude`), роут ensure сделан отдельным match в `web_api.rs`, а не общим helper'ом; снят дополнительный гейт `ProviderEmptyState.importUnavailable`.
- **Известные ограничения**:
  - Подпись статическая: она не показывает фактически настроенный client host, только отсылает к настройке (URL с host'ом строит backend, это закреплено тестом T1).
  - Файл `web_api_claude_desktop.rs` — `#![cfg(not(target_os = "windows"))]`, как `web_api_proxy.rs`: Windows-путь (`LOCALAPPDATA`) не проверяется; assertion `installed:false` ещё и `cfg(not(macos))`.
  - 2 NIT Т1 оставлены открытыми: (1) `web_api_claude_desktop.rs:332` `error.contains(app)` для `app = "claude"` проходит и на «claude-desktop» из того же сообщения (остальные три app и статус 400 держат проверку); (2) соответствие app → seed продублировано в `web_api.rs:4557+` и в `commands/provider.rs` (три `ensure_*_official_provider`) — разойдутся только при добавлении четвёртого seed'а в одну сторону.
  - Web пишет файлы Claude Desktop на сервере; если клиент на другой машине, нужно задать client host (фаза 27) — UI лишь подсказывает.
  - Takeover/apply Claude Desktop на живом сервисе не прогонялись (пишут под реальный HOME) — только тестами.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust и фронт менялись). Миграций нет. Перед рестартом — бэкап БД (`~/.cc-switch-web/backups-predeploy-p32-<дата>/`).
  - Smoke без takeover: `GET /api/claude-desktop/status` (`supported:true`), `default-routes`, Apps в web показывает подпись «на сервере» и состояние `installed`; `POST /api/official-seeds/nope/ensure` → 400. Import/ensure на живой БД — осознанно (создают seed-карточки).
  - DX-77 → Done после merge. Отметка «Фаза 32» в индексе — первым коммитом ветки следующей фазы.
