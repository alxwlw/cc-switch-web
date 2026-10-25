# Phase 6 — WEB-00 / DX-58: честный feature gating для неподдерживаемых функций

Цель: пользователь web-версии не видит активных controls, которые заведомо завершатся `WebUnsupportedError` или фиктивным `null`; update check не выглядит успешным; server-host semantics показаны явно. Parity в этой фазе НЕ добавлять — только честность. К этому моменту фазы 1–5 уже убрали часть неподдерживаемых поверхностей (ссылки, prompt import, иконки, env cleanup, OMO) — инвентаризация фиксирует остаток.

Спека: § WEB-00. Оценка 2–3 дн. Приоритет high.

Scope: `src/lib/updater.ts`, `src/contexts/UpdateContext.tsx`, `src/components/settings/AboutSection.tsx`, точечно компоненты из инвентаризации; новый хук/утилита gating-а (не изобретать дубликат `isWeb` — использовать `isWeb` из adapter/web-shims core). Backend: максимум чтение уже существующего `GET /api/capabilities`.

Факты разведки:
- `isWeb` определён в adapter.ts:12-13, реэкспортируется web-shims/core.ts:3, но в компонентах НЕ используется (grep по src/ — 3 попадания, из них одно — локальная переменная в SessionMarkdown.tsx:622, не связанная с web-режимом).
- Update-поток в web нечестный: `@tauri-apps/plugin-updater` НЕ заалиашен (vite.config.web.mts:23-32) → реальный плагин вызывает `plugin:updater|check` → adapter noop для `plugin:*` (adapter.ts:325-334) → `null` → updater.ts:36-38 возвращает `{status:"up-to-date"}`. Авто-чек на маунте: UpdateContext.tsx:119-127; ручной `settingsApi.checkUpdates` → `check_for_updates` тоже noop (adapter.ts:330).
- `GET /api/capabilities` (web_api.rs:147-155) уже возвращает `{mode:"web", version, mvp_apps, runtime_boundaries}` и НЕ используется фронтом.
- app.tsx:260-271 — баннер proxy startup-attach-failures (не update check); рядом checkToolUpdatesOnStartup (App.tsx:250-258).
- Полный список desktop-only команд = всё, что не покрыто `mapCommand` (default null, adapter.ts:305-307) + noop-список + `plugin:*`.

### Task 1: инвентаризация desktop-only controls
- [x] Прогнать codebase-scout: для каждой команды вне `mapCommand` и каждого `plugin:*`-вызова найти UI-вызывающих (компонент/кнопка, file:line) и классифицировать: (a) уже мёртвая точка (фаза 1–5 закрыла), (b) надо hide, (c) надо disable+tooltip, (d) noop безобиден (tray/deeplink). Таблица — в Notes этой фазы.
- [x] Зафиксировать список controls, которые кидают `WebUnsupportedError` при клике в web-сборке (проверить по коду, не запуская desktop). Добавлен класс (e) — фоновые/mount-вызовы и side effects; задачи 3–6 разбиты по таблице.

Правило гейтинга (решение контроллера по итогам Task 1):
- **hide (b)** — экран/секция/вкладка, где не работает ничего (nav `auth`, settings `routing`/`network`/`data`, режимы proxy, профили, universal providers, входы в views openclaw/hermes/workspace, discovery skills).
- **disable + tooltip (c)** — отдельная кнопка внутри рабочего экрана: пользователь видит, что функция есть, но в web недоступна. Tooltip — общий текст `web.unavailable`, через `HoverTip` (`src/components/ui/hover-tip.tsx`).
- **silence (e)** — фоновые query и mount/side-effect вызовы: в web не выполняются (`enabled: !isWeb` / ранний return), чтобы не было error state, error-toast'ов и перезапросов каждые 2–30 с.
- Источник `isWeb` — `@/lib/api/adapter` (как в `EnvWarningBanner.tsx:10`, фаза 4). Новых флагов не заводить.
- Каждый гейт помечен комментарием `// WEB-00: <что> — снимет WEB-NN`, чтобы parity-фазы находили свои гейты grep'ом `WEB-00:`.

### Task 2: честный update-поток + общие примитивы гейтинга
- [x] **Тестовый дефолт**: в vitest (jsdom, нет `__TAURI_INTERNALS__`) реальный `isWeb` из adapter равен `true` — гейты сломают desktop-тесты. Сделать так, чтобы в тестах по умолчанию `isWeb === false` (desktop), а web-тесты включали web явно (как `tests/components/EnvWarningBanner.test.tsx:18-22`). Существующие тесты, включая `EnvWarningBanner` и `tests/lib/adapter.*`, остаются зелёными.
- [x] **i18n**: namespace `web.*` во всех 5 локалях (`src/i18n/locales/{en,ja,tr,zh,zh-TW}.json`): минимум `web.unavailable` («недоступно в web-режиме»), `web.update.unsupported` («обновления недоступны в web-режиме: сервер обновляет оператор»). Задачи 3–7 добавляют ключи туда же.
- [x] `updater.ts`: в web (`isWeb`) `checkForUpdate()` возвращает явный статус `unsupported` (НЕ "up-to-date") без вызова плагина.
- [x] `UpdateContext.tsx`: авто-чек на маунте пропускается в web; состояние «обновления недоступны в web-режиме (сервер обновляется оператором)» вместо «up-to-date»; `hasUpdate` в web — false (dot в Sidebar не горит).
- [x] `AboutSection.tsx`: кнопка check-update в web скрыта или disabled с текстом `web.update.unsupported`; `settingsApi.checkUpdates` в web не вызывается; install/portable-кнопки недостижимы.
- [x] Тесты: `tests/contexts/UpdateContext.test.tsx` (или существующий рядом) — web-ветка без fake success, desktop-ветка не изменилась; AboutSection в web не показывает «up-to-date»; typecheck/test:unit зелёные.

