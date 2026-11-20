# Phase 2 — консоль, лицо проекта, `.github/`, синхронизация

Цель: консоль понимает режим релизов; репозиторий снаружи — самостоятельный проект (README, SECURITY, шаблоны, свои workflows); собственные файлы форка переживают синхронизацию с апстримом. После фазы можно ставить первый тег.

Начинать только после того, как `sync/upstream-6da70837` слит в `main`: ветка плана ребейзится на свежий `main`, номера строк ниже — до синхронизации, их нужно сверить заново (`AboutSection.tsx` синхронизация тоже меняет).

Спека: `docs/superpowers/specs/2026-11-19-oss-release-design.md`, разделы 5–7.

Scope: `src/components/settings/{AboutSection,SelfUpdatePanel}.tsx`, `src/lib/updater.ts`, `src/lib/api/{capabilities,selfUpdate}.ts`, `src/hooks/useSelfUpdate.ts`, `src/i18n/locales/*.json` (ключи `web.update.*`), тесты в `tests/`; `docs/upstream.lock`, `scripts/sync-upstream.sh`; `.github/`; `README*.md`, `CHANGELOG.md`, `LICENSE`, `SECURITY.md`, `SUPPORT.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `docs/deploy.md`, `assets/web/`.

Факты разведки (`origin/main@b43896c1`, до синхронизации):
- `src/lib/api/adapter.ts:14` `isWeb`; `web_check_update` → `GET /api/update/check[?force=1]` (:1063), `web_install_update` → `POST /api/update/install` (:1070).
- `src/lib/api/capabilities.ts:1-18` `ServerCapabilities {mode, version, buildSha, runtime_boundaries, platform, selfUpdate}`; `fetchCapabilities()` (:24) бросает, если `version` не строка.
- `src/lib/updater.ts:34-47` `WebUpdateCheck {status, currentVersion, currentSha, latestSha, behindBy, commits, checkedAt, branch, instructions{systemd,docker}}`; `parseWebUpdateCheck(raw)` (:76); `checkWebUpdate` (:146) строит `availableVersion` как `${currentVersion}+${latest.slice(0,7)}`.
- `src/lib/api/selfUpdate.ts`: `UpdateRunStatus` (:4-12), `InstallLaunch {unit, branch}` (:18-21).
- `AboutSection.tsx:57-196` `WebUpdateStatus`: ошибка (:86-95), подвал с sha и `checkedAt` (:98-112), `unknown` (:114-122), `up-to-date` (:124-131), `behind` (:133-195: заголовок с числом, `SelfUpdateButton`, `WebCommitList`, инструкции с кнопками Copy); строки инструкций приходят с сервера.
- `SelfUpdatePanel.tsx:14-22` `PROGRESS_STAGES` = `checking, fetching, merging, building, installing, restarting, rolling-back`; текст стадии — `web.update.install.stage.<stage>`, неизвестная — сырой строкой (:156-158); `SelfUpdateButton` (:39-99) — подтверждение с веткой, `latestSha`, `behindBy`, `WebCommitList`, предупреждением про БД.
- `useSelfUpdate.ts:72` `REBUILD_STAGES` = `building, installing, restarting`; финальные стадии — :174-202.
- Локали: `src/i18n/locales/{en,ja,tr,zh-TW,zh}.json` (после синхронизации ещё `de, ko, ru`); блок `web.update` (en.json:5090).
- Тесты: `tests/components/AboutSection.web.test.tsx`, `tests/components/AboutSection.selfUpdate.test.tsx`, `tests/hooks/useSelfUpdate.test.tsx`, `tests/i18n/updateCatalog.test.tsx`, `tests/lib/capabilities.test.ts`.
- `.github/`: workflows `ci.yml, claude.yml (секрет CLAUDE_CODE_OAUTH_TOKEN), labeler.yml, release.yml (Tauri/Apple-секреты, тег v*), stale.yml, sync-r2.yml (R2-секреты), wsl2-nightly.yml`; `actions/prepare-tauri-signing-key/`; `CODEOWNERS` (`@farion1231`), `FUNDING.yml`, `labeler.yml`; `ISSUE_TEMPLATE/{bug_report,config,doc_issue,feature_request,question}.yml` (config ведёт на `farion1231/cc-switch`); `pull_request_template.md` двуязычный. У форка секретов нет (`gh secret list` пуст, 2026-11-19).
- Следы апстрима: `CODE_OF_CONDUCT.md:41,131` (почта автора апстрима), `SUPPORT.md` и `CONTRIBUTING.md` (ссылки на `farion1231/cc-switch`, `support@ccswitch.io` в `CONTRIBUTING.md:230,487`), `README*.md`, `SECURITY.md`, `docs/deploy.md`.

### Task 1: консоль — режим релизов

**Files:**
- Modify: `src/lib/api/capabilities.ts`, `src/lib/updater.ts`, `src/lib/api/selfUpdate.ts`
- Modify: `src/components/settings/AboutSection.tsx` (web-ветка), `src/components/settings/SelfUpdatePanel.tsx`, `src/hooks/useSelfUpdate.ts`
- Modify: `src/i18n/locales/*.json` (только `web.update.*`)
- Test: `tests/lib/capabilities.test.ts`, тест `parseWebUpdateCheck` (найти `grep -rl parseWebUpdateCheck tests/`; нет — `tests/lib/webUpdateCheck.test.ts`), `tests/components/AboutSection.web.test.tsx`, `tests/components/AboutSection.selfUpdate.test.tsx`, `tests/hooks/useSelfUpdate.test.tsx`, `tests/i18n/updateCatalog.test.tsx`

**Interfaces:**
- Consumes (фаза 1): `webVersion` в `/api/capabilities`; в ответе проверки `mode`, `latestVersion`, `releaseUrl`, `publishedAt`; ответ установки `{unit, branch|null, repo|null}`; в статусе `fromVersion`, `toVersion`; стадии `downloading`, `verifying`.
- Produces:
  - `ServerCapabilities.webVersion: string | null` (не строка → `null`).
  - `WebUpdateCheck` + `mode: "release" | "commit"`, `latestVersion: string | null`, `releaseUrl: string | null`, `publishedAt: string | null`; `parseWebUpdateCheck`: нет `mode` или чужое значение → `"commit"` (старый сервер).
  - `checkWebUpdate`: в режиме релизов `availableVersion` = `latestVersion`.
  - `InstallLaunch {unit: string; branch: string | null; repo: string | null}`; `UpdateRunStatus` + `fromVersion: string | null; toVersion: string | null`.
  - `PROGRESS_STAGES` + `downloading`, `verifying` (порядок: `checking, fetching, merging, downloading, verifying, building, installing, restarting, rolling-back`).
  - Ключи `en.json`: `web.update.releaseBehindTitle` = «Version {{version}} is available», `web.update.releaseNotes` = «Release notes», `web.update.install.stage.downloading` = «Downloading the release», `web.update.install.stage.verifying` = «Checking the download», `web.update.install.confirmReleaseMessage` = «Install version {{version}}? The server restarts and the page reloads by itself.». Во всех остальных локалях — переводы тех же ключей.

Поведение:
- Строка версии в About (web): есть `webVersion` → `v<webVersion>`, иначе `version`, как сейчас.
- `behind` в режиме релизов: заголовок `releaseBehindTitle` с `latestVersion`, ссылка `releaseNotes` на `releaseUrl` (`target="_blank" rel="noopener noreferrer"`), списка коммитов нет; кнопка самообновления и инструкции — как в режиме коммитов. `up-to-date`, `unknown`, ошибка — без изменений.
- `useSelfUpdate` считает финальную стадию `done` «ничего не изменилось» (страницу не перезагружать) только при `fromSha === toSha` и обоих не `null`. Релизный `done` «already up to date» приходит с `fromVersion === toVersion` и `fromSha`/`toSha` равными `null`: по старому правилу страница перезагрузилась бы. Правило: «ничего не изменилось» = (`fromSha === toSha`, оба не `null`) или (`fromVersion === toVersion`, оба не `null`).
- Подтверждение самообновления в режиме релизов: `confirmReleaseMessage` с `latestVersion` вместо ветки, sha и списка коммитов; предупреждение про БД остаётся.

- [ ] **Step 1: тесты.**
  - `capabilities.test.ts`: `webVersion is passed through and anything but a string is null` — `"4.0.35-web.1"` → та же строка; `undefined`, `1`, `null` → `null`.
  - тест `parseWebUpdateCheck`: `a check without mode is commit mode`; `release fields are read` (`mode: "release"`, `latestVersion`, `releaseUrl`, `publishedAt`).
  - `AboutSection.web.test.tsx`: `release mode behind shows the new version and the release notes link, not a commit list` — текст «Version 4.0.35-web.2 is available», ссылка «Release notes» с `href` = `releaseUrl`, нет элементов `WebCommitList`; `the version line shows the fork version when the server has one` — «v4.0.35-web.1».
  - `AboutSection.selfUpdate.test.tsx`: `release mode confirmation names the version, not the branch` — в диалоге «Install version 4.0.35-web.2?», нет имени ветки; `downloading and verifying stages have their own text` — «Downloading the release», «Checking the download».
  - `tests/hooks/useSelfUpdate.test.tsx`: `a release done with equal versions and null shas does not reload the page`; `a release done with different versions reloads it`; commit-mode `done` с равными sha — как раньше.
  - `updateCatalog.test.tsx`: новые ключи — в списке проверяемых, во всех локалях.
- [ ] **Step 2:** `pnpm test:unit -- tests/lib tests/components/AboutSection.web.test.tsx tests/components/AboutSection.selfUpdate.test.tsx tests/i18n/updateCatalog.test.tsx` → новые FAIL.
- [ ] **Step 3:** реализовать.
- [ ] **Step 4:** тот же запуск → PASS; `pnpm typecheck`, `pnpm format:check`, весь `pnpm test:unit` → зелёные.
- [ ] **Step 5:** commit `feat(web): the console shows release updates and release self-update stages`.

### Task 2: собственные пути форка и скрипт синхронизации

**Files:**
- Modify: `docs/upstream.lock`
- Create: `scripts/sync-upstream.sh`
- Create: `scripts/tests/sync-upstream.test.sh`

**Interfaces:**
- Produces:
  - В `docs/upstream.lock` — массив, по строке на шаблон:

    ```
    # Fork-owned paths: after a merge of upstream, scripts/sync-upstream.sh puts them back to the
    # fork's version (and removes the ones the fork does not have). `*` matches any characters, `/` too.
    fork_owned = [
      "README.md",
      "README_*.md",
      "SECURITY.md",
      "SUPPORT.md",
      "CONTRIBUTING.md",
      "CODE_OF_CONDUCT.md",
      "CHANGELOG.md",
      "LICENSE",
      ".github/*",
    ]
    ```
    Блок «Sync cadence» переписать на вызов скрипта; комментарий «Branch policy» остаётся.
  - `scripts/sync-upstream.sh [--ref <upstream-ref>]` (по умолчанию `upstream/main`). Подключённый через `source` — только функции: `fork_owned_patterns` (печатает шаблоны из `docs/upstream.lock`, по одному в строке), `is_fork_owned <path>` (сопоставление `[[ $path == $pattern ]]`). Коды: 0 — merge подготовлен без конфликтов; 2 — остались конфликты в коде; 1 — отказ.

Шаги скрипта:
1. Отказ (1), если есть изменения отслеживаемых файлов или идёт merge/rebase.
2. `git fetch upstream`; `upstream-main` двигается только fast-forward до `<ref>` (`git merge-base --is-ancestor`, затем `git update-ref`), иначе отказ (1).
3. Ветка `sync/upstream-<7 знаков ref>` от `main`; `git merge --no-ff --no-commit upstream-main` (конфликты допустимы).
4. Каждый путь из объединения деревьев `HEAD`, `MERGE_HEAD` и индекса, для которого `is_fork_owned`: есть в `HEAD` → `git checkout HEAD -- <path>`; нет → `git rm -q -f --cached <path>` и удалить файл.
5. Напечатать `kept fork version (upstream changed): …` — собственные пути, которые апстрим изменил между merge-base и `MERGE_HEAD`; затем оставшиеся конфликты (`git diff --name-only --diff-filter=U`). Коммит не делается: скрипт печатает, что проверить и как закоммитить.

- [ ] **Step 1: тесты** `scripts/tests/sync-upstream.test.sh` — временные репозитории: «апстрим» с `README.md`, `.github/workflows/release.yml`, `src/a.rs` (20 строк); «форк» — клон с remote `upstream`, в `main` форк меняет `README.md`, удаляет `release.yml`, меняет строку 1 `src/a.rs`, кладёт свой `docs/upstream.lock`; затем апстрим меняет `README.md` и `release.yml`, добавляет `README_FR.md` и `.github/workflows/new.yml`, меняет строку 20 `src/a.rs`. Случаи:
  1. `fork-owned files keep the fork version and new upstream ones are dropped` — после скрипта `README.md` побайтно как в `main` форка; `release.yml`, `README_FR.md`, `new.yml` отсутствуют в рабочем дереве и индексе; код 0.
  2. `code changes from both sides merge` — в `src/a.rs` обе правки.
  3. `a real code conflict is reported and the fork-owned paths are still restored` — апстрим меняет строку 1 `src/a.rs` иначе: код 2, `src/a.rs` в выводе конфликтов, `README.md` — версия форка.
  4. `the report names what upstream changed in fork-owned paths` — вывод содержит `README.md` и `.github/workflows/release.yml`.
  5. `a dirty tree or a merge in progress is refused` — код 1, ветка `sync/…` не создана.
  6. `upstream-main only moves fast-forward` — `upstream-main` указывает на коммит не из истории `<ref>`: код 1.
- [ ] **Step 2:** `bash scripts/tests/sync-upstream.test.sh` → FAIL; написать скрипт → `6 passed, 0 failed`.
- [ ] **Step 3:** commit `chore(sync): fork-owned paths survive upstream merges`. Проба на настоящем апстриме — в Task 7, когда собственные файлы уже переписаны.

### Task 3: `.github/` форка

**Files:**
- Delete: `.github/workflows/{release,sync-r2,wsl2-nightly,stale,claude}.yml`, `.github/actions/prepare-tauri-signing-key/`, `.github/FUNDING.yml`, `.github/ISSUE_TEMPLATE/{doc_issue,question}.yml`, `.github/dependabot.yml` (если синхронизация его не удалила)
- Modify: `.github/CODEOWNERS`, `.github/ISSUE_TEMPLATE/{bug_report,feature_request,config}.yml`, `.github/pull_request_template.md`
- Keep: `.github/workflows/ci.yml`, `.github/workflows/labeler.yml` + `.github/labeler.yml` (секретов не требуют), `.github/workflows/release-web.yml`, `.github/workflows/scripts.yml`, workflows, пришедшие синхронизацией, если они зелёные без секретов (например `schema-version.yml`)

Содержание (всё по-английски):
- `CODEOWNERS`: одна строка `* @alxwlw` и комментарий над ней.
- `bug_report.yml`: поля — версия (input, подсказка «Settings → About, e.g. v4.0.35-web.1», обязательное), способ установки (dropdown: `install.sh`, `Docker Compose`, `built from source`, обязательное), дистрибутив (input), агент (dropdown: Claude Code, Codex, Gemini CLI, OpenCode, other), что произошло и что ожидалось (textarea, обязательное), логи (textarea, `render: shell`, подсказка `journalctl --user -u cc-switch-web -n 100` или `docker compose logs`).
- `feature_request.yml`: проблема (обязательное), предложение.
- `config.yml`: `blank_issues_enabled: false`; `contact_links`: «Report a security issue» → `https://github.com/alxwlw/cc-switch-web/security/advisories/new`; «Bug in the cc-switch desktop app» → `https://github.com/farion1231/cc-switch/issues`.
- `pull_request_template.md`: `## Summary`, `## How it was tested`, чек-лист (tests, docs/deploy.md при изменении установки, CHANGELOG).

- [ ] **Step 1:** удалить и переписать файлы по списку.
- [ ] **Step 2:** проверка: `ls .github/workflows` — только оставленные; `grep -rn "secrets\." .github/workflows` — только `GITHUB_TOKEN`; `actionlint` без ошибок; `python3 -c 'import sys,yaml; [yaml.safe_load(open(f)) for f in sys.argv[1:]]' .github/ISSUE_TEMPLATE/*.yml` — без ошибок.
- [ ] **Step 3:** commit `chore(github): the fork's own workflows, owners and issue templates`.

### Task 4: README, CHANGELOG, LICENSE, скриншот

**Files:**
- Modify: `README.md` (полностью новый), `CHANGELOG.md` (полностью новый), `LICENSE` (+ строка)
- Delete: `README_DE.md`, `README_JA.md`, `README_KO.md`, `README_RU.md`, `README_TR.md`, `README_ZH.md`, `README_ZH-TW.md`
- Create: `assets/web/console.png`
- Create: `scripts/tests/fork-identity.test.sh`

**Interfaces:**
- Consumes: `fork_owned_patterns`, `is_fork_owned` (Task 2, через `source scripts/sync-upstream.sh`).
- Produces: `fork-identity.test.sh` — проверка, которую Task 5 и следующие правки собственных файлов обязаны проходить.

`fork-identity.test.sh`: по всем отслеживаемым файлам, для которых `is_fork_owned`, ищет без учёта регистра `ccswitch\.io|trendshift|star-history|assets/partners|farion1231@|@farion1231|github: farion1231|aff=cc-switch|invitecode=` и адреса почты (`[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}`, кроме `noreply.github.com`); каждое совпадение печатается как `файл:строка`, есть хоть одно → код ≠ 0. Ссылки на `https://github.com/farion1231/cc-switch…` и строка копирайта апстрима в `LICENSE` разрешены.

`README.md` (английский), по порядку:
1. `# cc-switch-web` и одна строка: «The cc-switch console for servers without a desktop: manage the providers, MCP servers and skills of Claude Code, Codex, Gemini CLI and other coding agents from your browser.»
2. Бейджи: последний релиз (`img.shields.io/github/v/release/alxwlw/cc-switch-web`), CI (`ci.yml` badge), лицензия MIT.
3. Скриншот `assets/web/console.png`.
4. «Quick start» — два способа рядом:

   ```bash
   # On the server, as the user your agents run as:
   curl -fsSL https://github.com/alxwlw/cc-switch-web/releases/latest/download/install.sh | bash
   # On your laptop:
   ssh -L 18766:localhost:18766 you@your-server   # then open http://localhost:18766
   ```
   Docker: команды из шапки `deploy/compose.yml` (`curl -fsSLO https://raw.githubusercontent.com/alxwlw/cc-switch-web/main/deploy/compose.yml`, `.env`, `docker compose up -d`). Tailscale: `… | bash -s -- --tailscale` и адрес из вывода установщика.
5. «Based on [farion1231/cc-switch](https://github.com/farion1231/cc-switch)» — благодарность автору апстрима одним абзацем: форк повторяет десктопное приложение в браузере и регулярно синхронизируется с ним.
6. Ниже: «What works and what does not» (что из десктопа недоступно в web — по `runtime_boundaries` в `/api/capabilities` и таблице desktop-only строк из закрытого плана web-parity M5, удалённого из дерева: см. историю git), «Updating» (Settings → About, самообновление, ручные команды), «Uninstalling», «Security» (два предложения + ссылка на `SECURITY.md`), «Building from source» (ссылка на `CONTRIBUTING.md`), «License» (MIT).
Спонсоров, Trendshift, star-history, ссылок на ccswitch.io и переводов нет.

`CHANGELOG.md`:

```markdown
# Changelog

Changes of cc-switch-web. Changes of the cc-switch desktop app it is based on are in the upstream
[CHANGELOG](https://github.com/farion1231/cc-switch/blob/main/CHANGELOG.md).

## v<Cargo version>-web.1 — unreleased

- First release of the fork: …
```
Пункты первого раздела: консоль в браузере (паритет с десктопом), установка `install.sh` и Docker Compose, доступ через SSH-туннель или Tailscale, проверка обновлений и самообновление по релизам. `<Cargo version>` — `version` из `src-tauri/Cargo.toml` на момент задачи.

`LICENSE`: после строки копирайта апстрима — `Copyright (c) 2026 alxwlw`.

Скриншот: сервер из этой ветки с временными `HOME` и `CC_SWITCH_CONFIG_DIR` и двумя-тремя выдуманными провайдерами (имена вида «Example Relay», ключи вида `sk-example-…`, URL `https://api.example.com`); страница провайдеров Claude Code, окно 1440×900, PNG до 500 КБ (Playwright MCP или headless Chromium). Ни одного настоящего ключа, URL, имени хоста или пути с именем пользователя — проверить глазами перед коммитом.

- [ ] **Step 1:** написать `fork-identity.test.sh`; `bash scripts/tests/fork-identity.test.sh` → FAIL с совпадениями в README, `.github` уже чистый после Task 3 (если нет — вернуть Task 3).
- [ ] **Step 2:** новый `README.md`, `CHANGELOG.md`, строка в `LICENSE`; удалить переводы.
- [ ] **Step 3:** снять скриншот, проверить содержимое, положить в `assets/web/console.png`.
- [ ] **Step 4:** `bash scripts/tests/fork-identity.test.sh` → совпадения только в файлах Task 5 (`SECURITY.md`, `SUPPORT.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`) — их список в отчёте задачи; в README, CHANGELOG, LICENSE, `.github/` — ни одного.
- [ ] **Step 5:** commit `docs: README, changelog and license of the fork`.

### Task 5: SECURITY, CONTRIBUTING, SUPPORT, CODE_OF_CONDUCT, `docs/deploy.md`

**Files:**
- Modify: `SECURITY.md`, `SUPPORT.md`, `CONTRIBUTING.md` (все — полностью новые, английский)
- Modify: `CODE_OF_CONDUCT.md` (только контакт: строки :41, :131)
- Modify: `docs/deploy.md` (перестроить)

**Interfaces:**
- Consumes: `fork-identity.test.sh` (Task 4); флаги и раскладка `install.sh`, `deploy/compose.yml` (фаза 1).

Содержание:
- `SECURITY.md`: поддерживается только последний релиз; модель угроз — у сервера нет логина, по умолчанию он слушает `127.0.0.1`, не-loopback адрес — только с `TRUSTED_TAILNET=1`; любой, кто достучался до порта, полностью управляет конфигами агентов этого пользователя и может запустить обновление до опубликованного релиза; поэтому порт не публикуется наружу, а доступ — SSH-туннель или tailnet с ACL. Сообщать — через «Report a vulnerability» (`https://github.com/alxwlw/cc-switch-web/security/advisories/new`), не в публичные issues. В scope — сервер (`/api/*`), установщик, самообновление, образ; уязвимости самого десктопного приложения — апстриму.
- `CONTRIBUTING.md`: сборка и запуск из исходников (`pnpm install`, `pnpm build:web`, `mkdir -p dist`, `cargo run --example server --manifest-path src-tauri/Cargo.toml`, `scripts/install-server.sh` для systemd); тесты (`cargo nextest run`, `pnpm test:unit`, `bash scripts/tests/*.test.sh`); где живёт код форка (адаптеры из Global Constraints) и почему апстримовский код не правится попутно; синхронизация (`scripts/sync-upstream.sh`, `docs/upstream.lock`); PR — draft в `main`; баги десктопного приложения — в апстрим.
- `SUPPORT.md`: вопросы и баги — issues этого репозитория; приложить версию из Settings → About, способ установки, дистрибутив, логи (`journalctl --user -u cc-switch-web -n 100` или `docker compose logs`).
- `CODE_OF_CONDUCT.md`: текст Contributor Covenant остаётся; контакт — «the maintainer, [@alxwlw](https://github.com/alxwlw), through GitHub» вместо почты.
- `docs/deploy.md`, разделы по порядку: «Install from a release» (`install.sh`, флаги, раскладка, unit, linger, системные пакеты), «Docker Compose» (`deploy/compose.yml`, `network_mode: host` и почему — прокси `127.0.0.1:15721` и takeover, UID хоста, монтирование `$HOME`), «Access» (SSH-туннель, Tailscale, `TRUSTED_TAILNET`), «Updates» (проверка по релизам, самообновление, ручные команды), «Uninstall», «Build from source» (нынешнее содержимое про `install-server.sh`, build commit, проверку по коммитам и самообновление из checkout — сюда, без потерь), «Data directory» (нынешний раздел).

- [ ] **Step 1:** переписать файлы.
- [ ] **Step 2:** `bash scripts/tests/fork-identity.test.sh` → код 0, совпадений нет.
- [ ] **Step 3:** каждая команда из `docs/deploy.md` и README сверена с флагами `install.sh` и `deploy/compose.yml` (`grep -o -- '--[a-z-]*' docs/deploy.md README.md | sort -u` ⊆ флагов `install.sh` и `docker compose`).
- [ ] **Step 4:** commit `docs: security policy, contributing guide, support and deploy guide of the fork`.

### Task 6: настройки репозитория и отвязка от сети форков

Задача без кода: внешние действия в GitHub. Описание и topics владелец одобрил в спеке; отвязку делает владелец.

- [ ] **Step 1:** `gh repo edit alxwlw/cc-switch-web --description "cc-switch console for servers without a desktop: manage Claude Code, Codex and Gemini CLI providers, MCP servers and skills from a browser" --add-topic claude-code,codex,gemini-cli,mcp,cc-switch,self-hosted,headless,web-ui`; проверка `gh repo view alxwlw/cc-switch-web --json description,repositoryTopics`.
- [ ] **Step 2:** `gh api -X PUT repos/alxwlw/cc-switch-web/private-vulnerability-reporting`; проверка `gh api repos/alxwlw/cc-switch-web/private-vulnerability-reporting` → `{"enabled":true}`.
- [ ] **Step 3:** файл вопроса для владельца (`plan-state block <task> --ask <file>`): текст запроса в GitHub Support на отвязку `alxwlw/cc-switch-web` от сети форков `farion1231/cc-switch` (по-английски, 3–4 предложения: репозиторий стал самостоятельным продуктом, синхронизация идёт через git remote, связь с сетью форков не нужна), ссылка `https://support.github.com/request` и что выбрать в форме. Задача ждёт ответа владельца, остальные задачи фазы её не ждут.
- [ ] **Step 4:** после ответа владельца: `gh api repos/alxwlw/cc-switch-web -q .fork` → `false`; `plan-state answer`, задача закрыта. Пока ответа нет — задача остаётся `blocked`, PR фазы её не ждёт.

### Task 7: гейты, проба синхронизации, PR фазы

- [ ] **Step 1:** гейты фазы 1 (Rust, фронтенд, все `scripts/tests/*.test.sh`) → зелёные.
- [ ] **Step 2: проба на настоящем апстриме** (критерий 6 спеки). Одноразовый клон ветки в scratchpad (`git clone --no-local <worktree> <scratchpad>/sync-probe`, remote `upstream` = `https://github.com/farion1231/cc-switch.git`, ветка `main` клона = ветка плана), там `bash scripts/sync-upstream.sh` → среди конфликтов нет собственных путей форка, `bash scripts/tests/fork-identity.test.sh` в клоне после скрипта → код 0. Конфликты в коде (если есть) — списком в `plan-state journal`; клон удалить.
- [ ] **Step 3:** draft PR фазы (`gh pr create --draft -R alxwlw/cc-switch-web --base main`), в описании — что после слияния можно ставить первый тег и что отвязка от сети форков ждёт владельца; `plan-state verify-push`; CI (`CI`, `Release (web)`) зелёный.

## Notes for next phase

- Первый тег — `v<Cargo version>-web.1`; строка `## v…-web.1 — unreleased` в `CHANGELOG.md` перед тегом получает дату вместо «unreleased».
- Фаза закрыта PR https://github.com/alxwlw/cc-switch-web/pull/44 (ветка `feat-cld/oss-release-p2`).
- Проба синхронизации (upstream `main` fa4a237, 2026-11-20): конфликтов нет, но апстрим поднимает версию до 4.0.36. Если синхронизация сольётся до первого тега, тег — `v4.0.36-web.1`, и заголовок в `CHANGELOG.md` меняется вместе с ним.
- `.github/*` — собственный путь форка: синхронизация отбрасывает правки апстрима в `ci.yml` и других workflows и печатает их строкой `kept fork version (upstream changed)`. Нужное переносится руками.
- В `fork_owned` сверх списка плана — `docs/deploy.md` (спека §7) и `tests/config/readmeParity.test.ts` (апстримовский тест требует удалённые переводы README; синхронизация удаляет его снова).
- Консоль в режиме релизов не говорит о ветке, checkout, коммите и пересборке. Общие ключи `web.update.upToDateTitle`, `install.unchangedTitle`, `install.stage.checking` переписаны нейтрально для обоих режимов; добавлены `releaseUnknownTitle`, `releaseUnknownHint`, `install.releaseConfirmButton`. Переводы на 7 языков сделаны исполнителем, носители их не проверяли.
- Каталог данных у установки из релиза переопределяется drop-in `~/.config/systemd/user/cc-switch-web.service.d/data-dir.conf` с `Environment=CC_SWITCH_CONFIG_DIR=`: `install.sh` переписывает сам unit на каждом запуске, а `.d` не трогает (`docs/deploy.md`, Data directory).
- Не сделано, вне фазы: About в web по-прежнему показывает ссылку на сайт ccswitch.io, заметки к релизам и ссылки GitHub апстрима, а также просьбу поставить звезду (`AboutSection.tsx`, web-ветка). Для релиза форка ссылка на заметки апстрима неверна. Кандидат в фазу 3 или в бэклог.
- Task 6 Step 3 (отвязка от сети форков через GitHub Support) ждёт владельца: вопрос в `plan-state` задачи 6. PR от неё не зависит.
- Мелочи, отложенные ревью: `upstream-main` сдвигается раньше поздних отказов скрипта (безвредно); `fork-identity.test.sh` без `grep -I`; prettier в `docs/deploy.md` поставил продолжения inline-кода в колонку 0 внутри списков; `tests/integration/App.test.tsx` падает в полном `test:unit` под нагрузкой машины, а отдельно проходит.
