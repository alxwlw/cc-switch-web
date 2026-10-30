# Plane backlog: cc-switch-web parity

Дата: 2026-10-25.

Источник: `docs/research/web-feature-gaps-2026-10-25.md` (`main@4e2914de`).

Plane: проект `tooling` (`20ca9902-8c99-465b-8919-30de57db4b43`), модуль `M5 — cc-switch-web-parity` (`c2c8690c-443f-4dd4-8ad5-cab9a00f6a00`).

Формат: WEB-00 — стартовая сквозная задача; WEB-01…WEB-32 соответствуют пунктам аудита. Оценки — человеко-дни одного знакомого с репозиторием разработчика, включая базовые тесты. Все операции с файлами, CLI и proxy выполняются на серверном хосте, а не на компьютере с браузером.

### WEB-00 — Честный feature gating web-режима

- **Plane external_id:** `2026-10-25/web-00`
- **Оценка:** 2–3 человеко-дня.
- **Объём:** по `isWeb` скрыть или disable desktop-only controls; update check не должен выглядеть успешным после silent no-op; явно показать server-host semantics. Не добавлять parity в рамках этой задачи.
- **Доказательства:** `src/lib/api/adapter.ts:12-13,321-336`, `src/App.tsx:260-271`, `src/contexts/UpdateContext.tsx:62-97,119-127`.
- **Результат:** пользователь не видит активных controls, которые заведомо завершатся `WebUnsupportedError` или фиктивным `null`.

### WEB-01 — Local proxy, routing/Stack, takeover и failover

- **Plane external_id:** `2026-10-25/web-01`
- **Оценка:** L, 10–15 человеко-дней.
- **Объём:** start/stop proxy, direct/route/Stack, live takeover, выбор маршрута, global/per-app settings, failover queue, circuit breaker и health. Вынести общий command/service слой, добавить Axum handlers и adapter mappings, startup recovery, polling/SSE и тесты takeover/recovery/failover. Базовый proxy без OAuth-маршрутов можно выпустить отдельным этапом.
- **Доказательства:** `src/lib/api/proxy.ts:16-170`, `src/lib/api/failover.ts:1-99`, `src-tauri/src/commands/proxy.rs:31-121,214-339`, `src-tauri/src/commands/failover.rs:72-177`.
- **Зависимости:** WEB-32; OAuth data plane — WEB-02; realtime updates — WEB-29. Управление только proxy серверного хоста.

### WEB-02 — Auth Center и managed OAuth

- **Plane external_id:** `2026-10-25/web-02`
- **Оценка:** L, 7–12 человеко-дней; device-code flows и account CRUD отдельным этапом — M, 3–5 дней.
- **Объём:** login/reauth/poll/cancel, accounts/default/remove/logout для Codex OAuth, GitHub Copilot, xAI и Antigravity; Kimi OAuth и official account cards. Перенести managed states из Tauri DI в web server state. Для browser login заменить loopback callback на безопасный tailnet flow либо явно ограничить UX device code; заменить `open_external` и clipboard browser-операциями.
- **Доказательства:** `src/lib/api/auth.ts:3-10,46-128`, `src/lib/api/kimiOAuth.ts:1-58`, `src-tauri/src/commands/auth.rs:128-299,301-420`, `src-tauri/src/commands/kimi_oauth.rs:8-79`, `src-tauri/src/lib.rs:1271-1318`.
- **Зависимости:** WEB-30, WEB-32; интеграция с proxy — WEB-01.

### WEB-03 — Получение списков моделей

- **Plane external_id:** `2026-10-25/web-03`
- **Оценка:** M, 2–4 человеко-дня.
- **Объём:** HTTP handlers и adapter mappings для `fetch_models_for_config`, `get_agy_models`, `get_opencode_models` и OAuth catalogs; camelCase DTO, тесты ошибок, timeout и отсутствующего CLI. Generic OpenAI/Anthropic-compatible fetch переиспользует handle-free service; CLI discovery выполняется на сервере.
- **Доказательства:** `src/lib/api/model-fetch.ts:32-107`, `src-tauri/src/commands/model_fetch.rs:16-71,103-148,177-205`.
- **Зависимости:** OAuth catalogs — WEB-02.

### WEB-04 — Provider usage, подписки, баланс и connectivity

