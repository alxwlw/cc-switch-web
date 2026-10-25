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
- [ ] server.rs: `env_logger` пишет одновременно в stderr и в `<get_log_dir()>/cc-switch-web.log` (tee-writer через `Target::Pipe`). При старте файл больше 20 МиБ переименовывается в `.1` (одна архивная копия).
- [ ] Фильтр `env_logger` — из `RUST_LOG`, иначе `trace`; эффективный уровень задаёт `log::set_max_level` из сохранённого `LogConfig` (bootstrap фазы 7 уже применяет его при старте). Так `set_log_config` в web действительно меняет детализацию.

### Task 2: backend — роуты + тесты
- [ ] `GET/PUT /api/settings/rectifier`, `GET/PUT /api/settings/optimizer`, `GET/PUT /api/settings/log` (body `{config}`), ответы — как у команд.
- [ ] `GET /api/logs/tail?lines=N` (N по умолчанию 500, максимум 5000) → `{path, lines: string[]}`; `GET /api/logs/download` → файл как `text/plain` с `Content-Disposition: attachment`. Файла нет → `{ok:false}` с путём, где он ожидался.
- [ ] `src-tauri/tests/web_api_settings_config.rs`: round-trip для трёх конфигов; `PUT log {level:"debug"}` → `log::max_level() == Debug`; tail на подготовленном файле из 10 строк с `lines=3` → последние 3; `lines=100000` обрезается до 5000; download отдаёт заголовок attachment.

### Task 3: frontend
- [ ] 6 case в `mapCommand` для конфигов; `reveal_app_log_file` в web не мапить.
- [ ] `SettingsPage`: в web кнопка «открыть лог» открывает просмотр хвоста (компонент `ServerLogViewer`: путь на сервере, последние строки, кнопка «скачать» → `/api/logs/download`).
- [ ] Тесты: панели rectifier/log в web сохраняют через adapter (mock fetch); viewer показывает строки и ссылку на download.
- [ ] Снять WEB-00-гейты с этих панелей и кнопки лога (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; на agentbox проверить, что после `PUT log debug` в файле появляются debug-строки.
- [ ] Ветка `web-parity/web-24-rectifier-log`, PR; DX-82 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Лог может содержать секреты (ключи в URL, токены): tail и download отдаются любому участнику tailnet, как и остальной API. Проверить, что маскирование `http_client::mask_url` и прочее, что desktop применяет при логировании, работает и в сервере; в Notes зафиксировать модель доверия.

## Notes for next phase

- (заполнить по итогам)
