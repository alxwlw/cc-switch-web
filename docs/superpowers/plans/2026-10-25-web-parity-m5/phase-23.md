# Phase 23 — WEB-05 / DX-63: Skills discovery, install/update, репозитории и backups

Цель: discovery и поиск на skills.sh, установка и обновление skills, репозитории, импорт unmanaged, backups/restore, миграция хранилища и установка из ZIP через загрузку файла в браузере работают в web. Уже поддержанные list/toggle/resync/uninstall не переделываются.

Спека: § WEB-05. Оценка M (4–6 дн.). Зависимость WEB-32 закрыта фазой 7 (`init_default_skill_repos`, миграция SSOT skills в bootstrap).

⚠️ Уточнения к спеке: (1) `SkillServiceState` в headless не нужен как managed state — `SkillService` без runtime-полей (services/skill.rs:576, только `#[cfg(test)] repo_fixture`); для четырёх `&self`-методов хендлер создаёт `SkillService::new()`. (2) Upload ZIP — не multipart (axum-фича `multipart` не включена): сырое тело `application/octet-stream` во временный файл, затем существующий `install_from_zip`.

Scope: `src-tauri/src/web_api.rs` (≈16 роутов), `src-tauri/tests/web_api_skills.rs`, `src/lib/api/adapter.ts` (raw body), `src/lib/api/skills.ts` (web-функция upload), `src/components/skills/UnifiedSkillsPanel.tsx:705-709` (web-ветка ZIP), тесты фронта.

Факты разведки:
- Фронт skills.ts:202-355: `get_skill_backups`, `delete_skill_backup {backupId}`, `install_skill_unified {skill: DiscoverableSkill, currentApp}`, `restore_skill_backup {backupId, currentApp}`, `scan_unmanaged_skills`, `import_skills_from_apps {imports}`, `discover_available_skills`, `check_skill_updates`, `update_skill {id}`, `migrate_skill_storage {target: "cc_switch"|"unified"}`, `get_cc_switch_skills_dir`, `open_cc_switch_skills_dir`, `search_skills_sh {query, limit, offset}`, `get_skill_repos`, `add_skill_repo {repo}`, `remove_skill_repo {owner, name}`, `open_zip_file_dialog`, `install_skills_from_zip {filePath, currentApp}`. Уже в web: installed, toggle, resync, uninstall (web_api.rs:107-110,547-589; adapter.ts:173-186).
- Вызовы: `useSkills.ts:40,48,75,116,182,267,282,312,322,337,358,383,395,437`; `UnifiedSkillsPanel.tsx:705` (dialog) → `:709` (installFromZip); `SkillsStorageSheet.tsx:96` (migrate).
- Backend commands/skill.rs: с `State<SkillServiceState>` + `State<AppState>` — :53 `install_skill_unified` → `service.install(&db,&skill,&app_type).await` (services/skill.rs:865), :122 `discover_available_skills` → `discover_available_report(repos)` (:3068), :136 `check_skill_updates` → `check_updates_report(&db)` (:1367), :161 `update_skill` → `update_skill(&db,&id)` (:1526). Только `State<AppState>` → статические `SkillService::*`: backups :37/:42 (`list_backups` :2057, `delete_backup` :2091), restore :78 (:2110), `scan_unmanaged` :103 (:2262), `import_from_apps` :111 (:2326), `migrate_storage` :175 (:1895), репозитории :335/:341/:355 (DB; add сначала `validate_repo_ref`), ZIP :369 `install_skills_from_zip(file_path, current_app, State<AppState>)` → `install_from_zip(&db,&Path,&AppType)` (:4339; распаковка с лимитом размера, :389). `search_skills_sh` :209 — async без State. `open_cc_switch_skills_dir` :195 — opener. `open_zip_file_dialog` — `AppHandle<R>`, native dialog (import_export.rs:131).
- DTO: большинство — camelCase (`InstalledSkill`, `UnmanagedSkill`, `SkillDiscoveryResult`, `SkillUpdateCheckResult`, `ZipInstallResult`, `MigrationResult`, `SkillsShSearchResult`, `SkillBackupEntry`, `ImportSkillSelection` …); `DiscoverableSkill` (:77), `SkillRepo` (:128) — поштучные rename; `SkillStorageLocation` (:66) — snake_case.
- Тесты: `src-tauri/tests/skill_sync.rs`, ~93 inline в services/skill.rs; фронт — `SkillsPageInstall.test.tsx`, `SkillsStorageSheet.test.tsx`, `UnifiedSkillsPanel.test.tsx`; HTTP-тестов нет.

