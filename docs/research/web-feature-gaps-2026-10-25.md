# Пробелы web-версии cc-switch-web

Дата аудита: 2026-10-25. Состояние: локальная ветка `main`, коммит `4e2914de`, upstream `de0a235a` (`docs/upstream.lock:1-5`). Это статический аудит текущего исходного кода, а не обещание поведения конкретного запущенного экземпляра.

## Метод и границы

За desktop-эталон принят зарегистрированный набор Tauri-команд (`src-tauri/src/lib.rs:1518-1925`) и использующий его React UI. Web состоит из того же React UI, сборочных shim-слоёв (`vite.config.web.mts:5-31`), транспорта команд в HTTP (`src/lib/api/adapter.ts:1-9,44-307`) и Axum-сервера (`src-tauri/examples/server.rs:42-105`, `src-tauri/src/web_api.rs:62-141`).

«Нет в web» означает, что вызов доходит до `default: null` адаптера и затем завершается `WebUnsupportedError` (`src/lib/api/adapter.ts:305-336`), если только у вызывающего кода нет собственного browser fallback. Отдельно отмечены silent no-op и частично работающие функции. Одинаковый React UI сам по себе не означает поддержку: desktop-only кнопки в основном не скрыты.

Оценки трудоёмкости — для одного знакомого с репозиторием разработчика, включая базовые тесты, без полного regression-аудита всех 13 приложений:

- **S**: до 1–2 человеко-дней; локальный shim или несколько тонких HTTP-wrapper'ов.
- **M**: 2–5 человеко-дней; несколько endpoint'ов, сериализация и небольшая адаптация UI.
- **L**: 1–3 недели; нужен новый runtime/transport или существенное разделение desktop-зависимостей.
- **XL**: больше 3 недель; отдельная подсистема, безопасность и заметный новый UX.

Это оценки реализации web-аналога на **серверном хосте**. Web не управляет файлами и процессами компьютера, с которого открыт браузер: сервер работает с реальными live-config'ами хоста (`docs/deploy.md:19-25`, `src-tauri/src/web_api.rs:147-154`).

## Что уже поддерживается (не является пробелом)

HTTP API и adapter покрывают базовые CRUD/переключение/сортировку providers, импорт default config и editor view; базовую библиотеку prompts; основной список и синхронизацию MCP; просмотр installed skills, toggle/resync/uninstall; список/чтение/постраничные блоки/удаление sessions; чтение usage/логов/pricing и ручную синхронизацию usage; get/save общих settings, пути, версии CLI и проверку env conflicts (`src/lib/api/adapter.ts:46-303`, `src-tauri/src/web_api.rs:67-140`). Поэтому ниже эти возможности не перечисляются как отсутствующие целиком; отдельно разобраны только их непокрытые части.

## Подтверждённые пробелы

### 1. Локальный proxy, routing/Stack/takeover и failover — отсутствуют; **L, 10–15 человеко-дней**

**Что не работает.** Запуск/остановка proxy, переключение direct/route/Stack, live takeover, выбор маршрута, список Stack, глобальные и per-app proxy settings, failover queue, circuit breaker и health не имеют HTTP-отображения: весь `proxyApi` остаётся на Tauri-командах (`src/lib/api/proxy.ts:16-170`, `src/lib/api/failover.ts:1-99`), а adapter завершает такие команды как unsupported (`src/lib/api/adapter.ts:305-336`). Desktop-команды и нужные данные при этом существуют (`src-tauri/src/commands/proxy.rs:31-121,214-339`, `src-tauri/src/commands/failover.rs:72-177`).

**Почему не S/M.** `AppState` и `ProxyService` в headless server создаются (`src-tauri/examples/server.rs:64-66`, `src-tauri/src/store.rs:17-29`), поэтому часть thin handlers действительно проста. Но полный parity требует ещё: отделить tray refresh от `AppHandle` (`src-tauri/src/commands/proxy.rs:111-121`, `src-tauri/src/commands/failover.rs:119-150`), воспроизвести startup recovery/autostart, который находится в desktop setup (`src-tauri/src/lib.rs:731-738,1351-1369`), и решить события обновления UI. Для OAuth-провайдеров data plane сейчас явно зависит от `AppHandle` и managed states (`src-tauri/src/proxy/forwarder.rs:1424-1427,1680-1682,2215-2227`).

