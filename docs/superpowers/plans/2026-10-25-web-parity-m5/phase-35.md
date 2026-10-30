# Phase 35 — WEB-26 / DX-84: смена data directory и restart

Цель: в web видно, какой каталог данных использует сервер и откуда он взят. Если каталог не зафиксирован оператором через env, его можно сменить текстовым путём. Перезапуск сервера доступен из UI, когда процесс работает под supervisor (systemd/Docker).

Спека: § WEB-26. Оценка M (3–5 дн.).

Scope: `src-tauri/src/app_store.rs` (headless-бэкенд override), `src-tauri/src/config.rs:318` (резолвер не меняется по порядку), `src-tauri/src/bootstrap.rs` (загрузка override до `Database::init`), `src-tauri/src/web_api.rs` (3 роута), `src-tauri/examples/server.rs` (graceful shutdown + restart), `src/lib/api/adapter.ts`, `src/components/settings/SettingsPage.tsx:274-310,408-448`, `src/hooks/useDirectorySettings.ts`, `docs/deploy.md`.

Факты разведки:
- Резолвер `config::get_app_config_dir()` (config.rs:318): 1) env `CC_SWITCH_CONFIG_DIR` (непустой), 2) кэш override из Tauri Store (`app_store::get_app_config_dir_override`, app_store.rs:24), 3) `~/.cc-switch`. От него зависят `Database::init` (`<dir>/cc-switch.db`, database/mod.rs:100), `settings.json` (settings.rs:716) и OAuth-каталоги.
- Production-unit (`scripts/cc-switch-web.service.template`, рендерит `scripts/install-server.sh`) задаёт `CC_SWITCH_CONFIG_DIR=%h/.cc-switch-web` и `Restart=always`, `RestartSec=3`. В сервере кэш Store всегда None — override не читается.
- Desktop: `get_app_config_dir_override(app)` (commands/settings.rs:375) / `set_app_config_dir_override(app, path)` (:384) → `app_store::set_app_config_dir_to_store` пишет Tauri Store `app_paths.json`, ключ `app_config_dir_override` (пустой путь удаляет ключ; при чтении несуществующий путь игнорируется с warning, `~` раскрывается). DB при смене не копируется — новая директория начинает с того, что в ней лежит.
- `restart_app(app)` (settings.rs:253): `save_window_state_before_exit` → `cleanup_before_exit(&app)` (восстанавливает takeover/live-бэкапы) → `restart_process` (lib.rs:2375). `pick_directory` (config.rs:255) — native dialog. `is_portable_mode` (misc.rs:126) — handle-free (`portable.ini` рядом с exe).
- Фронт: `useSettings.ts:399` (`setAppConfigDirOverride` после save), `:515` (`requiresRestart`), `useDirectorySettings.ts:211` (чтение override), `:392,:411` (picker), `SettingsPage.tsx:288-307` (`handleRestartNow` → `settingsApi.restart()`), `:408-448` (`DirectoryInput` с browse/reset). В web не замаплены: `restart_app`, `get/set_app_config_dir_override`, `pick_directory`, `is_portable_mode`.
- Graceful shutdown с восстановлением takeover в headless сделан в фазе 27 (handle-free ядро `cleanup_before_exit`) — переиспользовать.

### Task 1: headless override
- [ ] `app_store`: headless-бэкенд — JSON-файл `app_paths.json` в `dirs::config_dir()/cc-switch-web/` с тем же ключом `app_config_dir_override`; `pub fn load_headless_override() -> Option<PathBuf>` заполняет тот же OnceLock-кэш, что читает резолвер. Запись атомарная (tmp + rename). Порядок резолвера не меняется: env > override > default.
- [ ] `bootstrap.rs`: `load_headless_override()` — первым шагом, до `Database::init`.
- [ ] Тест: без env и с файлом override → `get_app_config_dir()` указывает на override; с env → env.

### Task 2: роуты data dir
- [ ] `GET /api/app-config-dir` → `{effective, source: "env"|"override"|"default", override: string|null, envLocked: bool}`.
- [ ] `PUT /api/app-config-dir` (body `{path: string|null}`): при `envLocked` → 409 «каталог задан CC_SWITCH_CONFIG_DIR в unit сервера»; валидация пути — абсолютный или `~`, существует или создаётся, доступен на запись, если в нём есть `cc-switch.db` — её schema version не новее поддерживаемой (та же проверка, что в bootstrap фазы 7); иначе 400 с причиной. `null` удаляет override. Применяется только после restart.
- [ ] `is_portable_mode` — в adapter локально `false` (сервер не portable), без роута.

### Task 3: restart под supervisor
- [ ] `POST /api/restart`: supervisor определяется по `INVOCATION_ID` (systemd) или `CC_SWITCH_WEB_SUPERVISED=1` (Docker и прочие); без supervisor → 409 «перезапустите сервер вручную». С supervisor — ответ 202, затем graceful shutdown (ядро `cleanup_before_exit` из фазы 27) и `std::process::exit(75)`: код 75 перезапускается и при `Restart=always`, и при `on-failure`.
- [ ] Выход через инъецируемый `ShutdownHandle` в state, чтобы тест проверял сигнал, а не убивал процесс.
- [ ] Тесты: 409 без supervisor; с `CC_SWITCH_WEB_SUPERVISED=1` сигнал shutdown отправлен после ответа 202; PUT: env-locked → 409, путь без прав на запись → 400, DB с будущей schema → 400, валидный путь → override записан.

### Task 4: frontend
- [ ] adapter: `get_app_config_dir_override` → GET (`pick: "override"`), `set_app_config_dir_override` → PUT, `restart_app` → POST restart.
- [ ] `SettingsPage`/`DirectoryInput` в web: кнопка browse скрыта (native picker), текстовый ввод работает; при `envLocked` поле read-only с пояснением и значением env; показывать `source`.
- [ ] `handleRestartNow` в web: после 202 опрашивать `/api/health` (интервал 1 с, до 60 с), затем `location.reload()`; 409 → toast с текстом сервера.
- [ ] Тесты компонента: env-locked read-only, browse скрыт, restart-поток (mock fetch).

### Task 5: документация, гейты, PR, Plane
- [ ] `docs/deploy.md`: env-locked режим (по умолчанию) и режим override (убрать `CC_SWITCH_CONFIG_DIR` из unit); почему exit 75.
- [ ] Полный набор гейтов + `cargo build --release --example server`; ручная проверка на agentbox: restart из UI поднимает сервис (`systemctl --user status cc-switch-web`).
- [ ] Ветка `web-parity/web-26-data-dir`, PR; DX-84 → In Review + ссылка. Закрыть чекбоксы, Notes (интерфейс restart нужен фазе 37).

Риски для ревью:
- Restart при активном takeover: live-конфиги CLI на сервере должны вернуться в исходное состояние до выхода (тест на ядро cleanup из фазы 27).
- Двойной клик «перезапустить»: второй запрос во время shutdown не должен ронять процесс с паникой.

## Notes for next phase

- (заполнить по итогам)
