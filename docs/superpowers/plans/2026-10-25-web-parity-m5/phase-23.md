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
- [ ] `GET /api/skills/backups`, `DELETE /api/skills/backups/:id`, `POST /api/skills/backups/:id/restore` (`{currentApp}`), `POST /api/skills/install` (`{skill, currentApp}`), `GET /api/skills/unmanaged`, `POST /api/skills/import-from-apps` (`{imports}`), `GET /api/skills/discover`, `GET /api/skills/updates`, `POST /api/skills/:id/update`, `POST /api/skills/migrate-storage` (`{target}`), `GET /api/skills/dir`, `GET /api/skills/search?query=&limit=&offset=`, `GET/POST /api/skills/repos` (`{repo}`), `DELETE /api/skills/repos/:owner/:name`.
- [ ] Сетевые операции (discover, search, install, update) идут через глобальный HTTP-клиент; ошибки сети — текстом сервиса.
- [ ] `src-tauri/tests/web_api_skills.rs`: с `repo_fixture`-аналогом (локальный каталог-репозиторий, если seam доступен из integration-тестов; иначе — через локальный git-репозиторий в temp): discover → список; install → `installed` содержит, файлы в каталоге приложения temp HOME; update; backups: uninstall создаёт backup → restore возвращает; add repo с невалидной ссылкой → `{ok:false}`; migrate-storage меняет location.

### Task 2: ZIP upload
- [ ] `POST /api/skills/install-zip?currentApp=…&filename=…` — тело `application/octet-stream`, `DefaultBodyLimit` 50 МиБ на роуте; сохранить в `tempfile` с исходным расширением (`.zip`/`.skill`, иначе 400) → `install_from_zip` → `ZipInstallResult`; временный файл удаляется в любом исходе.
- [ ] Тесты: валидный ZIP со skill → установлен; не-ZIP → `{ok:false}`; ZIP с `../` в путях → отказ распаковки (существующая защита); тело больше лимита → 413.

### Task 3: frontend
- [ ] adapter: case для всех команд, кроме `open_cc_switch_skills_dir` и `open_zip_file_dialog`; `Endpoint.body` допускает `Blob` (без `JSON.stringify`, свой `Content-Type`).
- [ ] `skills.ts`: `installFromZipUpload(file: File, currentApp)` → `invoke("web_install_skills_from_zip_upload", {file, currentApp})` (adapter → POST install-zip с телом-файлом).
- [ ] `UnifiedSkillsPanel` в web: `<input type="file" accept=".zip,.skill">` вместо native dialog; «открыть каталог skills» → показ серверного пути (`get_cc_switch_skills_dir`) с копированием.
- [ ] Тесты: `tests/lib/skillsApi.web.test.ts` (URL/метод/body, Blob-тело для ZIP); `UnifiedSkillsPanel` web-ветка — выбор файла вызывает upload; существующие тесты skills зелёные.
- [ ] Снять WEB-00-гейты (таблица фазы 6).

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-05-skills`, PR; DX-63 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- ZIP-бомба: лимит распаковки (services/skill.rs:389) должен действовать и для upload — тест с архивом, раздувающимся сверх лимита.
- Параллельные install одного skill из двух вкладок: поведение как на desktop (`skill_state_write_guard` или аналог), без полуустановленного каталога.

## Notes for next phase

- (заполнить по итогам)