### Task 3: silence — фоновые query и mount-вызовы
Строки таблицы с гейтом T3. Только слой данных: `src/lib/query/*`, `src/hooks/*`, mount-эффекты `App.tsx`, `src/lib/api/adapter.ts`. UI-компоненты не трогать (задачи 4–6).
- [x] Query с командами вне `mapCommand` в web не выполняются (`enabled: … && !isWeb`, интервалы не стартуют): proxy status/takeover/app mode, failover (5 с), auth status (15 с), kimi status (30 с), subscription/copilot quotas, `useUsageQuery`, claude desktop status (5 с), live provider ids (opencode/openclaw/hermes), `get_${app}_current_state`, `get_session_usage_last_sync` (30 с), `scan_unmanaged_skills`, tool probe/list на Apps page, `get_pi_session_discovery`. Потребители получают «нет данных», а не error: проверить, что sidebar mode/auth-dot, ProviderCard и т.п. в web показывают нейтральное состояние (например sidebar — «direct», как фоллбэк `useSidebarStatus.ts:56`).
- [x] Mount-вызовы в web пропускаются: `take_startup_attach_failures` (`App.tsx:266`), checkToolUpdatesOnStartup (`App.tsx:250-258`), если он зовёт неподдерживаемое.
- [x] `adapter.ts`: `update_tray_menu` и `set_window_theme` — в silent noop-список (tray/окно: desktop-only fire-and-forget, сейчас кидают и логируют в console).
- [x] Тесты: для 2–3 query (proxy status, failover queue, auth status) — в web `invoke`/`fetch` не вызывается и query не в error; desktop-ветка не изменилась; adapter: `update_tray_menu`/`set_window_theme` → `null` без `fetch`.

### Task 4: hide/disable — shell и Settings
Строки таблицы с гейтом T4.
- [x] Nav `auth` (Sidebar globals `Sidebar.tsx:268-293`) скрыт в web; прямой переход на view `auth` (если возможен) показывает notice, а не throwing-панель.
- [x] Settings: секции `routing`, `network`, `data` скрыты в web (`SETTINGS_ITEMS`, `Sidebar.tsx:695-702`, и `SettingsPage.tsx:365-527`); если сохранённая/запрошенная секция скрыта — открывается `general`. `restart_app`-prompt недостижим.
- [x] General: тоглы launch-on-startup, skip-Claude-onboarding, Claude plugin integration — disabled + tooltip `web.unavailable`. Side effects в `useSettings.ts` (`setAutoLaunch` :260/:409, onboarding skip :280-286/:426-428, `applyClaudePluginConfig` :160-170, `setAppConfigDirOverride` :399) и `useProviderActions.ts:64` в web не вызываются: полный save в web не падает после успешного `save_settings` (сейчас fake failure). `is_portable_mode` (`useSettingsMetadata.ts:30`) в web не вызывается.
- [x] AppConfig: Browse-кнопки директорий (`pick_directory`) disabled + tooltip, ручной ввод пути остаётся (подсказка: путь на серверном хосте — ключ `web.serverPath` или аналог); блок `CodexAuthSettings` (`AppConfigSection.tsx:182`) скрыт.
- [x] Window controls: `useAppWindowControls` (`App.tsx:214-215`) — false в web; WindowControls и вызовы отсутствующих методов window-shim (`App.tsx:546,592-595,669-670`) в web не исполняются.
- [x] Тесты: Settings-навигация в web без `routing/network/data`; полный save в web не зовёт `setAppConfigDirOverride`/`setAutoLaunch` и не показывает ошибку; Sidebar в web без `auth`; desktop-ветки без изменений.

