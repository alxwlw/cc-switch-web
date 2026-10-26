# Phase 12 — WEB-24 / DX-82: Rectifier/optimizer и log config

Цель: настройки rectifier/optimizer и уровень логирования меняются из web. Вместо «показать лог в файловом менеджере» web-консоль показывает хвост лог-файла сервера и даёт его скачать.

Спека: § WEB-24. Оценка M (2–3 дн.).

Scope: `src-tauri/src/web_api.rs` (6 роутов конфигурации + 2 роута логов), `src-tauri/examples/server.rs` (лог в файл), `src-tauri/tests/web_api_settings_config.rs`, `src/lib/api/adapter.ts`, `src/components/settings/SettingsPage.tsx:312-321,511-515` (кнопка лога в web), новый компонент просмотра лога, тесты фронта.

Факты разведки:
- Фронт settings.ts:328-349: `get_rectifier_config` / `set_rectifier_config {config}`, `get_optimizer_config` / `set_optimizer_config {config}`, `get_log_config` / `set_log_config {config}`. UI: `RectifierConfigPanel.tsx:29-34` (загрузка обоих на mount), `:35-65` (оптимистичный setState, откат при ошибке); `LogConfigPanel.tsx:25-47` — тот же паттерн. Монтируются в `RoutingSection.tsx:455` и `SettingsPage.tsx:519`. React Query не используется.
- `get/set_copilot_optimizer_config` (settings.rs:~765) есть в backend, на фронте не вызываются — не переносим.
- Backend commands/settings.rs — все `async fn(state: State<AppState>[, config]) -> bool`, прямые DAO-вызовы: `get_rectifier_config` :731, `set_rectifier_config` :739, `get_optimizer_config` :751, `set_optimizer_config` :759, `get_log_config` :791, `set_log_config` :799 (DAO database/dao/settings.rs:244-322). `set_log_config` дополнительно вызывает `log::set_max_level(config.to_level_filter())`.
- DTO `RectifierConfig`, `OptimizerConfig`, `LogConfig` (proxy/types.rs:195,250,337) — camelCase.
- Логи: desktop пишет файл через `tauri_plugin_log` (lib.rs:529-565) в `panic_hook::get_log_dir()/cc-switch.log` (ротация 20 МиБ × 4). Headless-сервер логирует только в stderr через `env_logger` с фильтром `info` по умолчанию (server.rs:44) — файла нет. Reveal — `reveal_app_log_file` (commands/config.rs:~307-322, opener); UI — `SettingsPage.tsx:312-321` и кнопка `:511-515`.
- ⚠️ `log::set_max_level(Debug)` не включит debug, пока фильтр `env_logger` стоит на `info`: фильтр env_logger режет раньше.

### Task 1: лог-файл сервера
- [x] server.rs: `env_logger` пишет одновременно в stderr и в `<get_log_dir()>/cc-switch-web.log` (tee-writer через `Target::Pipe`). При старте файл больше 20 МиБ переименовывается в `.1` (одна архивная копия).
- [x] Фильтр `env_logger` — из `RUST_LOG`, иначе `trace`; эффективный уровень задаёт `log::set_max_level` из сохранённого `LogConfig` (bootstrap фазы 7 уже применяет его при старте). Так `set_log_config` в web действительно меняет детализацию.

### Task 2: backend — роуты + тесты
- [x] `GET/PUT /api/settings/rectifier`, `GET/PUT /api/settings/optimizer`, `GET/PUT /api/settings/log` (body `{config}`), ответы — как у команд.
- [x] `GET /api/logs/tail?lines=N` (N по умолчанию 500, максимум 5000) → `{path, lines: string[]}`; `GET /api/logs/download` → файл как `text/plain` с `Content-Disposition: attachment`. Файла нет → `{ok:false}` с путём, где он ожидался.
- [x] `src-tauri/tests/web_api_settings_config.rs`: round-trip для трёх конфигов; `PUT log {level:"debug"}` → `log::max_level() == Debug`; tail на подготовленном файле из 10 строк с `lines=3` → последние 3; `lines=100000` обрезается до 5000; download отдаёт заголовок attachment.

