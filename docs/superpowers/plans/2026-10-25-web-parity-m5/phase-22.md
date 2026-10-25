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
- [ ] `run_tool_lifecycle_silently`: `spawn()` + ожидание с дедлайном `TOOL_LIFECYCLE_TIMEOUT = 15 мин`; по истечении — kill процесса и его группы (на unix — `process_group(0)` + `killpg`), ошибка `"TOOL_ACTION_TIMEOUT: <tool> > 15 min"` с хвостом вывода. Захват stdout/stderr — как сейчас. Windows-ветка — тот же дедлайн через `wait_timeout`-эквивалент.
- [ ] Unit-тест: команда `sleep 5` при дедлайне 1 с завершается ошибкой timeout за ≈1 с, процесс убит (pid отсутствует).

### Task 2: backend — роуты + тесты
- [ ] `POST /api/tools/lifecycle` (`{tools, action, wslShellByTool?, uninstallPlan?}`), `POST /api/tools/uninstall-preview` (`{tool, wslShellByTool?}`), `POST /api/tools/probe` (`{tools}`), `POST /api/tools/installations` (`{tools}`). Команды вызывать напрямую (tauri-типов нет).
- [ ] `src-tauri/tests/web_api_tools.rs`: фейковые бинарники в temp `PATH` (`claude --version` и т.п.) — probe/installations их находят; lifecycle `install` с `build_tool_lifecycle_command`, подменённым на быстрый скрипт через тестовый seam, → ok; два параллельных install одного tool → второй `TOOL_ACTION_IN_PROGRESS`; uninstall без плана → ошибка; неизвестный tool → ошибка `normalize_requested_tools`.

### Task 3: frontend
- [ ] 4 case в `mapCommand`.
- [ ] Apps page в web: подпись «CLI устанавливаются на сервер cc-switch-web (<hostname из /api/capabilities, если есть>)»; на время lifecycle — индикатор «до 15 минут»; ошибки `TOOL_ACTION_TIMEOUT` и `TOOL_ACTION_IN_PROGRESS` — человекочитаемым текстом.
- [ ] Тесты: `tests/lib/toolsApi.test.ts`; `useToolManagement` — install проходит через adapter, timeout-ошибка показывает текст.
- [ ] Снять WEB-00-гейты с install/update/uninstall (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`; на agentbox — update одного реального CLI из web.
- [ ] Ветка `web-parity/web-25-cli-lifecycle`, PR; DX-83 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Установщик, ждущий ввода (sudo-пароль, интерактивный prompt npm), повисает до таймаута: stdin процесса должен быть `/dev/null`, тогда он падает сразу с понятной ошибкой.
- Сервер работает от пользователя без прав на глобальный `npm -g`: ошибка доходит до UI хвостом вывода, а не общим «failed».

## Notes for next phase

- (заполнить по итогам)