### Task 5: hide/disable — providers, формы, proxy, profiles, app views
Строки таблицы с гейтом T5.
- [x] ProviderList/ProviderCard: кнопка Test (`stream_check_provider`), пункт «open terminal», import-from-live на пустом списке, usage-script кнопки в `UsageScriptModal` (test/balance/quota), default-model actions opencode/openclaw — disabled + tooltip.
- [x] Формы провайдеров: fetch-models кнопки (Claude/Codex/Gemini/ClaudeDesktop/OMO) и copilot models — disabled + tooltip; `EndpointSpeedTest` скрыт; OAuth login/logout в формах (`useManagedAuth`, `useKimiOAuth`, `AgyOAuthSection`, `KimiAccountLogin`) — disabled + tooltip; `EditProviderDialog.tsx:211,233` не зовёт `get_openclaw_live_provider`/`read_live_provider_settings` в web, если это ломает/шумит открытие формы.
- [x] Proxy-режимы: `SwitchModePanel`, `CodexStaleClientsNotice`, `AutoFailoverConfigPanel`, health badge — скрыты в web (режим всегда direct).
- [x] `ProfileSwitcher`/`ProfileManageDialog` скрыты; вкладка universal providers в `AddProviderDialog` скрыта.
- [x] Входы в views `openclawConfig`, `hermesMemory`, `workspace` и кнопки hermes web UI/dashboard (`App.tsx:944,1881`) скрыты в web; openclaw health banner (`App.tsx:335`) не опрашивает.
- [x] Тесты: 2–3 самых заметных (Test в ProviderList, fetch-models в форме, SwitchModePanel/ProfileSwitcher отсутствуют в web), desktop без изменений.

### Task 6: hide/disable — skills, sessions, usage, prompts, Apps page
Строки таблицы с гейтом T6.
- [x] Skills: сегмент discover/`skillsDiscovery` и SkillsStorageSheet скрыты; ZIP install, check updates/update, import from apps, backups restore/delete — disabled + tooltip. Работающие (installed list, toggle, uninstall, resync) не трогать.
- [x] Sessions: `SessionImage` в web — placeholder «изображение недоступно в web-режиме» без вызова; export markdown — disabled + tooltip; reveal в `PathChip` скрыт (copy path остаётся); «resume in terminal» в web всегда идёт по copy-command фоллбэку (`SessionManagerPage.tsx:309`), без `launch_session_terminal`.
- [x] Usage: rebuild codex usage, pricing edit/delete/batch, pricing model source, models.dev sync, proxy client host в `UsageSettingsSheet`, open config folder — disabled + tooltip или скрыты (по месту). Работающий dashboard не трогать.
- [x] Prompts: «open folder» (`usePromptPageCommon.ts:32`) скрыт; `PiNativePromptResources` скрыт.
- [x] Apps page: install/update/uninstall — disabled + tooltip; версии (`get_tool_versions`) показываются.
- [x] Тесты: skills ZIP install disabled в web, sessions reveal/terminal в web, Apps actions disabled; desktop без изменений.

### Task 7: server-host semantics
- [ ] В web при старте читать `GET /api/capabilities` и показывать `runtime_boundaries` (About-секция и/или неброский баннер): все файловые операции/CLI/proxy — на серверном хосте.
- [ ] Тест на рендер баннера в web-режиме.

### Task 8: гейты, PR, Plane
- [ ] полный набор гейтов (frontend обязателен; Rust — только если задет).
- [ ] ветка `web-parity/web-00-feature-gating`, PR; DX-58 → In Review + ссылка. Закрыть чекбоксы, Notes (таблица инвентаризации обязательна — понадобится WEB-28).

## Notes for next phase

### Инвентаризация desktop-only controls (Task 1, `main@f675b330`)

Источник — codebase-scout по коду (без запуска). `mapCommand` обрабатывает 67 команд; silent-null noop: `tray_app_page_seen`, `take_tray_navigation`, `peek_pending_deeplink`, `ack_pending_deeplink`, `check_for_updates`, `plugin:*`; `open_external` — в браузере (фаза 1). Из 274 команд, которые вызывает `src/`, ~268 кидают `WebUnsupportedError`. До фазы 6 гейтов в UI нет: единственный `isWeb` — подсказка в `EnvWarningBanner.tsx:263` (фаза 4). `isMac/isWindows/isLinux` (`src/lib/platform.ts`) читают UA браузера, это не web-гейт.

Классы: **a** — мёртвое (нет UI-вызывающих или закрыто фазами 1–5); **b** — hide; **c** — disable + tooltip «недоступно в web-режиме»; **d** — noop/фоллбэк безобиден; **e** — фоновый/mount-вызов или side effect: в web не делать (query `enabled: !isWeb`, пропуск вызова). Колонка «гейт» — задача этой фазы, которая ставит гейт; «parity» — фаза/задача, которая его снимет.