**Объём:** общий command/service слой вместо `tauri::State`/`AppHandle`, Axum handlers и adapter mappings, startup lifecycle, polling или SSE для статусов, тесты takeover/recovery/failover. Без OAuth-маршрутов базовый proxy можно выпустить раньше; полный parity включает пункт про managed auth ниже.

**Риск:** web server управляет proxy и live-config'ами серверного хоста; remote browser не превращает эти маршруты в локальный proxy клиентского компьютера.

### 2. Auth Center и managed OAuth-аккаунты — отсутствуют; **L, 7–12 человеко-дней**

**Что не работает.** Вход, повторный вход, polling/cancel, список аккаунтов, default/remove/logout для Codex OAuth, GitHub Copilot, xAI и Antigravity (`src/lib/api/auth.ts:3-10,46-128`), отдельный Kimi OAuth flow (`src/lib/api/kimiOAuth.ts:1-58`) и связанные official account cards не имеют web endpoints. Desktop-реализация этих операций есть (`src-tauri/src/commands/auth.rs:128-299,301-420`, `src-tauri/src/commands/kimi_oauth.rs:8-79`).

**Что переиспользовать.** Сами OAuth managers и device-code flow не требуют browser UI; polling уже является частью frontend API. Нужно перенести managed states из Tauri DI в явное web server state: сейчас они создаются только в desktop setup (`src-tauri/src/lib.rs:1271-1318`), а `server.rs` инициализирует лишь `AppState` (`src-tauri/examples/server.rs:64-66`).

**Почему L для полного parity.** Codex browser login слушает фиксированный `127.0.0.1:1455` (`src-tauri/src/proxy/providers/codex_oauth_auth/browser.rs:145-179`); Antigravity аналогично использует loopback callback (`src-tauri/src/services/agy_account/oauth.rs:89-111`). Когда авторизация проходит в браузере другого устройства, `127.0.0.1` указывает уже не на сервер. Нужен доступный через tailnet callback с проверкой state/безопасным redirect либо осознанный device-code-only UX. Дополнительно `open_external` и clipboard надо заменить browser-операциями.

**Минимальный этап:** только device-code flows и account CRUD — ориентировочно M, 3–5 дней. Полный включает browser callbacks, Kimi и интеграцию с proxy из пункта 1.

### 3. Получение списков моделей от провайдера и CLI — отсутствует; **M, 2–4 человеко-дня**

Кнопки fetch models используют неподдерживаемые `fetch_models_for_config`, `get_agy_models`, `get_opencode_models`, а также OAuth catalogs (`src/lib/api/model-fetch.ts:32-107`). Generic OpenAI/Anthropic-compatible fetch уже вынесен в handle-free service (`src-tauri/src/commands/model_fetch.rs:177-205`), поэтому его wrapper относительно прост. Antigravity и OpenCode discovery запускают CLI на хосте (`src-tauri/src/commands/model_fetch.rs:16-71,103-148`), что надо явно сохранить в семантике web и покрыть ошибками/timeout'ами. OAuth catalogs зависят от пункта 2.

**Объём:** HTTP handlers с camelCase DTO, adapter mappings, тесты ошибок и отсутствующего CLI; для OAuth — managed states.

### 4. Provider usage scripts, подписки/баланс, лимиты и connectivity check — отсутствуют; **M, 4–6 человеко-дней**