### Task 3: frontend
- [x] 6 case в `mapCommand` для конфигов; `reveal_app_log_file` в web не мапить.
- [x] `SettingsPage`: в web кнопка «открыть лог» открывает просмотр хвоста (компонент `ServerLogViewer`: путь на сервере, последние строки, кнопка «скачать» → `/api/logs/download`).
- [x] Тесты: панели rectifier/log в web сохраняют через adapter (mock fetch); viewer показывает строки и ссылку на download.
- [x] Снять WEB-00-гейты с этих панелей и кнопки лога (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`; на agentbox проверить, что после `PUT log debug` в файле появляются debug-строки.
- [x] Ветка `web-parity/web-24-rectifier-log`, PR; DX-82 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Лог может содержать секреты (ключи в URL, токены): tail и download отдаются любому участнику tailnet, как и остальной API. Проверить, что маскирование `http_client::mask_url` и прочее, что desktop применяет при логировании, работает и в сервере; в Notes зафиксировать модель доверия.

## Notes for next phase

- **Закрыта 2026-10-26**: PR #13 merged `d94f812b` → main; live на `cc-switch-web.service`.
  - Server пересобран из main (`cargo build --release --example server` → `~/.local/bin/cc-switch-web`); бинарник фазы 11 сохранён в `cc-switch-web.prev`. `pnpm build:web` выполнен в main-checkout.
  - С подтверждения владельца из `~/.config/systemd/user/cc-switch-web.service` убрана строка `Environment=RUST_LOG=info`, затем `daemon-reload` + restart. Миграций нет.
  - Проверено: unit active, WARN/ERROR в journal нет. `/api/health`, `/api/init-status`, `GET /api/settings/{rectifier,optimizer,log}`, `/api/logs/tail` → 200; download отдаёт `attachment`; отдаваемый `index.html` совпадает со сборкой.
  - Лог живого сервиса — `~/.cc-switch-web/logs/cc-switch-web.log`. На живом сервисе `PUT log error` гасит info-строку «日志配置已更新», `PUT info` её возвращает. Уровень оставлен `info`.
  - DX-82 Done (комментарий со ссылками file:line). Worktree `.worktrees/web-24`, локальная и remote ветки удалены; ledgers прогона перенесены в `.claude/plans/2026-10-25-web-parity-m5-p12` main-checkout.
- **Статус до merge (2026-10-26)**: ветка `web-parity/web-24-rectifier-log` (worktree `.worktrees/web-24`), PR #13 (https://github.com/alxwlw/cc-switch-web/pull/13) открыт, DX-82 In Review (комментарий со ссылкой). Т1 одобрена с r1: в r0 был SHOULD-FIX DP-256 — пустой `RUST_LOG=` три места читали по-разному. Закрыто в `cf64f67e`. Т2–Т4 одобрены с r0. NIT 1/2/4 из ревью Т3 закрыты в `3108e3ea`, NIT DP-017 из ревью Т2 — в `e2744bae`. Финальные гейты на `3108e3ea`:
  - fmt и clippy `-D warnings` чисто; `cargo build --release --example server` ок.
  - `cargo test --no-fail-fast` (nextest не установлен): 4330 passed / 6 failed. Baseline: omp_search_paths, 2× file_modes::switch_*. Ещё 3× `forwarder::tests::bookkeeping::*` — нестабильны и на merge-base, ветка proxy не трогает.
  - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` ок; `test:unit` 3704 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - В PR едет неподтолкнутый `0a232463` (docs закрытия фазы 11 с локального main).
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся; в `Cargo.toml` добавлена фича tokio `fs`). Перед install — `cp cc-switch-web cc-switch-web.prev`. Миграций БД нет.
  - **Юнит `cc-switch-web.service` задаёт `Environment=RUST_LOG=info`.** Явный `RUST_LOG` побеждает сохранённый LogConfig — так сделано намеренно ещё в фазе 7. Поэтому под этим юнитом уровень debug из web сохраняется, но не действует (проверено smoke на временном экземпляре). Чтобы настройка работала, строку из юнита убрать; это правка живого сервиса — с подтверждения владельца.
  - Лог живого сервиса будет в `~/.cc-switch-web/logs/cc-switch-web.log` (`CC_SWITCH_CONFIG_DIR`). DX-82 → Done.