- **Plane external_id:** `2026-10-25/web-04`
- **Оценка:** M, 4–6 человеко-дней.
- **Объём:** `queryProviderUsage`/`testUsageScript`, official subscription quotas, coding plan quota, balance, `stream_check_provider` и `check_provider_limits`. Отделить вычисление результата и cache от `AppHandle`/tray side effects; для web вернуть HTTP response и инвалидировать React Query. Endpoint speed test принадлежит WEB-21 и здесь не дублируется.
- **Доказательства:** `src/lib/api/usage.ts:22-50,213-219`, `src/lib/api/subscription.ts:4-38`, `src/lib/api/connectivity-check.ts:21-29`, `src-tauri/src/commands/provider.rs:516-558,838-883`, `src-tauri/src/commands/stream_check.rs:13-36`.
- **Зависимости:** Copilot/xAI/agy managed states — WEB-02.

### WEB-05 — Skills discovery, install/update, репозитории и backups

- **Plane external_id:** `2026-10-25/web-05`
- **Оценка:** M, 4–6 человеко-дней.
- **Объём:** discovery и поиск skills.sh, install/update, репозитории, import unmanaged, backups/restore, storage migration и ZIP install через browser upload. Добавить `SkillServiceState` в headless server и HTTP wrappers. Уже поддержанные list/toggle/resync/uninstall не переделывать.
- **Доказательства:** `src/lib/api/skills.ts:202-355`, `src-tauri/src/commands/skill.rs:37-69,78-148,161-220,335-378`, `src-tauri/src/lib.rs:1275-1277`.
- **Зависимости:** WEB-32.

### WEB-06 — Project Profiles

- **Plane external_id:** `2026-10-25/web-06`
- **Оценка:** M, 2–3 человеко-дня.
- **Объём:** list/create/rename/resnapshot/delete/apply/clear через тонкие HTTP handlers и adapter mappings. Отделить emit событий после apply от `AppHandle`; для web явно обновлять affected queries.
- **Доказательства:** `src/lib/api/profiles.ts:57-108`, `src-tauri/src/commands/profile.rs:63-99,101-181`.
- **Зависимости:** общий realtime refresh может быть переиспользован после WEB-29, но не блокирует HTTP-вариант.

### WEB-07 — Universal Providers

- **Plane external_id:** `2026-10-25/web-07`
- **Оценка:** M, 2–3 человеко-дня.
- **Объём:** CRUD, duplicate и sync универсальных providers в приложения. Добавить HTTP DTO/mappings и проверить последовательность upsert→sync в текущем UI, включая ошибки частично выполненной синхронизации.
- **Доказательства:** `src/lib/api/providers.ts:279-318`, `src-tauri/src/commands/provider.rs:969-1028`.
- **Зависимости:** нет; realtime refresh может позже использовать WEB-29.

### WEB-08 — Пользовательская библиотека иконок

- **Plane external_id:** `2026-10-25/web-08`
- **Оценка:** S, 1–2 человеко-дня.
- **Объём:** list/add/rename/delete через DB-backed HTTP handlers и adapter mappings. Входные данные иконки уже приходят из browser как data URI; native file picker не требуется.
- **Доказательства:** `src/lib/api/customIcons.ts:17-33`, `src-tauri/src/commands/custom_icon.rs:10-35`.
- **Зависимости:** нет.

### WEB-09 — Import/export конфигурации и sync live

- **Plane external_id:** `2026-10-25/web-09`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** export как browser download, import как upload с подтверждением и backup; ручной `sync_current_providers_live`. Не переносить native save/open dialogs. Проверить ошибки загрузки, подтверждение destructive import и результат синхронизации live-config.
- **Доказательства:** `src/lib/api/settings.ts:147-161,233-240`, `src-tauri/src/commands/import_export.rs:32-98,101-143`.
- **Зависимости:** автоматический pre-import backup может переиспользовать WEB-10.

### WEB-10 — DB backups

- **Plane external_id:** `2026-10-25/web-10`
- **Оценка:** M, 2–4 человеко-дня.
- **Объём:** create/list/restore/rename/delete, backup locations и очистка через HTTP wrappers. Отдельно покрыть restore и concurrency. Reveal в OS file manager заменить download или отображением server path.
- **Доказательства:** `src/lib/api/settings.ts:417-465`, `src-tauri/src/commands/import_export.rs:147-244`.
- **Зависимости:** нет; WEB-09 может использовать создание backup перед import.

### WEB-11 — WebDAV/S3 sync

- **Plane external_id:** `2026-10-25/web-11`
- **Оценка:** L, 5–8 человеко-дней полный объём; M, 3–4 дня только ручной режим.
- **Объём:** test/save/upload/download/remote info для WebDAV и S3; для автоматической синхронизации вынести workers в headless scheduler, восстановление при startup и замену desktop events. Ручной режим выпустить независимым этапом.
- **Доказательства:** `src/lib/api/settings.ts:163-230`, `src-tauri/src/commands/webdav_sync.rs:105-205`, `src-tauri/src/commands/s3_sync.rs:100-197`, `src-tauri/src/lib.rs:1262-1270`, `src/components/settings/SettingsPage.tsx:485-500`.
- **Зависимости:** WEB-32; sync status events — WEB-29.