В web отсутствуют `queryProviderUsage`/`testUsageScript` (`src/lib/api/usage.ts:22-50`), официальные subscription quotas, coding plan quota и balance (`src/lib/api/subscription.ts:4-38`), `stream_check_provider` (`src/lib/api/connectivity-check.ts:21-29`) и `check_provider_limits` (`src/lib/api/usage.ts:213-219`). Desktop services и команды присутствуют (`src-tauri/src/commands/provider.rs:516-558,838-883`, `src-tauri/src/commands/stream_check.rs:13-36`, `src-tauri/src/commands/usage.rs:293-301`).

Это преимущественно thin handlers поверх готовых services, но не чистый copy-paste: provider usage и subscription обновляют cache, emit'ят события и refreshes tray через `AppHandle` (`src-tauri/src/commands/provider.rs:518-556`, `src-tauri/src/commands/subscription.rs:18-44`), а Copilot/xAI/agy требуют managed states из пункта 2. Следует отделить вычисление результата от desktop side effects; для web сначала достаточно HTTP response + React Query invalidation. Test endpoint speed относится к той же небольшой группе wrappers (`src-tauri/src/commands/provider.rs:876-883`).

## Остальной полный инвентарь пользовательских функций

Для всех перечисленных ниже команд проверено отсутствие mapping'а в `mapCommand`: после существующих categories adapter переходит в `default: null` (`src/lib/api/adapter.ts:281-307,321-336`). В таблице сгруппированы команды одной пользовательской операции; deprecated/unreferenced MCP wrappers не выдаются за отдельные функции.

