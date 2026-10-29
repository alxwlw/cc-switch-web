# Phase 22 — WEB-25 / DX-83: install/update/uninstall CLI на Apps page

Цель: установка, обновление и удаление CLI, preview удаления и диагностика установок работают в web. Операции выполняются на сервере и ограничены таймаутом, по которому процесс убивается.

Спека: § WEB-25. Оценка M (3–5 дн.).

⚠️ Расхождение со спекой: спека велит «переиспользовать команды … добавить wrappers с timeout/cancellation», но у существующей lifecycle-команды нет ни таймаута, ни отмены. `cmd.output()` блокирует `spawn_blocking`, и отмена future не убивает дочерний процесс. Решение: таймаут с kill — в общем коде (desktop тоже выигрывает). Отмену из UI не делаем: обычный install/update занимает до нескольких минут, а таймаут закрывает зависание. Зафиксировать комментарием в DX-83.

Scope: `src-tauri/src/commands/misc.rs:~395-440` (spawn с дедлайном), `src-tauri/src/web_api.rs` (4 роута), `src-tauri/tests/web_api_tools.rs`, `src/lib/api/adapter.ts` (4 case), `src/components/apps/*` (server-host подпись), тесты фронта.

Факты разведки:
- Фронт settings.ts:285-325: `run_tool_lifecycle_action {tools: string[], action: "install"|"update"|"uninstall", wslShellByTool?, uninstallPlan?}`, `prepare_tool_uninstall {tool, wslShellByTool?}`, `probe_tool_installations {tools}`, `list_tool_installations {tools}`. Вызовы: `useToolManagement.ts:517` (list), `:569,:605` (probe), `:924`, `:670,:677` (lifecycle); `ToolUninstallConfirmDialog.tsx:53` (prepare). Уже в web — только `get_tool_versions` (web_api.rs:1003, adapter.ts:325).
- Backend commands/misc.rs, всё без State/AppHandle: :316 `prepare_tool_uninstall(tool, wsl_shell_by_tool) -> Vec<ToolUninstallCandidate>` (`spawn_blocking` → `tool_uninstall::candidates`); :334 `run_tool_lifecycle_action(tools, action, wsl_shell_by_tool, uninstall_plan) -> Result<(),String>` — `normalize_requested_tools`; uninstall требует план для единственного tool → `tool_uninstall::uninstall`; install/update — `build_tool_lifecycle_command` (:566), update сначала `tool_node_requirement::ensure_update_node(tool)` (tool_node_requirement.rs:44); всё внутри `TOOL_LIFECYCLE.run(...)`. :4729 `probe_tool_installations`, :4767 `list_tool_installations` (делегирует в probe), `spawn_blocking` вокруг `enumerate_tool_installations`.
- Процессы: `run_tool_lifecycle_silently` (:~395, non-windows) — `Command::new("bash").arg("-c")…output()`, без видимого терминала; Windows — temp `.bat` + `CREATE_NO_WINDOW` (~:410). Конкурентность: `ToolLifecycleCoordinator` (:242-290) — per-tool `try_lock_owned` + глобальный mutex; повтор для того же tool → `"TOOL_ACTION_IN_PROGRESS"`. Таймауты есть только у `--version`-проб (`INSTALL_PROBE_TIMEOUT`, tool_uninstall.rs:359,:588). Ошибка — последние 8 строк stderr/stdout (`finish_lifecycle_output`, :438).
- DTO: `WslShellPreferenceInput` (:178) — camelCase; `ToolUninstallPlan` (:294), `ToolInstallation` (:2909), `ToolInstallationReport` (:4707), `ToolUninstallCandidate` (:305) — snake_case (по комментариям — намеренно). Отдаём как есть.
- Тесты: 28 в tool_uninstall.rs, 3 в tool_node_requirement.rs; интеграционных и фронтовых нет.