| область | команды | UI (file:line) | сейчас в web | класс | гейт | parity |
|---|---|---|---|---|---|---|
| update | `check_for_updates`, `plugin:updater\|check` | `updater.ts:31,36`; авто-чек `UpdateContext.tsx:119-125`; кнопка `AboutSection.tsx:241-253` (handler `:133-170`); dot `Sidebar.tsx:222,711` | fake success «up-to-date» | c | T2 | WEB-28 (ф.36–37) |
| update | `install_update_and_restart`, `open_portable_update_download`, `check_app_update_available` | `AboutSection.tsx:138,150`; `DatabaseUpgrade.tsx:65,102` | недостижимо (`hasUpdate` всегда false; `get_init_error` кидает) | a | T2 | WEB-28 |
| settings | `is_portable_mode` | `useSettingsMetadata.ts:30` | mount, throws, пойман | e | T4 | WEB-28 |
| settings | `restart_app` | `SettingsPage.tsx:293-309` | throws → toast; достижимо только после смены data dir | b | T4 | WEB-26 (ф.35) |
| settings › routing | proxy/takeover, `get/set_rectifier_config`, `get/set_optimizer_config` | `RoutingSection` (`SettingsPage.tsx:392`), `RectifierConfigPanel.tsx:30-56` | throws | b (вся секция) | T4 | WEB-01 (ф.27–28), WEB-24 (ф.12) |
| settings › network | `get/set_global_proxy_url`, `test_proxy_url`, `get_upstream_proxy_status`, `scan_local_proxies` | секция `network` (`SettingsPage.tsx:394-407`, `GlobalProxySettings`) | throws | b (вся секция) | T4 | WEB-23 (ф.24) |
| settings › data | `get/set_app_config_dir_override`, `pick_directory`, import/export + `open/save_file_dialog`, `*_db_backup`, `*_backup_location`, `webdav_*`, `s3_*`, `reveal_app_log_file`, `get/set_log_config` | секция `data` (`SettingsPage.tsx:408-522`) | throws на каждом блоке | b (вся секция) | T4 | WEB-26, WEB-09/10/11, WEB-24 |
| settings › general | `set_auto_launch`, `apply/clear_claude_onboarding_skip`, `apply_claude_plugin_config` | тоглы GeneralSection; side effects `useSettings.ts:160-170,260,280-286,409,426-428`; `useProviderActions.ts:64` | throws → toast | c (тоглы) + e (side effects) | T4 | WEB-20 (ф.14) |
| settings › save | `set_app_config_dir_override` в полном save | `useSettings.ts:399` (после `save_settings`) | **fake failure**: настройки сохранены, а save падает | e | T4 | WEB-26 |
| settings › appConfig | `pick_directory` (Browse), Codex compression/unify history (5 команд) | `useDirectorySettings.ts:211,392,411`; `CodexAuthSettings.tsx:40-112` (`AppConfigSection.tsx:182`) | throws | c (Browse; ввод пути работает — путь на сервере) + b (CodexAuthSettings) | T4 | WEB-20 |
| window | `getCurrentWindow` shim без `isMaximized/onResized/isFullscreen/…` | `WindowControls.tsx:25,29,80`; `App.tsx:214-215,546,592-595,669-670` | кнопки min/max/close молча ничего; TypeError в console | b (`useAppWindowControls` false в web) | T4 | — |
| window | `set_window_theme` | `theme-provider.tsx:125` | throws, пойман `console.debug` | d → noop в adapter | T3 | — |
| tray | `update_tray_menu` (после мутаций, ~12 мест) | `mutations.ts:132,287,381`, `useSettings.ts:320,464`, … | throws, проглочен `console.error` | d → noop в adapter | T3 | — |
| tray | `tray_panel_*` | отдельная страница `tray-panel.html` | никто не открывает | d | — | — |
| deeplink | `peek/ack_pending_deeplink` (noop), `merge_deeplink_config`, `fetch_deeplink_icon`, `import_from_deeplink_unified` | `DeepLinkImportDialog.tsx:59-264` | диалог не открывается | d | — | WEB-27 (ф.20) |
| boot | `get_init_error` | `main.tsx:95` | throws, пойман | d | — | WEB-32 (ф.7) |
| proxy | `get_proxy_status` (2 с при running), `get_proxy_takeover_status`, `get_app_mode`, `take_startup_attach_failures` | `query/proxy.ts:31,45,63`; `useSidebarStatus.ts:31-56`; `App.tsx:266,313` | mount, error state | e | T3 | WEB-01 |
| proxy | start/stop/takeover/route/stack (≈12 команд), `codex_forces_multi_agent_v2` | `SwitchModePanel.tsx:122-365`, `useProxyStatus.ts`, `useModeActions.ts`, `CodexStaleClientsNotice.tsx`, `StackSubagentFields.tsx:90` | throws | b (SwitchModePanel/режимы) | T5 | WEB-01 |
| failover | 11 команд (health, circuit breaker, queue, auto-failover) | `query/failover.ts:23,94` (5 с), `SwitchModePanel.tsx:26-30`, `AutoFailoverConfigPanel.tsx`, health badge `ProviderCard.tsx:223` | throws, перезапрос каждые 5 с | e + b | T3 (queries) / T5 (UI) | WEB-01 |
| provider › test | `stream_check_provider` | кнопка Test, `ProviderList.tsx:126,209` | throws | c | T5 | WEB-04 (ф.31) |
| provider › terminal | `open_provider_terminal`, `pick_directory`, `agy_account_is_running` | `OpenTerminalDialog.tsx:131-178` (меню карточки, `App.tsx:1835`) | throws | c (пункт меню) | T5 | — (desktop-only) |
| provider › forms | `fetch_models_for_config`, `get_agy/opencode/codex_oauth/xai_oauth_models`, `copilot_get_models*` | `ClaudeFormFields.tsx:337-459`, `CodexFormFields.tsx:822-906`, `GeminiFormFields.tsx:96`, `ClaudeDesktopProviderForm.tsx:578-654`, `useOmoModelSource.ts:59` | throws | c | T5 | WEB-03 (ф.29) |
| provider › forms | `test_api_endpoints`, `get/add/remove_custom_endpoint` | `EndpointSpeedTest.tsx:132-430` | throws | b | T5 | WEB-21 (ф.11) |
| provider › usage | `testUsageScript`, `queryProviderUsage`, `get_balance`, `get_coding_plan_quota` | `UsageScriptModal.tsx:603-712`; `useUsageQuery` (`queries.ts:262-268`, interval) из `ProviderCard.tsx:286-290` | throws | c (кнопки модалки) + e (query) | T5 / T3 | WEB-04 |
| provider › quotas | `kimi_oauth_quota`, `get_subscription_quota`, `get_codex_oauth_quota`, `get_xai_oauth_quota`, `agy_account_quota`, `copilot_get_usage*` | `query/subscription.ts:93-262`, `query/copilot.ts:31-32`, `SubscriptionQuotaFooter.tsx:354`, `AccountQuota.tsx:35-279` | throws, auto-refresh | e | T3 | WEB-02/04 |
| provider › live | `import_{opencode,openclaw,hermes,claude_desktop}_providers_from_live` | `ProviderList.tsx:214-223` (пустой список) | throws | c | T5 | WEB-14/15/16/19 |
| provider › live | `get_{opencode,openclaw,hermes}_live_provider_ids`, `get/set_opencode_default_model`, `get/set_openclaw_default_model`, `get/set_openclaw_model_catalog`, `get_openclaw_live_provider`, `read_live_provider_settings` | `App.tsx:1125-1135`, `ProviderList.tsx:135-140`, `ProviderForm.tsx:1105`, `useOpenClaw.ts:35-47`, `useHermes.ts:55`, `useProviderActions.ts:102-115,370,425,436`, `EditProviderDialog.tsx:211,233` | mount/edit throws | e (+ c для default-model actions) | T3 / T5 | WEB-14/15/16 |
| provider › claude desktop | `get_claude_desktop_status` (5 с), `get_claude_desktop_default_routes`, `ensure_claude_desktop_official_provider`, `ensure_grokbuild_official_provider` | `useDesktopConfigMessages.ts:14-15`, `AppsPage.tsx:182`, `ClaudeDesktopProviderForm.tsx:322`, `mutations.ts:48,58` | throws, интервал | e | T3 | WEB-19 (ф.32) |
| provider › universal | `get_universal_providers`, upsert/delete/sync | `UniversalProviderPanel.tsx:36-180` (вкладка `AddProviderDialog.tsx:205,220,599`) | throws | b (вкладка) | T5 | WEB-07 (ф.9) |
| profiles | `list/create/update/delete/apply_profile`, `clear_current_profile` | `ProfileSwitcher.tsx:37-40`, `ProfileManageDialog.tsx:17-19` | throws | b | T5 | WEB-06 (ф.10) |
| auth | `auth_get_status` (15 с для xai), `auth_*` login/logout/accounts, `agy_account_is_running` | nav `auth` (`Sidebar.tsx` globals, `App.tsx:1639-1666`), sidebar dot `useSidebarStatus.ts:46`, `ProviderCard.tsx:206`, `useManagedAuth.ts:53-319`, `AgyOAuthSection.tsx:193` | throws | b (nav `auth`) + e (status queries) + c (login в формах) | T4 / T3 / T5 | WEB-02 (ф.25–26) |
| auth › kimi | `kimi_oauth_*` (status 30 с, start/poll/cancel/remove/use) | `KimiAccountQuotaFooter.tsx:45`, `useKimiOAuth.ts:21-188`, `KimiAccountLogin.tsx:25` | throws | e + c | T3 / T5 | WEB-02 |
| app views | openclaw env/tools/agents/health, hermes config/memory/web UI/dashboard, workspace (7 команд), `get_${app}_current_state`, `update_pi_provider_usage_script` | views `openclawConfig`/`hermesMemory`/`workspace` (`App.tsx:1567-1601`), `App.tsx:330,335,944,1881`, `useProviderActions.ts:311` | mount throws | b (входы в views) + e (mount) | T5 / T3 | WEB-15/16/17 |
| skills | `scan_unmanaged_skills` (mount), discovery/repos/install/`search_skills_sh`, `check_skill_updates`/`update_skill`, `import_skills_from_apps`, ZIP install, backups, storage sheet | `UnifiedSkillsPanel.tsx:177-196,705,1752,2084`, `SkillsPage.tsx:67-205`, `SkillsStorageSheet.tsx:96,136`, `useSkills.ts:267,358,474` | throws | e (scan) + b (discover, storage) + c (остальные кнопки) | T3 / T6 | WEB-05 (ф.23) |
| sessions | `stream_session_messages` | `query/sessions.ts:139-154` | фоллбэк на `get_session_messages` до первого chunk — **работает** | d | — | WEB-13 (ф.21) |
| sessions | `get_session_image`, `export_session_markdown`, `reveal_session_path`, `launch_session_terminal`, `get_pi_session_discovery` | `SessionImage.tsx:62`, `SessionReader.tsx:428`, `PathChip.tsx:56,108`, `SessionManagerPage.tsx:191,309,349` | throws (terminal → clipboard-фоллбэк только при `!isMac()`) | c (image placeholder, export) + b (reveal) + c (terminal: в web — всегда copy) + e (discovery) | T6 / T3 | WEB-13, WEB-17 |
| usage | `get_session_usage_last_sync` (30 с), `rebuild_codex_usage`, pricing edit (4 команды), `get/set_pricing_model_source`, models.dev sync (3), `get/set_proxy_client_host`, `open_app_config_folder` | `query/usage.ts:397,423-458`, `UsageDashboard.tsx:49,390`, `PricingConfigPanel.tsx:84-314`, `PricingEditModal.tsx:30`, `ModelsDevAutoSyncPanel.tsx:196-526`, `modelsDevAutoSync.ts:30-76`, `UsageSettingsSheet.tsx:121` | throws | e + c | T3 / T6 | WEB-12 (ф.13) |
| prompts | `open_config_folder`, Pi prompts (6 команд) | `usePromptPageCommon.ts:32`, `PiNativePromptResources.tsx:96-794` | throws | b | T6 | WEB-17 |
| apps page | `probe_tool_installations`, `list_tool_installations` (mount), `run_tool_lifecycle_action`, `prepare_tool_uninstall` | `useToolManagement.ts:517-924`, `ToolUninstallConfirmDialog.tsx:53` (`get_tool_versions` работает) | throws | e (mount) + c (install/update/uninstall) | T3 / T6 | WEB-25 (ф.22) |
| clipboard | `copy_text_to_clipboard` | `clipboard.ts:5` (14 файлов) | throws → фоллбэк `navigator.clipboard` (нужен secure context) | d | — | — |
| plugins | `plugin:log`, `plugin:dialog\|message`, `plugin:process\|exit` | `frontendLogger.ts`, `main.tsx:18-19,60-78` | silent noop, путь config-error недостижим | d | — | — |
| мёртвое | `auth_list_accounts`, `copilot_*` (11, UI идёт через `auth_*`), `parse_deeplink`, `*_claude_mcp_*`/`*_mcp_server_in_config` (6), `ensure_codex_official_provider`, `get_universal_provider`, `get_claude_code_config_path`, `get_auto_launch_status`, legacy skills (6), `get_usage_data_sources`, `update_endpoint_last_used`, дубли в `vscode.ts:74-92` | нет UI-вызывающих | — | a | — | — |
| закрыто фазами 1–5 | `open_external` (ф.1), `import_prompt_from_file` (ф.2), custom icons (ф.3), env conflicts (ф.4), OMO/OMO Slim (ф.5) | — | работает | a | — | — |