| # | Функция, которой нет в web | Доказательства frontend ↔ desktop backend | Сложность / дни | Что нужно и зависимости |
|---|---|---|---|---|
| 5 | **Skills:** discovery и поиск skills.sh, install/update, репозитории, import unmanaged, backups/restore, storage migration, ZIP install | `src/lib/api/skills.ts:202-355`; `src-tauri/src/commands/skill.rs:37-69,78-148,161-220,335-378` | **M, 4–6** | Добавить `SkillServiceState` в server (сейчас только desktop setup: `src-tauri/src/lib.rs:1275-1277`), wrappers; ZIP через browser upload, не native picker. Список/toggle/resync/uninstall уже работают. |
| 6 | **Project Profiles:** list/create/rename/resnapshot/delete/apply/clear | `src/lib/api/profiles.ts:57-108`; `src-tauri/src/commands/profile.rs:101-181` | **M, 2–3** | Тонкие handlers; отделить emit событий после apply от `AppHandle` (`src-tauri/src/commands/profile.rs:63-99,171-181`). |
| 7 | **Universal Providers:** CRUD, duplicate и sync в приложения | `src/lib/api/providers.ts:279-318`; `src-tauri/src/commands/provider.rs:969-1028` | **M, 2–3** | HTTP DTO/mappings и проверка последовательности upsert→sync в текущем UI. |
| 8 | **Custom icon library:** list/add/rename/delete | `src/lib/api/customIcons.ts:17-33`; `src-tauri/src/commands/custom_icon.rs:10-35` | **S, 1–2** | Простые DB-backed handlers; данные иконки уже приходят из browser как data URI. |
| 9 | **Config import/export** и ручной `sync_current_providers_live` | `src/lib/api/settings.ts:147-161,233-240`; `src-tauri/src/commands/import_export.rs:32-98` | **M, 3–5** | Экспорт как download, импорт как upload с подтверждением/backup; не переносить native save/open dialogs (`src-tauri/src/commands/import_export.rs:101-143`). |
| 10 | **DB backups:** create/list/restore/rename/delete, backup locations и очистка | `src/lib/api/settings.ts:417-465`; `src-tauri/src/commands/import_export.rs:147-244` | **M, 2–4** | В основном wrappers; отдельно тестировать restore и concurrency. Reveal в файловом менеджере — desktop-only, для web заменить download/показом пути. |
| 11 | **WebDAV/S3:** test/save/upload/download/remote info и автоматическая синхронизация | `src/lib/api/settings.ts:163-230`; `src-tauri/src/commands/webdav_sync.rs:105-205`, `src-tauri/src/commands/s3_sync.rs:100-197`; UI: `src/components/settings/SettingsPage.tsx:485-500` | **L, 5–8** полный объём; **M, 3–4** только ручной | Manual handlers просты; auto workers запускаются только в desktop setup (`src-tauri/src/lib.rs:1262-1270`), нужны headless scheduler и замена событий. |
| 12 | **Usage administration:** edit/batch/delete pricing, pricing model source, models.dev sync config/result, Codex rebuild, data sources, last sync | `src/lib/api/usage.ts:167-237`, `src/lib/api/proxy.ts:159-169`; `src-tauri/src/commands/usage.rs:235-309,327-369` | **M, 2–3** | DB/service wrappers; после rebuild/sync явно инвалидировать queries, поскольку event bus отсутствует. Базовые dashboard/logs/manual sync уже работают. |
| 13 | **Sessions:** изображения, export Markdown, progressive/streaming transcript | `src/lib/api/sessions.ts:49-107`; `src-tauri/src/commands/session_manager.rs:37-112,232-245` | **M, 2–4** | Binary HTTP response для images, Blob download для Markdown; SSE лишь для progressive loading. Обычное чтение уже имеет fallback на supported `getMessages` (`src/lib/query/sessions.ts:103-155`). Terminal вынесен ниже. |
| 14 | **OpenCode:** import live, live provider IDs, default model get/set | `src/lib/api/providers.ts:221-244`; `src-tauri/src/commands/provider.rs:1033-1050` | **M, 1–2** | Thin wrappers над live config сервера; CLI model discovery входит в пункт 3. |
| 15 | **OpenClaw:** live import/IDs/provider, health, default model/catalog/agents/env/tools; workspace files и daily memory CRUD/search | `src/lib/api/openclaw.ts:20-121`, `src/lib/api/providers.ts:246-268`, `src/lib/api/workspace.ts:20-55`; `src-tauri/src/commands/openclaw.rs:16-127`, `src-tauri/src/commands/workspace.rs:60-343` | **M, 3–5** | Преимущественно handle-free file/config operations; open directory не переносить. Проверить path allowlists и atomic writes через HTTP-тесты. |
| 16 | **Hermes:** live import/IDs, model config, memory read/write/limits/enabled | `src/lib/api/hermes.ts:18-67`, `src/lib/api/providers.ts:254-275`; `src-tauri/src/commands/hermes.rs:22-87` | **M, 2–3** | Thin wrappers. Открытие Hermes Web UI — отдельный browser link до **серверного** адреса; `hermes dashboard` относится к terminal. |
| 17 | **Pi / Oh My Pi:** current state, usage-script update, Pi session discovery; native SYSTEM/APPEND files и slash-command templates | `src/lib/api/pi.ts:26-45` (включая динамическую команду `get_${app}_current_state`), `src/lib/api/prompts.ts:63-112`; `src-tauri/src/commands/pi.rs:11-30`, `src-tauri/src/commands/omp.rs:23-28`, `src-tauri/src/commands/prompt.rs:89-133` | **M, 3–5** | Handlers + adapter для динамических Pi/OMP команд; revision/conflict DTO для native prompts. Общая библиотека prompts уже работает. |
| 18 | **OMO / OMO Slim:** read local file, current provider, disable current | `src/lib/api/omo.ts:4-17`; `src-tauri/src/commands/omo.rs:8-66` | **M, 1–2** | Thin wrappers и tests переключения live config. |
| 19 | **Claude Desktop integration и official cards:** status/default routes/import from Claude; ensure official для Claude Desktop/Codex/GrokBuild; Claude Desktop route mode | `src/lib/api/providers.ts:31-46,159-181`; desktop registry `src-tauri/src/lib.rs:1529-1534` | **M, 3–5** | Config handlers просты; полный route mode зависит от proxy из пункта 1 и server-host installation semantics. |
| 20 | **App-specific settings:** Claude plugin/onboarding; Codex multi-agent probe, compression, disk usage, unified-history restore | `src/lib/api/settings.ts:42-70,132-145`; `src-tauri/src/commands/settings.rs:178-251`; registry `src-tauri/src/lib.rs:1577-1582` | **M, 2–4** | В основном wrappers; restore тестировать отдельно. Side effects общего `save_settings` частично уже адаптированы в `src-tauri/src/web_api.rs:946-984`. |
| 21 | **Endpoint speed test + custom endpoints + read live provider settings** | `src/lib/api/vscode.ts:12-70`; `src-tauri/src/commands/provider.rs:870-955` | **M, 1–3** | Простые service/DB wrappers; network test выполняется с сервера, поэтому latency не равна latency браузера. |
| 22 | **Env conflicts:** удаление переменных с backup и restore | `src/lib/api/env.ts:19-36`; `src-tauri/src/commands/env.rs:8-22` | **S, 1–2** | Два handlers и tests; detection уже поддержан (`src-tauri/src/web_api.rs:1023-1033`). Требуется осторожность с OS user environment сервера. |
| 23 | **Global upstream/outbound proxy:** URL set/get, test, status, scan local proxies | `src/lib/api/globalProxy.ts:36-85`; `src-tauri/src/commands/global_proxy.rs:15-87,89-227` | **M, 2–3** | Handlers + инициализация сохранённого HTTP client в headless startup; desktop делает это в `src-tauri/src/lib.rs:1320-1348`. Не путать с local gateway из пункта 1. |
| 24 | **Rectifier/optimizer и log config** | `src/lib/api/settings.ts:328-349`; `src-tauri/src/commands/settings.rs:731-813`; UI `src/components/settings/RectifierConfigPanel.tsx:35-65`, `src/components/settings/LogConfigPanel.tsx:25-47` | **M, 2–3** | DB/settings wrappers. Для логов web предпочтительны view/download, а не reveal в OS file manager. |
| 25 | **Apps page:** install/update/uninstall CLI, uninstall preview, installation diagnostics | `src/lib/api/settings.ts:285-325`; `src-tauri/src/commands/misc.rs:315-382,4729-4790`; UI `src/components/apps/useToolManagement.ts:517-605,670-677` | **M, 3–5** | Команды уже исполняют и ждут процесс без visible terminal (`src-tauri/src/commands/misc.rs:384-420`), поэтому возможны wrappers с timeout/cancellation. Версии CLI уже поддержаны; операции выполняются на сервере. |
| 26 | **Изменение CC Switch data directory и restart** | `src/lib/api/settings.ts:72-130`; `src-tauri/src/commands/settings.rs:253-274,375-390`; UI `src/components/settings/SettingsPage.tsx:274-310,408-448` | **M, 3–5** | Tauri Store/AppHandle заменить серверной конфигурацией (env/config), определить атомарность смены DB и restart через systemd/supervisor. Обычный текстовый ввод пути возможен без native picker. |
| 27 | **Deep-link config import:** parse/merge/icon fetch/unified import | `src/lib/api/deeplink.ts:114-152`; desktop registry `src-tauri/src/lib.rs:1680-1687` | **M, 2–4** | Backend operations можно expose; для browser нужен paste-URL/upload flow. Получение `ccswitch://` из OS и pending queue — desktop-only; сейчас peek/ack молча возвращают null (`src/lib/api/adapter.ts:323-333`). |
| 28 | **Проверка и установка обновлений CC Switch** | `src/lib/api/settings.ts:72-89`; `src/contexts/UpdateContext.tsx:62-97,119-127`; desktop `src-tauri/src/commands/settings.rs:277-373` | **M, 2–4** проверка+инструкция; **L, 5–10** safe self-update | `check_for_updates` сейчас silent no-op (`src/lib/api/adapter.ts:323-333`), install вызывает unsupported. Tauri updater не подходит systemd/Docker; нужен release manifest и отдельный update/restart механизм либо скрыть кнопки. |
| 29 | **Realtime events:** provider switched, usage/proxy updates, sync status, config/deeplink events | `src/lib/web-shims/event.ts:1-20`; подписчики `src/lib/api/providers.ts:194-200`, `src/hooks/useUsageEventBridge.ts:15-31`, `src/main.tsx:82-90` | **M, 3–5** | SSE/WebSocket + reconnect и query invalidation. Сейчас `listen`/`once` только возвращают no-op unlisten; не путать с уже работающими явными refetch после mutations. |
| 30 | **Открытие внешних HTTP(S) ссылок** | `src/lib/api/settings.ts:243-254`; desktop `src-tauri/src/commands/misc.rs:50-58`; пример видимых кнопок `src/components/settings/AboutSection.tsx:274-303` | **S, 0.5–1** | Browser `window.open`/`location.assign` по user gesture с `noopener,noreferrer`; backend не нужен. Это ломает также OAuth links из пункта 2. |
| 31 | **Import prompt из файла** | `src/lib/api/prompts.ts:51-53`; `src-tauri/src/commands/prompt.rs:56-63` | **S, 1–2** | `<input type=file>` + upload/text parse вместо native dialog; остальные базовые prompt operations уже есть. |
| 32 | **Startup/migration parity:** legacy config.json→DB, default skills seed/migration и общая инициализация services | Desktop orchestration `src-tauri/src/lib.rs:597-738,744-794`; web startup только `Database::init` + `AppState::new` (`src-tauri/examples/server.rs:64-66`) | **M, 3–5** | Вынести общий bootstrap из Tauri setup, а не копировать его целиком; покрыть first-run/upgrade paths. Это инфраструктурная зависимость пунктов 1, 5, 11, 23, а не отдельная кнопка. |