### Task 1: backend — роуты (без ZIP) + тесты
- [x] `GET /api/skills/backups`, `DELETE /api/skills/backups/:id`, `POST /api/skills/backups/:id/restore` (`{currentApp}`), `POST /api/skills/install` (`{skill, currentApp}`), `GET /api/skills/unmanaged`, `POST /api/skills/import-from-apps` (`{imports}`), `GET /api/skills/discover`, `GET /api/skills/updates`, `POST /api/skills/:id/update`, `POST /api/skills/migrate-storage` (`{target}`), `GET /api/skills/dir`, `GET /api/skills/search?query=&limit=&offset=`, `GET/POST /api/skills/repos` (`{repo}`), `DELETE /api/skills/repos/:owner/:name`.
- [x] Сетевые операции (discover, search, install, update) идут через глобальный HTTP-клиент; ошибки сети — текстом сервиса.
- [x] `src-tauri/tests/web_api_skills.rs`: с `repo_fixture`-аналогом (локальный каталог-репозиторий, если seam доступен из integration-тестов; иначе — через локальный git-репозиторий в temp): discover → список; install → `installed` содержит, файлы в каталоге приложения temp HOME; update; backups: uninstall создаёт backup → restore возвращает; add repo с невалидной ссылкой → `{ok:false}`; migrate-storage меняет location.

### Task 2: ZIP upload
- [x] `POST /api/skills/install-zip?currentApp=…&filename=…` — тело `application/octet-stream`, `DefaultBodyLimit` 50 МиБ на роуте; сохранить в `tempfile` с исходным расширением (`.zip`/`.skill`, иначе 400) → `install_from_zip` → `ZipInstallResult`; временный файл удаляется в любом исходе.
- [x] Тесты: валидный ZIP со skill → установлен; не-ZIP → `{ok:false}`; ZIP с `../` в путях → отказ распаковки (существующая защита); тело больше лимита → 413.