Не проверено кодом (проверить при гейтинге): точный catch/toast в proxy/failover/skills/usage/workspace/hermes/openclaw; дефолтный `retry` query client; фильтрует ли что-то nav views.

### Примитивы гейтинга (Task 2, `7d5322c3`)

- Тесты по умолчанию desktop: глобальный `vi.mock("@/lib/api/adapter")` в `tests/setupTests.ts` подменяет только `isWeb` (getter на `globalThis.__TEST_IS_WEB__`), остальной adapter настоящий. Web-ветка — `setWeb(true)` из `tests/helpers/web.ts`, сброс после каждого теста. Файл со своим `vi.mock("@/lib/api/adapter", () => ({…}))` обязан включать ключ `isWeb`, иначе импорт `isWeb` падает.
- `isWeb` читать в момент использования (в хуке/`enabled`/эффекте), не кэшировать в module-level константе — иначе `setWeb` в тестах не действует.
- i18n: `web.unavailable`, `web.update.unsupported` во всех 5 локалях (переведены, не копия en).
- Update: `checkForUpdate()` в web → `{status:"unsupported"}` без плагина; `UpdateContext` пропускает авто-чек, `hasUpdate` false; About вместо кнопки показывает текст `web.update.unsupported`.

### Silence (Task 3, `2a42f427..6f9f6e06`)