- **Лог сервера** (`src-tauri/src/server_log.rs`, вызывается из `examples/server.rs`):
  - Путь — `get_app_config_dir()/logs/cc-switch-web.log`, не `panic_hook::get_log_dir()`: тот приватный и по умолчанию смотрит в `~/.cc-switch`.
  - Писатель `TeeWriter` (stderr + файл); при старте файл больше 20 МиБ уходит в `.1`. Во время работы ротации нет: файл растёт до перезапуска.
  - Фильтр: непустой `RUST_LOG`, иначе `trace` с потолком info для hyper, hyper_util, h2, reqwest, rustls, tower, tower_http, tokio_util, rusqlite, mio. Сразу после init max_level = info, дальше bootstrap применяет сохранённый LogConfig.
  - Предикат «RUST_LOG задан» — один на всех, `server_log::explicit_rust_log()` (trim, непустой). Его используют init-фильтр, init-потолок и `bootstrap.rs` (`run_headless_bootstrap_with`).
  - axum и tungstenite не ограничены: в smoke на debug они ничего не писали. Под нагрузкой от них может появиться шум — тогда добавить в список.
- **Роуты**:
  - `GET/PUT /api/settings/{rectifier,optimizer,log}`, body `{config}`. Desktop-команда `set_log_config` держит `State`, поэтому логика вынесена в `ConfigService::set_log_config` (сохранить + `log::set_max_level` + info-строка); команда переключена на него.
  - `GET /api/logs/tail?lines=N`: по умолчанию 500, clamp 5000; 0, отрицательное или нечисловое → 400. Файл читается с конца кусками по 64 КиБ с потолком 8 МиБ. NIT, не правили: строка длиннее потолка даёт пустой список без признака обрезки.
  - `GET /api/logs/download` стримит файл чанками, без Content-Length.
  - Нет файла → **404** `{ok:false,error,path}`, а не 500 из `fail()`. Это в рамках конвенции: 400/503 с той же формой уже есть. Адаптер бросает `payload.error`, путь в нём уже есть.
- **Frontend**:
  - Хвост лога — web-only псевдокоманда `get_server_log_tail` в `mapCommand` + `settingsApi.getServerLogTail`; монтируется только под `isWeb`.
  - Скачивание — обычный `<a href="/api/logs/download" download>`: адаптер ходит на тот же origin без auth-заголовков. Если в web появится авторизация заголовком, перейти на fetch→blob. Ссылка скрыта, пока tail отвечает ошибкой.
  - **Гейтинг**: WEB-00 скрывал секции `routing` и `data` целиком в `navigation.ts`; теперь скрыта только `network`. В web `routing` рендерит только `RectifierBlock`, а `data` — только блок лога; остальное (proxy/failover, import/export, backup, WebDAV/S3, data dir) остаётся под `!isWeb` в `SettingsPage.tsx`/`RoutingSection.tsx`. Фазы WEB-01, WEB-09/10/11, WEB-26 снимают эти `!isWeb` у своих блоков, а не в `navigation.ts`.
  - Заголовки секций «Local routing»/«Data» над одним блоком в web — принятый NIT до WEB-01/WEB-26.
- **Модель доверия лога**: tail и download открыты любому участнику tailnet, как и весь API. Аудит log-макросов (точечный, не исчерпывающий) не нашёл немаскированных секретов на info и ниже:
  - forwarder пишет origin или redacted URL;
  - заголовки ответа идут через allowlist (`response_processor.rs:884`);
  - глобальный proxy использует `mask_url`;
  - ошибки reqwest — `without_url`.

  На debug `proxy/forwarder.rs:1696-1701` пишет Copilot `dynamic_endpoint`/`base_url` без редактирования — код старый, не трогали. Пригодится фазам proxy (27–28). Тексты ошибок не-proxy сервисов (model_fetch, stream_check) поштучно не проверяли.
- **Smoke**: сейчас в web нет ни одного runtime-источника DEBUG-строк; debug пишется только при старте (bootstrap). Смысл уровня debug появится с портированием proxy (WEB-01).
- **Инвентаризация фазы 6**: строки rectifier/optimizer, log config и кнопки лога перенесены в «закрыто».
