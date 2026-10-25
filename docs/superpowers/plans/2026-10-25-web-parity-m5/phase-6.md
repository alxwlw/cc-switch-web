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
- [ ] Прогнать codebase-scout: для каждой команды вне `mapCommand` и каждого `plugin:*`-вызова найти UI-вызывающих (компонент/кнопка, file:line) и классифицировать: (a) уже мёртвая точка (фаза 1–5 закрыла), (b) надо hide, (c) надо disable+tooltip, (d) noop безобиден (tray/deeplink). Таблица — в Notes этой фазы.
- [ ] Зафиксировать список controls, которые кидают `WebUnsupportedError` при клике в web-сборке (проверить по коду, не запуская desktop).

### Task 2: честный update-поток
- [ ] `updater.ts`: в web (`isWeb`) `checkForUpdate()` возвращает явный статус `unsupported` (НЕ "up-to-date") без вызова плагина.
- [ ] `UpdateContext.tsx`: авто-чек на маунте пропускается в web; состояние «обновления недоступны в web-режиме (сервер обновляется оператором)» вместо «up-to-date».
- [ ] `AboutSection.tsx`: кнопка/секция check-update в web скрыта или disabled с этим текстом; `settingsApi.checkUpdates` в web не вызывается.
- [ ] Тесты: `tests/contexts/UpdateContext.test.tsx` (или существующий рядом) — web-ветка без fake success; typecheck/test:unit зелёные.

### Task 3: gating остальных controls по инвентаризации
- [ ] Для каждого пункта (b)/(c) из Task 1 — hide/disable через `isWeb` (импорт из `@/lib/web-shims/core`), tooltip/подпись «недоступно в web-режиме». Не трогать (d).
- [ ] Точечные тесты на 2–3 самых заметных control (например skills ZIP install, terminal launch, reveal-in-manager — по факту инвентаризации).

### Task 4: server-host semantics
- [ ] В web при старте читать `GET /api/capabilities` и показывать `runtime_boundaries` (About-секция и/или неброский баннер): все файловые операции/CLI/proxy — на серверном хосте.
- [ ] Тест на рендер баннера в web-режиме.

### Task 5: гейты, PR, Plane
- [ ] полный набор гейтов (frontend обязателен; Rust — только если задет).
- [ ] ветка `web-parity/web-00-feature-gating`, PR; DX-58 → In Review + ссылка. Закрыть чекбоксы, Notes (таблица инвентаризации обязательна — понадобится WEB-28).

## Notes for next phase

- (заполнить по итогам; сюда — таблицу инвентаризации desktop-only controls)