- `enabled: … && !isWeb` на query: proxy (`query/proxy.ts`: status, takeover, app mode, direct provider, stack, global/app proxy config, client host), failover/circuit breaker (`query/failover.ts`, все 6), quotas (`query/subscription.ts` 5, `query/copilot.ts:58`), `useUsageQuery` (`queries.ts:268`), `useSessionUsageLastSync`/`useProviderLimits` (`query/usage.ts`), `usePiCurrentState`, `useProfilesQuery`, skills (`useSkills.ts`: discoverable, failures, scan unmanaged, repos, skills.sh search, ccSwitch dir), openclaw (`useOpenClaw.ts`: live ids, default model, env, tools, agents defaults, health), hermes (`useHermes.ts`: live ids, model config, memory, limits), sidebar mode/auth (`useSidebarStatus.ts`), `useKimiOAuth` (30 с), `useManagedAuth` (xai 15 с), `useDesktopConfigMessages` (5 с). Call-site `enabled` в `ProviderCard`, `KimiAccountQuotaFooter`, `AppsPage`, `ProviderList`, `SessionManagerPage`, `AgyOAuthSection`, `ClaudeDesktopProviderForm`, `ProviderForm`.
- Mount: `take_startup_attach_failures` (`App.tsx`) и `loadInstallations` (`useToolManagement.ts`) — ранний return в web. `checkToolUpdatesOnStartup` зовёт `get_tool_versions` (mapped) — работает, не гейтится.
- Adapter: `update_tray_menu`, `set_window_theme` — silent noop.
- **Грабли**: disabled react-query query навсегда `isPending` → вечный skeleton. Поэтому `isLiveMembershipPending` (`ProviderList`) и `isInitialStatusPending` (`useProxyStatus`) в web false; в `AppsPage` pending-ветка `rowInstalled` даёт null → плоский список без групп (строка Claude Desktop не попадает в «not installed»).
- **Ручной `refetch()` disabled query всё равно выполняет** — кнопки refresh/check-updates надо disable в UI (T5/T6).
- Долги вперёд: `handleDuplicate` (`App.tsx` ~:1115) для opencode/openclaw/hermes/pi/omp в web падает на live ids → toast `provider.duplicateLiveIdsLoadFailed` (T5); в `ProviderList` для live-membership apps `isInConfig` false в web — карточки в «available» (T5); `piStateUnavailable` true для pi/omp в web — честно (карточки заблокированы с подсказкой); не тронуты, за T4/T6: `useBackupManager`, `useBackupLocations`, `useGlobalProxyUrl`/`useUpstreamProxyStatus`, `is_portable_mode`, `OpenTerminalDialog`, `ModelsDevAutoSyncPanel`/`ModelsDevPickerDialog`, `PiNativePromptResources`. `EditProviderDialog.tsx:211,233` уже ловят ошибку и откатываются на сохранённый конфиг — гейт не нужен.