### WEB-12 — Администрирование usage и pricing

- **Plane external_id:** `2026-10-25/web-12`
- **Оценка:** M, 2–3 человеко-дня.
- **Объём:** edit/batch/delete pricing, pricing model source, models.dev sync config/result, Codex rebuild, data sources и last sync. Добавить DB/service wrappers; после rebuild/sync явно инвалидировать queries. Базовые dashboard/logs/manual sync уже работают.
- **Доказательства:** `src/lib/api/usage.ts:167-237`, `src/lib/api/proxy.ts:159-169`, `src-tauri/src/commands/usage.rs:235-309,327-369`.
- **Зависимости:** нет; WEB-29 может позже заменить явную инвалидацию на realtime refresh.

### WEB-13 — Sessions images, Markdown export и streaming

- **Plane external_id:** `2026-10-25/web-13`
- **Оценка:** M, 2–4 человеко-дня.
- **Объём:** binary HTTP response для images, Blob download для Markdown export, progressive/streaming transcript. SSE нужен только для progressive loading; обычное чтение уже имеет fallback на supported `getMessages` и не требует переписывания. Terminal в задачу не входит.
- **Доказательства:** `src/lib/api/sessions.ts:49-107`, `src-tauri/src/commands/session_manager.rs:37-112,232-245`, `src/lib/query/sessions.ts:103-155`.
- **Зависимости:** WEB-29 может предоставить транспорт для progressive updates, но не обязателен для images/export.

### WEB-14 — OpenCode live import и default model

- **Plane external_id:** `2026-10-25/web-14`
- **Оценка:** M, 1–2 человеко-дня.
- **Объём:** import live, live provider IDs, default model get/set через thin wrappers над live config серверного хоста. CLI model discovery входит в WEB-03 и здесь не дублируется.
- **Доказательства:** `src/lib/api/providers.ts:221-244`, `src-tauri/src/commands/provider.rs:1033-1050`.
- **Зависимости:** CLI model discovery — WEB-03.

### WEB-15 — OpenClaw config, workspace и memory

- **Plane external_id:** `2026-10-25/web-15`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** live import/IDs/provider, health, default model/catalog/agents/env/tools; workspace files и daily memory CRUD/search. Переиспользовать handle-free file/config operations; через HTTP-тесты закрепить path allowlists и atomic writes. Open directory не переносить.
- **Доказательства:** `src/lib/api/openclaw.ts:20-121`, `src/lib/api/providers.ts:246-268`, `src/lib/api/workspace.ts:20-55`, `src-tauri/src/commands/openclaw.rs:16-127`, `src-tauri/src/commands/workspace.rs:60-343`.
- **Зависимости:** нет.

### WEB-16 — Hermes config и memory

- **Plane external_id:** `2026-10-25/web-16`
- **Оценка:** M, 2–3 человеко-дня.
- **Объём:** live import/IDs, model config, memory read/write/limits/enabled через thin wrappers. Открытие Hermes Web UI заменить browser link на серверный адрес; запуск `hermes dashboard` в terminal не входит.
- **Доказательства:** `src/lib/api/hermes.ts:18-67`, `src/lib/api/providers.ts:254-275`, `src-tauri/src/commands/hermes.rs:22-87`.
- **Зависимости:** browser link использует WEB-30.

### WEB-17 — Pi / Oh My Pi state, sessions и prompts

- **Plane external_id:** `2026-10-25/web-17`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** current state, usage-script update, Pi session discovery; native SYSTEM/APPEND files и slash-command templates. Добавить handlers и adapter для динамических Pi/OMP команд, revision/conflict DTO для native prompts. Общая библиотека prompts уже работает.
- **Доказательства:** `src/lib/api/pi.ts:26-45`, `src/lib/api/prompts.ts:63-112`, `src-tauri/src/commands/pi.rs:11-30`, `src-tauri/src/commands/omp.rs:23-28`, `src-tauri/src/commands/prompt.rs:89-133`.
- **Зависимости:** нет.

### WEB-18 — OMO / OMO Slim live config

- **Plane external_id:** `2026-10-25/web-18`
- **Оценка:** M, 1–2 человеко-дня.
- **Объём:** read local file, current provider, disable current через thin wrappers и тесты переключения live config на серверном хосте.
- **Доказательства:** `src/lib/api/omo.ts:4-17`, `src-tauri/src/commands/omo.rs:8-66`.
- **Зависимости:** нет.

