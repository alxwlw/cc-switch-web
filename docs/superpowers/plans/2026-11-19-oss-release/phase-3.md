# Phase 3 — первый релиз, переезд развёртываний, проверка самообновления

Цель: опубликованы два релиза подряд; установка из релиза на agentbox обновилась из консоли (критерий 4 спеки); развёртывания владельца работают на артефактах релиза.

Начинать после слияния фазы 2. Каждый тег — только после «выпускай» владельца на этот релиз (`plan-state block --ask`, ответ — `plan-state answer`).

Спека: `docs/superpowers/specs/2026-11-19-oss-release-design.md`, критерии 1–4 и раздел 8.

Факты:
- На agentbox есть systemd user unit `cc-switch-web` (сборка из исходников, `HOST=100.123.155.62`, сейчас `inactive`) и приложение Dokploy `infra-cc-switch-web-eia5p8` (образ `cc-switch-web-fork:<sha>` локальной сборки, порт 8890, `pull_policy: never`, `HOME=/home/alex`). Судя по памяти сессий, оба используют один каталог данных `/home/alex/.cc-switch-web`. Перед Task 2 это нужно проверить (`CC_SWITCH_CONFIG_DIR` в compose Dokploy). Два сервера на одной БД и одном порту прокси `15721` одновременно работать не должны.
- Ключ Dokploy (`op://env-devsrvr/wf-sync/DOKPLOY_API_KEY`) читает только владелец: скрипт передаётся через `HANDOFF_OP_VAULTS=env-devsrvr handoff <script>`.

### Task 1: релиз `-web.1`

- [ ] **Step 1:** PR `release: v<версия>-web.1` в `main`: в `CHANGELOG.md` «unreleased» → дата (`YYYY-MM-DD`). Draft, сливает владелец.
- [ ] **Step 2:** после слияния — вопрос владельцу (`plan-state block <task> --ask <file>`): тег, коммит `main`, что будет опубликовано (два архива, `SHA256SUMS`, `install.sh`, образ `ghcr.io/alxwlw/cc-switch-web:<версия>` и `:latest`), что все будущие установки с самообновлением пойдут по релизам. Конец хода.
- [ ] **Step 3:** после «выпускай»: `git tag -a v<версия>-web.1 <sha> -m "v<версия>-web.1"`, `git push origin v<версия>-web.1`; `gh run watch` → все jobs `Release (web)` зелёные, `publish` выполнен.
- [ ] **Step 4: проверка релиза.** `gh release view v<версия>-web.1 -R alxwlw/cc-switch-web --json assets` — ровно 4 ассета; `curl -fsSL https://api.github.com/repos/alxwlw/cc-switch-web/releases/latest` → этот тег; `curl -fsSLI https://github.com/alxwlw/cc-switch-web/releases/latest/download/install.sh` → 200. Образ анонимно: `docker logout ghcr.io; docker pull ghcr.io/alxwlw/cc-switch-web:<версия>`; отказ → пакет приватный: видимость пакета в настройках GitHub → Public через skill `web-ui` (владельцу — только подтверждение 2FA, если спросят), затем pull снова.

### Task 2: установка из релиза на agentbox

- [ ] **Step 1:** вопрос владельцу (`--ask`): какой экземпляр на agentbox остаётся основным — (а) systemd-установка из релиза, а приложение Dokploy переключается на образ ghcr и останавливается (рекомендация: это основной путь продукта и на нём проверяется самообновление); (б) Dokploy остаётся основным, systemd-установка поднимается только на время проверки самообновления при остановленном Dokploy. Конец хода.
- [ ] **Step 2:** по ответу остановить второй экземпляр, затем как `alex`: `curl -fsSL https://github.com/alxwlw/cc-switch-web/releases/latest/download/install.sh | bash -s -- --tailscale` → вывод с версией и адресом tailnet; `systemctl --user is-active cc-switch-web` → `active`; `curl -fsS http://100.123.155.62:18766/api/capabilities` → `"webVersion":"<версия>-web.1"`, `"selfUpdate":true`; консоль по адресу tailnet открывается (Playwright MCP, снимок About), провайдеры на месте.
- [ ] **Step 3:** `plan-state journal`: что запущено, что остановлено, где данные.

### Task 3: релиз `-web.2` и самообновление из консоли