### Shell и Settings (Task 4, `2c2b94a8..4074dd24`)

- Доступность views/секций — одна точка: `src/lib/navigation.ts` (`isViewAvailable`, `isSettingsSectionAvailable`, `resolveSettingsSection`, `readStoredView`). В web скрыты nav `auth` (WEB-02) и секции `routing` (WEB-01/24), `network` (WEB-23), `data` (WEB-26/09/10/11). Запрошенная скрытая секция резолвится в `general` там, где рендерится (`Sidebar` highlight, `SettingsPage` — заголовок/иконка от резолвнутого значения); `App.tsx` хранит сырое состояние. Сохранённый view `auth` в web не восстанавливается. Снятие гейта секции — правка `navigation.ts`.
- `AuthSettingsPanel.tsx` в web показывает notice `web.unavailable` вместо `AuthCenterPanel` (ссылки «manage accounts» из форм провайдеров).
- Disabled + tooltip — `src/components/ui/web-unavailable.tsx` (`WebUnavailableTip`: на desktop отдаёт children как есть; в web — `span` с `[&>*]:pointer-events-none`, чтобы tooltip работал и над disabled `Switch`; в jsdom не проверяется) и проп `unavailableInWeb` у `SettingsSwitchRow` (`SettingsLayout.tsx`). Задачи 5–6 переиспользуют.
- General в web: disabled launch-on-startup, silent startup (WEB-20), minimize-to-tray, tray style, preferred terminal (`TerminalSelect` получил `disabled`), Linux window-controls switch — настройки desktop-оболочки, в браузере инертны. AppConfig: Claude plugin toggle и onboarding skip (WEB-20) disabled, `CodexAuthSettings` скрыт (WEB-20), Browse (`pick_directory`) disabled, ручной ввод пути + подсказка `web.serverPath` (WEB-26).
- `useSettings.ts` в web пропускает: `applyClaudePluginConfig`, `setAutoLaunch`, onboarding apply/clear, `setAppConfigDirOverride`, оба `sync_current_providers_live` (не в `mapCommand`, WEB-09), appDirChanged/restart prompt. `save_settings` выполняется — полный save больше не падает. Также `is_portable_mode` (`useSettingsMetadata.ts`, WEB-28), `get_app_config_dir_override` в `Promise.all` загрузки директорий (`useDirectorySettings.ts`, WEB-26 — без этого в web пропадали resolved dirs), `apply_claude_plugin_config` при switch (`useProviderActions.ts`).
- Window: `useAppWindowControls` false в web, `setDecorations` пропущен (окна нет — гейт постоянный).
- Долги вперёд: `onOpenRoutingSettings` из `UsageSettingsSheet.tsx:258` (T6), `SwitchModePanel`, `RouteSettingsSheet` (T5) в web молча открывают General — скрыть ссылки. Легаси `DirectorySettings` не используется, не гейтился.

### Providers, формы, proxy (Task 5, `64e92df6..7d69a62b`)