## Частичная поддержка и silent no-op

- **Clipboard не отнесён к полностью отсутствующим.** После ошибки Tauri-команды есть fallback в `navigator.clipboard.writeText` (`src/lib/clipboard.ts:3-17`). Но при обычном HTTP через tailnet browser secure-context/permission requirements могут не дать ему сработать; это нужен runtime-тест на целевых клиентах, а не новый backend.
- **Обычное чтение sessions работает.** Неподдерживаемый streaming вызов падает до получения первого chunk, после чего `loadTranscript` использует supported `get_session_messages` (`src/lib/query/sessions.ts:103-155`). Поэтому в пункт 13 входят progressive loading, images и export, а не весь session reader.
- **Светлая/тёмная тема браузера работает.** Цвет применяется к DOM и сохраняется в localStorage (`src/components/theme-provider.tsx:71-112`); unsupported `set_window_theme` относится только к native title bar и его ошибка игнорируется (`src/components/theme-provider.tsx:114-144`).
- **Молчаливые no-op:** `tray_app_page_seen`, `take_tray_navigation`, `peek_pending_deeplink`, `ack_pending_deeplink`, `check_for_updates` и все `plugin:*` возвращают `null` (`src/lib/api/adapter.ts:321-334`). Window controls также являются no-op (`src/lib/web-shims/window.ts:1-34`). Для tray это ожидаемо; для update/deeplink это скрывает отсутствие функции.
- **Нет систематического feature gating.** `isWeb` объявлен в transport (`src/lib/api/adapter.ts:12-13`), но основной UI продолжает вызывать desktop API напрямую — например proxy на старте (`src/App.tsx:260-271`) и update check (`src/contexts/UpdateContext.tsx:119-127`). Поэтому пользователь видит много controls, которые завершатся `WebUnsupportedError`, а не аккуратно скрытый web-режим.