### WEB-19 — Claude Desktop integration и official cards

- **Plane external_id:** `2026-10-25/web-19`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** status/default routes/import from Claude; ensure official для Claude Desktop/Codex/GrokBuild; Claude Desktop route mode. Config handlers сделать thin wrappers; явно сохранить server-host installation semantics.
- **Доказательства:** `src/lib/api/providers.ts:31-46,159-181`, `src-tauri/src/lib.rs:1529-1534`.
- **Зависимости:** полный Claude Desktop route mode — WEB-01.

### WEB-20 — App-specific settings Claude и Codex

- **Plane external_id:** `2026-10-25/web-20`
- **Оценка:** M, 2–4 человеко-дня.
- **Объём:** Claude plugin/onboarding; Codex multi-agent probe, compression, disk usage и unified-history restore. В основном thin wrappers; restore покрыть отдельными тестами. Side effects общего `save_settings`, уже адаптированные для web, не дублировать.
- **Доказательства:** `src/lib/api/settings.ts:42-70,132-145`, `src-tauri/src/commands/settings.rs:178-251`, `src-tauri/src/lib.rs:1577-1582`, `src-tauri/src/web_api.rs:946-984`.
- **Зависимости:** нет.

### WEB-21 — Endpoint speed test и custom endpoints

- **Plane external_id:** `2026-10-25/web-21`
- **Оценка:** M, 1–3 человеко-дня.
- **Объём:** endpoint speed test, custom endpoints CRUD и read live provider settings через service/DB wrappers. В UI явно обозначить, что network test выполняется с сервера и latency не равна latency браузера.
- **Доказательства:** `src/lib/api/vscode.ts:12-70`, `src-tauri/src/commands/provider.rs:870-955`.
- **Зависимости:** нет; connectivity semantics связаны с WEB-04, но scopes не пересекаются.

### WEB-22 — Env conflicts cleanup с backup/restore

- **Plane external_id:** `2026-10-25/web-22`
- **Оценка:** S, 1–2 человеко-дня.
- **Объём:** удаление переменных с backup и restore через два HTTP handlers и тесты. Detection уже поддержан; явно предупреждать, что изменяется OS user environment серверного хоста.
- **Доказательства:** `src/lib/api/env.ts:19-36`, `src-tauri/src/commands/env.rs:8-22`, `src-tauri/src/web_api.rs:1023-1033`.
- **Зависимости:** нет.

### WEB-23 — Global upstream/outbound proxy

- **Plane external_id:** `2026-10-25/web-23`
- **Оценка:** M, 2–3 человеко-дня.
- **Объём:** URL set/get, test, status и scan local proxies через HTTP handlers; инициализация сохранённого HTTP client при headless startup. Не смешивать с local gateway из WEB-01.
- **Доказательства:** `src/lib/api/globalProxy.ts:36-85`, `src-tauri/src/commands/global_proxy.rs:15-87,89-227`, `src-tauri/src/lib.rs:1320-1348`.
- **Зависимости:** WEB-32.

### WEB-24 — Rectifier/optimizer и log config

- **Plane external_id:** `2026-10-25/web-24`
- **Оценка:** M, 2–3 человеко-дня.
- **Объём:** rectifier/optimizer и log config через DB/settings wrappers. Для логов реализовать view/download вместо reveal в OS file manager; сохранить server-host semantics путей.
- **Доказательства:** `src/lib/api/settings.ts:328-349`, `src-tauri/src/commands/settings.rs:731-813`, `src/components/settings/RectifierConfigPanel.tsx:35-65`, `src/components/settings/LogConfigPanel.tsx:25-47`.
- **Зависимости:** нет.

### WEB-25 — Install/update/uninstall CLI на Apps page

- **Plane external_id:** `2026-10-25/web-25`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** install/update/uninstall CLI, uninstall preview и installation diagnostics. Переиспользовать команды, которые уже ждут процесс без visible terminal; добавить wrappers с timeout/cancellation и явной обработкой ошибок. Операции выполняются на сервере; версии CLI уже поддержаны.
- **Доказательства:** `src/lib/api/settings.ts:285-325`, `src-tauri/src/commands/misc.rs:315-420,4729-4790`, `src/components/apps/useToolManagement.ts:517-605,670-677`.
- **Зависимости:** нет.

### WEB-26 — Смена data directory и restart