### Task 1: таймаут с kill
- [x] `run_tool_lifecycle_silently`: `spawn()` + ожидание с дедлайном `TOOL_LIFECYCLE_TIMEOUT = 15 мин`; по истечении — kill процесса и его группы (на unix — `process_group(0)` + `killpg`), ошибка `"TOOL_ACTION_TIMEOUT: <tool> > 15 min"` с хвостом вывода. Захват stdout/stderr — как сейчас. Windows-ветка — тот же дедлайн через `wait_timeout`-эквивалент.
- [x] Unit-тест: команда `sleep 5` при дедлайне 1 с завершается ошибкой timeout за ≈1 с, процесс убит (pid отсутствует).

### Task 2: backend — роуты + тесты
- [x] `POST /api/tools/lifecycle` (`{tools, action, wslShellByTool?, uninstallPlan?}`), `POST /api/tools/uninstall-preview` (`{tool, wslShellByTool?}`), `POST /api/tools/probe` (`{tools}`), `POST /api/tools/installations` (`{tools}`). Команды вызывать напрямую (tauri-типов нет).
- [x] `src-tauri/tests/web_api_tools.rs`: фейковые бинарники в temp `PATH` (`claude --version` и т.п.) — probe/installations их находят; lifecycle `install` с `build_tool_lifecycle_command`, подменённым на быстрый скрипт через тестовый seam, → ok; два параллельных install одного tool → второй `TOOL_ACTION_IN_PROGRESS`; uninstall без плана → ошибка; неизвестный tool → ошибка `normalize_requested_tools`.

### Task 3: frontend
- [x] 4 case в `mapCommand`.
- [x] Apps page в web: подпись «CLI устанавливаются на сервер cc-switch-web (<hostname из /api/capabilities, если есть>)»; на время lifecycle — индикатор «до 15 минут»; ошибки `TOOL_ACTION_TIMEOUT` и `TOOL_ACTION_IN_PROGRESS` — человекочитаемым текстом.
- [x] Тесты: `tests/lib/toolsApi.test.ts`; `useToolManagement` — install проходит через adapter, timeout-ошибка показывает текст.
- [x] Снять WEB-00-гейты с install/update/uninstall (таблица фазы 6).

### Task 4: lifecycle через start + poll
Добавлена 2026-10-29 по решению владельца. Один HTTP-запрос на всю операцию (до 15 мин) обрывает браузер: по отчётам Safari на iOS режет запрос без ответа примерно через 60 с, у Firefox `network.http.response.timeout` равен 300 с. Сервер при этом доводит операцию до конца, а UI показывает «Web API недоступно».
- [x] Backend: `POST /api/tools/lifecycle` запускает операцию в фоне (`tokio::spawn`) и сразу отвечает `{jobId}`. `GET /api/tools/lifecycle/{jobId}` → `{state: "running" | "done" | "failed", error?}`; неизвестный id → 404. Реестр задач в памяти; завершённые старше часа удаляются при создании новой.
- [x] Adapter: `run_tool_lifecycle_action` — special-case до `mapCommand`. Старт, затем опрос каждые 2 с. Сетевой сбой при опросе не прерывает ожидание (iPhone засыпает, вкладка уходит в фон). `failed` → `Error(error)` дословно, `done` → resolve, 404 → понятная ошибка, общий предел ожидания больше 15 мин.
- [x] Тесты: `web_api_tools.rs` и `tests/lib/toolsApi.test.ts` переводятся на старт + опрос.

### Task 5: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`; на agentbox — update одного реального CLI из web.
- [x] Ветка `web-parity/web-25-cli-lifecycle`, PR; DX-83 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Установщик, ждущий ввода (sudo-пароль, интерактивный prompt npm), повисает до таймаута: stdin процесса должен быть `/dev/null`, тогда он падает сразу с понятной ошибкой.
- Сервер работает от пользователя без прав на глобальный `npm -g`: ошибка доходит до UI хвостом вывода, а не общим «failed».

## Notes for next phase