## Принципиально desktop-only или требующие другого UX

| Функция | Текущее доказательство | Web-аналог и сложность |
|---|---|---|
| **Tray menu/panel, tray navigation, minimize-to-tray, native window minimize/maximize/close/decorations** | Desktop registry `src-tauri/src/lib.rs:1688,1734-1740`; no-op `src/lib/web-shims/window.ts:1-34`, `src/lib/web-shims/event.ts:1-20` | Переносить не нужно: скрыть native controls (**S, 1–2 дня**). Трей нельзя воспроизвести обычной web-страницей. |
| **OS autostart / silent startup** | `src/lib/api/settings.ts:256-262`; desktop `src-tauri/src/commands/settings.rs:392-725` | Для desktop-клиента неприменимо. Для server-daemon аналог — установка/enable systemd unit; он уже частично создаётся install script (`docs/deploy.md:5-17`). UI управления unit: **M, 2–4 дня**. |
| **Native open/save/directory pickers и reveal/open folder/log/session/backup/skills/workspace paths** | `src-tauri/src/commands/import_export.rs:101-143`, `src/lib/api/sessions.ts:96-107`, `src/lib/api/workspace.ts:53-55` | Upload/download и отображение server path — **S–M на каждый flow** (учтено в пунктах 9, 10, 13, 31). Полноценный remote file manager — отдельный **L** с path security; открыть файловый менеджер клиента из browser нельзя. |
| **Provider/session terminal и `hermes dashboard`** | `src/lib/api/providers.ts:203-219`, `src/lib/api/sessions.ts:124-135`, `src/lib/api/hermes.ts:31-34`; desktop process launch `src-tauri/src/commands/session_manager.rs:160-201` | Копирование resume/command — **S, 1–2 дня**. Настоящий интерактивный web terminal — **XL, 15–25 дней**: PTY, WebSocket, resize, auth, audit, lifecycle. Запуск терминала на клиентском компьютере из обычной страницы невозможен. |
| **Регистрация и приём `ccswitch://` из OS, pending deep-link queue** | `src/lib/api/deeplink.ts:100-112`; desktop registry `src-tauri/src/lib.rs:1680-1688` | Сам custom protocol относится к installed app. В web использовать paste-URL/import из пункта 27 (**M**), не pending no-op. |
| **Tauri lightweight mode / tray-only lifecycle** | `src-tauri/src/commands/lightweight.rs:1-14`, registry `src-tauri/src/lib.rs:1921-1924` | Не требуется для browser console; server daemon управляется через service supervisor. |