- **Plane external_id:** `2026-10-25/web-26`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** изменение CC Switch data directory и restart. Заменить Tauri Store/AppHandle серверной конфигурацией (env/config), определить атомарность смены DB и restart через systemd/supervisor. Текстовый ввод server path возможен без native picker.
- **Доказательства:** `src/lib/api/settings.ts:72-130`, `src-tauri/src/commands/settings.rs:253-274,375-390`, `src/components/settings/SettingsPage.tsx:274-310,408-448`.
- **Зависимости:** согласовать lifecycle с WEB-32.

### WEB-27 — Deep-link config import через paste/upload

- **Plane external_id:** `2026-10-25/web-27`
- **Оценка:** M, 2–4 человеко-дня.
- **Объём:** parse/merge/icon fetch/unified import через backend endpoints; browser flow через paste-URL или upload. Регистрация `ccswitch://` в OS и pending queue не переносятся; сейчас peek/ack молча возвращают `null`.
- **Доказательства:** `src/lib/api/deeplink.ts:100-152`, `src-tauri/src/lib.rs:1680-1688`, `src/lib/api/adapter.ts:323-333`.
- **Зависимости:** нет.

### WEB-28 — Проверка и установка обновлений

- **Plane external_id:** `2026-10-25/web-28`
- **Оценка:** M, 2–4 человеко-дня для проверки и инструкции; L, 5–10 дней для safe self-update.
- **Объём:** проверка обновлений через release manifest и понятная инструкция; safe self-update требует отдельного update/restart механизма для systemd/Docker, потому что Tauri updater не подходит. `check_for_updates` сейчас silent no-op, install unsupported. До реализации controls скрываются через WEB-00.
- **Доказательства:** `src/lib/api/settings.ts:72-89`, `src/contexts/UpdateContext.tsx:62-97,119-127`, `src-tauri/src/commands/settings.rs:277-373`, `src/lib/api/adapter.ts:323-333`.
- **Зависимости:** WEB-00; restart lifecycle — WEB-26 и WEB-32.

### WEB-29 — Realtime events через SSE/WebSocket

- **Plane external_id:** `2026-10-25/web-29`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** provider switched, usage/proxy updates, sync status, config/deeplink events. Добавить SSE/WebSocket, reconnect и query invalidation. Сейчас `listen`/`once` возвращают no-op unlisten; уже работающие явные refetch после mutations не ломать.
- **Доказательства:** `src/lib/web-shims/event.ts:1-20`, `src/lib/api/providers.ts:194-200`, `src/hooks/useUsageEventBridge.ts:15-31`, `src/main.tsx:82-90`.
- **Зависимости:** нет; потребители событий перечислены в WEB-01, WEB-04, WEB-06, WEB-11 и WEB-13.

### WEB-30 — Открытие внешних HTTP(S) ссылок

- **Plane external_id:** `2026-10-25/web-30`
- **Оценка:** S, 0,5–1 человеко-день.
- **Объём:** browser `window.open`/`location.assign` по user gesture с `noopener,noreferrer`; backend не нужен. Сохранить существующую проверку HTTP(S) scheme. Это также разблокирует ссылки OAuth и Hermes Web UI.
- **Доказательства:** `src/lib/api/settings.ts:243-254`, `src-tauri/src/commands/misc.rs:50-58`, `src/components/settings/AboutSection.tsx:274-303`.
- **Зависимости:** нет; потребители — WEB-02 и WEB-16.

### WEB-31 — Import prompt из файла

- **Plane external_id:** `2026-10-25/web-31`
- **Оценка:** S, 1–2 человеко-дня.
- **Объём:** `<input type=file>` + upload/text parse вместо native dialog; ошибки чтения и размера/типа файла показать в UI. Остальные базовые prompt operations уже поддержаны.
- **Доказательства:** `src/lib/api/prompts.ts:51-53`, `src-tauri/src/commands/prompt.rs:56-63`.
- **Зависимости:** нет.

### WEB-32 — Общий headless startup/bootstrap и migrations

- **Plane external_id:** `2026-10-25/web-32`
- **Оценка:** M, 3–5 человеко-дней.
- **Объём:** legacy config.json→DB, default skills seed/migration и общая инициализация services. Вынести общий bootstrap из Tauri setup, не копируя его целиком; покрыть first-run/upgrade paths. Сейчас web startup выполняет только `Database::init` + `AppState::new`.
- **Доказательства:** `src-tauri/src/lib.rs:597-738,744-794`, `src-tauri/examples/server.rs:64-66`.
- **Зависимости:** базовая инфраструктурная задача для WEB-01, WEB-05, WEB-11 и WEB-23; lifecycle также используется в WEB-26 и WEB-28.