- **Статус до merge (2026-10-29)**: ветка `web-parity/web-25-cli-lifecycle` (worktree `.worktrees/web-25`), PR #23 (https://github.com/alxwlw/cc-switch-web/pull/23) открыт; DX-83 In Review (комментарий со ссылкой). Ветка также несёт закрытие фазы 21 в плане (`3e0c42d2`).
  - Финальные гейты на `5ce021ab`:
    - fmt/clippy (lib + example) — чисто;
    - `(umask 022; cargo test --no-fail-fast)` — 4446 passed / 1 failed (baseline omp_search_paths);
    - `cargo build --release --example server` — ок;
    - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` — ок;
    - `test:unit` — 3838 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - Ревью:
    - Т1 одобрена с r1. SHOULD-FIX DP-015: тест stdin оставался зелёным при `Stdio::inherit()`, потому что у тест-раннера stdin и так `/dev/null`. Теперь тест подменяет fd 0 на pipe и проверяет `/dev/stdin -ef /dev/null`.
    - Т2 одобрена с r1. SHOULD-FIX DP-018: поле `uninstallPlan` не было закреплено. Добавлены три теста отказа, включая подменённую команду в настоящем плане.
    - Т3 и Т4 одобрены с r0.
    - Все NIT приняты как есть, см. journal p22: процессный fd-0 swap в тесте; реальный установщик возможен только при сломанном seam; двойная регрессия path-фильтра и `validate_confirmed_plan`; непокрытые prune и bad-JSON poll; нет AbortSignal при unmount.
- **Решение владельца (2026-10-29)**: lifecycle в web работает через start + poll (задача 4). Причина — браузеры обрывают один долгий запрос: Safari iOS примерно через 60 с (по отчётам), Firefox через 300 с. Общий урок для долгих web-операций (WEB-26 restart, WEB-28 self-update, WEB-11 sync): не держать HTTP-запрос дольше минуты, а запускать задачу и опрашивать её статус.
- **Backend**:
  - Дедлайн — `run_tool_lifecycle_silently` (`misc.rs:434`) на готовом наборе `CommandDeadline`/`wait_child_output`/`terminate_child_tree`/`isolate_child_process_group`; `TOOL_LIFECYCLE_TIMEOUT` (`:408`).
  - Seam — `tool_lifecycle_command_test_hook` (`:400`).
  - Роуты — `web_api.rs:235-239`. Реестр задач — `TOOL_JOBS` (`:2100`), хендлеры `tool_lifecycle` (`:2114`) и `tool_lifecycle_status` (`:2154`).
  - Тесты — `src-tauri/tests/web_api_tools.rs` (11). Фейковая установка claude кладётся в `$HOME/.local/bin`, нативная — копией `/bin/true` в `~/.local/share/claude/versions/<v>` с симлинком.
- **Frontend**:
  - `runToolLifecycleAction` — special-case в `adapter.ts:958` (опрос 2 с, предел 20 мин); три других case — `:757-769`.
  - `TOOL_ACTION_TIMEOUT` → `settings.toolActionTimeout` (`useToolManagement.ts:775`).
  - Тесты — `tests/lib/toolsApi.test.ts` (10) и web-кейсы `AppsPage.test.tsx`.
- **Таблица фазы 6**: строка apps page закрыта WEB-25; `ToolUninstallConfirmDialog` в web теперь достижим. Гейт статуса Claude Desktop (`AppsPage.tsx:186`) остаётся до WEB-19.
- **Найдено живой проверкой, вне задачи**: `opencode upgrade` для npm-установки в `~/.local/share/opencode-ai` спрашивает «Install anyways? Yes/No» и на `/dev/null` stdin ждёт бесконечно. Теперь его через 15 минут убивает дедлайн. Нужен отдельный фикс команды обновления opencode (флаг `--method` или npm-путь), чтобы она не была интерактивной.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет.
  - Smoke:
    - `POST /api/tools/probe {"tools":["claude"]}` → отчёт с установкой;
    - `POST /api/tools/lifecycle {"tools":["nope"],"action":"update"}` → `jobId`, его опрос → `failed: No supported tools selected`;
    - `GET /api/tools/lifecycle/<случайный id>` → 404.
  - Реальный update на живом хосте в smoke не делать: opencode зависнет на 15 минут (см. выше).
  - DX-83 → Done.