### Task 3: frontend
- [x] adapter: case для всех команд, кроме `open_cc_switch_skills_dir` и `open_zip_file_dialog`; `Endpoint.body` допускает `Blob` (без `JSON.stringify`, свой `Content-Type`).
- [x] `skills.ts`: `installFromZipUpload(file: File, currentApp)` → `invoke("web_install_skills_from_zip_upload", {file, currentApp})` (adapter → POST install-zip с телом-файлом).
- [x] `UnifiedSkillsPanel` в web: `<input type="file" accept=".zip,.skill">` вместо native dialog; «открыть каталог skills» → показ серверного пути (`get_cc_switch_skills_dir`) с копированием.
- [x] Тесты: `tests/lib/skillsApi.web.test.ts` (URL/метод/body, Blob-тело для ZIP); `UnifiedSkillsPanel` web-ветка — выбор файла вызывает upload; существующие тесты skills зелёные.
- [x] Снять WEB-00-гейты (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [x] Полный набор гейтов + `cargo build --release --example server`.
- [x] Ветка `web-parity/web-05-skills`, PR; DX-63 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- ZIP-бомба: лимит распаковки (services/skill.rs:389) должен действовать и для upload — тест с архивом, раздувающимся сверх лимита.
- Параллельные install одного skill из двух вкладок: поведение как на desktop (`skill_state_write_guard` или аналог), без полуустановленного каталога.

## Notes for next phase

- **Статус до merge (2026-10-29)**: ветка `web-parity/web-05-skills` (worktree `.worktrees/web-05`), PR #24 (https://github.com/alxwlw/cc-switch-web/pull/24) открыт; DX-63 In Review (комментарий со ссылкой). Ветка также несёт закрытие фазы 22 в плане (`99f235af`).
  - Финальные гейты на `cfa708dc`:
    - fmt/clippy (lib + example) — чисто;
    - `(umask 022; cargo test --no-fail-fast)` — 4465 passed / 1 failed (baseline omp_search_paths);
    - `cargo build --release --example server` — ок;
    - `pnpm typecheck`/`format:check`/`build:renderer`/`build:web` — ок;
    - `test:unit` — 3858 passed / 3 failed (baseline: 2× UsageDashboard, rememberedChoice).
  - Ревью: Т1, Т2, Т3 одобрены с r0, BLOCKER и SHOULD-FIX нет. NIT Т1 (имя теста migrate-storage) исправлен в Т2. NIT Т3 DP-028 принят как есть: web-копирование пути вызывает `navigator.clipboard` напрямую, а не `copyText`; в web поведение то же. Решения — в journal p23.
- **Отличия от плана**: update — `POST /api/skills/update {id}`, потому что id `owner/name:directory` (`skill.rs:3244`) не помещается в путь. ZIP-бомба проверяется лимитом 30 000 записей. Байтовый бюджет 512 МиБ покрыт unit-тестами `skill.rs`. Отдельного whole-branch ревью не было: интеграцию задач проверила живая проверка.
- **Backend**:
  - Роуты — `web_api.rs:185-213`. ZIP — `install_skills_zip` (`:1464`), лимит `SKILL_ZIP_BODY_LIMIT` (`:1452`). Файл пишется во временный каталог под исходным именем: при корневом `SKILL.md` имя skill берётся из имени файла.
  - Seams — `skill_repo_fixture_test_hook`/`skills_sh_search_url_test_hook` (`web_api.rs:1625`, `:1631`). Статики — в `services/skill.rs:577` и рядом.
  - Тесты — `src-tauri/tests/web_api_skills.rs` (19). Fixture-репозиторий копируется как распакованный корень архива. Для поиска поднимается локальный axum-листенер на `127.0.0.1:0`.
- **Frontend**:
  - Skills-case в `adapter.ts:283`. Сырое тело — `mapped.body instanceof Blob` (`:1123`); это первый не-JSON запрос в адаптере, его можно переиспользовать для других upload (WEB-28?).
  - `installFromZipUpload` — `skills.ts:358`. Web-ZIP — `UnifiedSkillsPanel.tsx:707`, `:954`. Серверный путь с копированием — `SkillsStorageSheet.tsx:~149`.
  - `WEB_HIDDEN_VIEWS` теперь только `["auth"]`.
- **Таблица фазы 6**: строка skills и заметка Task 6 закрыты WEB-05.
- **Найдено живой проверкой, вне задачи**:
  - Headless-сервер не вызывает `crate::proxy::http_client::init` (на desktop его вызывает `lib.rs:913`). Поэтому все исходящие запросы web (skills, speed test и т.п.) идут через fallback-клиент, в логе `WARN [GP-004] Client not initialized, using fallback`. Это вход для WEB-23 (фаза 24): инициализировать клиент в общем bootstrap с proxy из настроек.
  - Тексты skills в web говорят «Import from this computer», хотя речь о сервере. Общая плашка «files live on the server host» это объясняет, но формулировку можно поправить отдельно.
  - `navigator.clipboard` по plain http на tailnet-IP недоступен: путь виден текстом, кнопка копирования показывает toast с ошибкой.
- **Post-merge**:
  - Пересобрать server и `dist-web` (Rust менялся). Перед install — `cp cc-switch-web cc-switch-web.prev`, бэкап БД при остановленном unit. Миграций нет.
  - Smoke:
    - `GET /api/skills/repos` → 4 дефолтных репозитория;
    - `GET /api/skills/discover` → список и `failures`;
    - `GET /api/skills/search?query=pdf&limit=3&offset=0` → результаты skills.sh;
    - `GET /api/skills/dir` → `~/.cc-switch-web/skills`;
    - `POST /api/skills/install-zip?currentApp=claude&filename=x.txt` → 400.
  - Реальный install на живом хосте не делать: он пишет в настоящий `~/.claude/skills`.
  - DX-63 → Done.