## Практический приоритет

1. **Сначала 2–3 дня на честный web UX:** скрыть/disable неподдерживаемые controls по `isWeb`, не возвращать «up to date» после no-op, показывать server-host semantics. Это не добавляет parity, но убирает ложное впечатление рабочих кнопок.
2. **Быстрый полезный пакет:** пункты 6–10, 14–24 и 30–31. Здесь преобладают существующие services и thin wrappers; общий adapter/request/error test harness даст наибольший эффект.
3. **Следующий пакет:** Skills (5), usage administration (12), sessions (13), Apps lifecycle (25), realtime events (29), затем quotas/models (3–4).
4. **Отдельные архитектурные эпики:** proxy + OAuth (1–2 и зависимые 19), общий bootstrap (32), cloud auto-sync (11), safe self-update (28), web terminal (если действительно нужен).

Оценки нельзя складывать механически: общий headless bootstrap, managed auth state, event transport и upload/download helpers устраняют повторную работу сразу в нескольких строках. Multi-user/auth и управление файлами **клиентского** компьютера в оценку не входят; текущий сервер намеренно рассчитан на доверенный tailnet без in-app auth (`docs/deploy.md:1-3`, `src-tauri/examples/server.rs:3-14,56-62`).

## Проверка и ограничения аудита

Проверены mappings всех найденных frontend `invoke(...)`-вызовов, включая динамические Pi/OMP, silent no-op и основные UI call sites. Для ключевых негативных утверждений выполнен repo-wide поиск по `src`, `src-tauri/src`, `tests` и `src-tauri/tests`. Специальных тестов web adapter/HTTP routes по маркерам `web_api`, `WebUnsupported`, `TRUSTED_TAILNET` не найдено; предложенные оценки включают создание базового покрытия. Приложение и каждая операция в живом browser/desktop не запускались: это статический аудит исходников на указанном коммите.