- [ ] **Step 1:** PR `release: v<версия>-web.2`: раздел `## v<версия>-web.2 — <дата>` в `CHANGELOG.md` с тем, что изменилось после `-web.1` (находки Task 1–2; если их нет — только этот раздел и правки документации). Draft, сливает владелец.

  Состав `-web.2` (решено 2026-11-20; владелец ответил `go` на вопрос Task 1, где ссылки апстрима в About отнесены к `-web.2`). Остальные пункты — находки Task 2 на консоли agentbox. Меняется только web-режим, desktop-ветка `AboutSection.tsx` остаётся как есть, и тесты это доказывают.
  1. `src-tauri/src/web_api.rs`, `capabilities_payload`: новое поле `projectUrl` = `https://github.com/<repo>`. `<repo>` берётся из той же конфигурации, что и проверка обновлений (`services/web_update.rs`: `CC_SWITCH_WEB_UPDATE_REPO`, по умолчанию `alxwlw/cc-switch-web`). Значение — параметр функции, как `build_sha`, чтобы тест мог его задать. Unit-тест рядом с `capabilities_payload_carries_the_web_version`. Поле `runtime_boundaries` в API не меняется.
  2. `src/lib/api/capabilities.ts`: `projectUrl: string | null`. Нормализация такая же, как у остальных полей: не строка или пустая строка → `null`.
  3. `src/components/settings/AboutSection.tsx`, только при `isWeb`:
     - кнопка «Official Website» (`https://ccswitch.io`) не рендерится;
     - «Release Notes» открывает `<projectUrl>/releases/tag/v<webVersion>`; без `webVersion` (сборка из исходников) — `<projectUrl>/releases`; без `projectUrl` кнопки нет;
     - кнопка GitHub показывается всегда, независимо от отказа от просьбы о звезде, и открывает `projectUrl`; без `projectUrl` её нет;
     - баннер «Star on GitHub» не рендерится.
  4. `src/components/web/WebModeInfo.tsx`:
     - строка `Server version v<version>` становится `Based on CC Switch v<version>`: новый ключ `web.mode.basedOn` во всех 8 локалях, `web.mode.serverVersion` удаляется, если больше нигде не используется;
     - абзац с сырой строкой `runtime_boundaries` убирается; подсказка `title` в `WebHostBanner.tsx` остаётся.
  5. `vite.config.web.mts`: плагин с `transformIndexHtml` ставит `<title>cc-switch-web</title>` в `index.html` web-сборки. Сейчас там апстримовское `Claude Code 供应商切换器`. Сам `src/index.html` (файл апстрима) не меняется, `tray-panel.html` тоже. Функцию замены покрывает тест, и сборка `build:web` даёт этот title в `dist-web/index.html`.
  6. `CHANGELOG.md`: раздел `## v4.0.35-web.2 — 2026-11-20` над `-web.1`, по-английски, пункты 3–5 с точки зрения пользователя. Если PR сливается не 2026-11-20, дата правится перед тегом.

  Не входит: подзаголовок диалога What's New `What changed since v4.0.20-web`. Это старое значение `whatsNewSeenVersion` из прежних web-сборок, оно показывается один раз и после «Got it» заменяется (файл апстрима `WhatsNewNotice.tsx`). Ссылка «View full changelog» ведёт на changelog апстрима на ccswitch.io — это изменения самого приложения, она верна.
- [ ] **Step 2:** вопрос «выпускай» (как в Task 1), тег, `gh run watch`, проверка релиза (Task 1, Step 4).
- [ ] **Step 3: самообновление.** Консоль установки из Task 2 (Playwright MCP): Settings → About → «Check for updates» → «Version <версия>-web.2 is available» → «Update now» → подтверждение → стадии `checking → downloading → verifying → installing → restarting` → страница перезагружается сама → About показывает `v<версия>-web.2`. Затем `cat ~/.local/state/cc-switch-web/update-status.json` → `"stage":"done"`, `"toVersion":"<версия>-web.2"`; `~/.local/bin/cc-switch-web.prev` есть.
- [ ] **Step 4:** критерий 4 спеки выполнен — `plan-state journal` со ссылками на оба релиза.

### Task 4: приложение Dokploy на образе из ghcr

- [ ] **Step 1:** скрипт в scratchpad: через API Dokploy заменить `image:` в compose `infra-cc-switch-web-eia5p8` на `ghcr.io/alxwlw/cc-switch-web:<версия>-web.2`, убрать `pull_policy: never`, задеплоить (если по ответу Task 2 приложение должно быть остановлено — после деплоя остановить). Передать владельцу: `HANDOFF_OP_VAULTS=env-devsrvr handoff <script>`, в ответе — одна строка, которую печатает `handoff`.
- [ ] **Step 2:** после выполнения: если приложение запущено — `/api/update/check?force=1` → `up-to-date`; если остановлено — в compose стоит образ ghcr.
- [ ] **Step 3:** обновить память `agentbox-dokploy-fork-image`: образ теперь из ghcr, локальная сборка больше не нужна; какой экземпляр основной.

### Task 5: закрытие

- [ ] **Step 1:** индекс плана `**Status:** closed`, фазы отмечены; спека `**Status:** implemented (<дата>)`. Draft PR, после слияния — `plan-state finish`.
- [ ] **Step 2:** итог владельцу: ссылки на релизы, на образ, что открыто (отвязка от сети форков, если ещё не сделана; находки, не попавшие в релизы).