- Views `workspace`/`openclawConfig`/`hermesMemory` скрыты через `navigation.ts:isViewAvailable` (сохранённый view → providers; WEB-15/16/17); в `App.tsx` скрыты `ProfileSwitcher` (WEB-06), кнопка Hermes Web UI/dashboard (WEB-16), sub-view табы openclaw/hermes (`renderAppSegments` → null).
- `SwitchModePanel` — единственный рендерер списка для claude/codex/gemini/grokbuild/claude-desktop, поэтому список остаётся (direct, `directId = currentProviderId`), а скрыты ModeTabs, status badge, `ModeDialog`, `RouteSettingsSheet`, `StackSettingsSheet` (WEB-01). `CodexStaleClientsNotice`, `AutoFailoverConfigPanel`, health badge достижимы только оттуда — не рендерятся. Провайдер, которому нужен routing, — switch disabled с `web.needsRouting`.
- Карточки (`presentation.ts`, `ProviderCardActions.tsx`): Connectivity check (WEB-04), Open terminal (desktop-only, гейт постоянный), set-default opencode/openclaw (WEB-14/15), configure usage для pi/omp (`update_pi_provider_usage_script` не замаплен; проп `configureUsageUnavailable`, WEB-17) — disabled с причиной. `UsageScriptModal` Test (WEB-04).
- **Live membership в web неизвестна** (opencode/openclaw/hermes): одна секция `web.liveUnknownTitle` вместо «в конфиге / доступно». Add/Remove замаплены и работают; Remove для openclaw/hermes disabled с `web.removeUnverifiable` (backend `remove_openclaw_provider_from_live` не проверяет default, а current/default в web неизвестны), opencode — Remove активен. Hermes enable в этой секции не предлагается. WEB-14/15/16 должны вернуть обычные секции.
- `handleDuplicate` (`App.tsx`) в web пропускает live-ids: шаг лишь проверял, что ключ копии не совпадает с id из live-конфига (копия сохраняется с `addToLive=false`). Потеря: копия `<id>-copy` может совпасть с id, который есть только в live — узкий случай, допустимо.
- Import-from-live (`ProviderEmptyState.importUnavailable`) disabled для opencode/openclaw/hermes/claude-desktop (WEB-14/15/16/19); `import_default_config` для остальных работает.
- Формы: fetch-models disabled + tip во всех формах и `ModelInputWithFetch`/`FetchedModelPicker` (WEB-03; picker открывается, если список уже есть); `EndpointField` «manage and speed test» скрыт (WEB-21); Codex/Copilot/Xai OAuth select-режим — notice `web.accountLogin` вместо вечного spinner'а (WEB-02); ClaudeDesktop: режим model-mapping (proxy) скрыт для новых провайдеров, оставлен для уже proxy (WEB-01/19); official-пресеты claude-desktop/grokbuild скрыты в web — seed `ensure_*_official_provider` создаёт провайдера с фиксированным id, обычный add это не воспроизводит (WEB-19). Формы стартуют на `custom`, id пресетов claude-desktop назначаются до фильтра.
- `AddProviderDialog`: вкладка universal providers скрыта (WEB-07). `useOmoModelSource`: без live-ids/runtime-models и ложных warning-toast'ов (WEB-14). `useProviderActions.ts:100`: регистрация openclaw model catalog/default после add пропущена (WEB-15). General: тогл show-profile-switcher disabled (WEB-06).
- i18n: `web.accountLogin`, `web.needsRouting`, `web.liveUnknownTitle`, `web.removeUnverifiable`.

### Skills, sessions, usage, prompts, Apps (Task 6, `53c0b089..8e85a1ce`)

- Skills (WEB-05): view `skillsDiscovery` скрыт (`navigation.ts`); в `UnifiedSkillsPanel` скрыты discover-сегмент, пункт add-меню, empty-state shortcut и «search in discover» на no-match; `initialView:"discover"` в web → installed, discover-страница не монтируется. Disabled: ZIP install, import from apps, check updates (меню + toolbar), restore backups, repos. `SkillsStorageSheet` оставлен — единственный вход к работающему `resync_skills_to_apps`; внутри disabled location radios и open folder. Per-skill update и repo-fail «Retry» недостижимы (данные только от отключённого check).
- Sessions (WEB-13): `terminalName` null в web → только copy-command фоллбэк, `launch_session_terminal` не вызывается, сообщение `web.sessionResumeCopied`; export markdown disabled (меню и toolbar `SessionReaderHeader`); reveal в `PathChip` скрыт, copy остаётся; `SessionImage` — placeholder `web.sessionImage` без `get_session_image`.
- Usage (WEB-12): `UsageSettingsSheet` — скрыты logging-status row и routing link (WEB-01; proxy client host живёт в `RoutingSection`, скрытой T4), rebuild Codex disabled; `PricingConfigPanel` — скрыты `ModelsDevAutoSyncPanel` и pricing-model-source, disabled models.dev import/add/edit/delete (чтение цен работает); startup sync `modelsDevAutoSync.ts` пропущен (`main.tsx:153` обрабатывает `skipped`).
- Prompts (WEB-17): open folder скрыт (`usePromptPageCommon.ts`); в `PiPromptPanel` скрыты табы system/templates. `PiNativePromptResources` ни откуда не импортируется.
- Apps (WEB-25): update-all, diagnose-all, install/update строки, uninstall disabled; версии и check-updates (`get_tool_versions`) работают; ручные install/uninstall-диалоги только копируют команду; `ToolUninstallConfirmDialog` недостижим.
- i18n: `web.sessionImage`, `web.sessionResumeCopied`.
- **Flaky вне диффа**: `tests/components/SessionManagerPage.test.tsx` «switches to all apps and to time buckets» падает около полуночи по локальному времени (фикстуры от `Date.now()`, заголовок «今天»); с `TZ=UTC` зелёный. Baseline не трогали.
