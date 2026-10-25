# Phase 1 — WEB-30 / DX-88: открытие внешних HTTP(S) ссылок

Цель: в web-сборке `invoke("open_external", { url })` открывает ссылку в браузере (`window.open` c `noopener,noreferrer`), а не бросает `WebUnsupportedError`. Backend не нужен. Разблокирует ссылки OAuth (WEB-02) и Hermes Web UI (WEB-16).

Спека: `docs/research/web-feature-gaps-2026-10-25-plane-backlog.md` § WEB-30. Оценка S (0,5–1 дн.).

Scope: `src/lib/api/adapter.ts` (invoke, ~322-362), новый тест `tests/lib/adapter.openExternal.test.ts`. Backend НЕ трогаем.

Факты разведки:
- `open_external` отсутствует в `mapCommand` (adapter.ts:44-308, default → null → `WebUnsupportedError`, adapter.ts:335-336).
- Все вызовы идут через `invoke("open_external")`: `settingsApi.openExternal` (settings.ts:243-254, уже валидирует http/https), прямой invoke в `DatabaseUpgrade.tsx:269` и `SessionMarkdown.tsx:583-586` (`openSessionLink`), ~15 компонентов через settingsApi.
- Desktop-валидация `validate_external_url` (misc.rs:34-48) допускает mailto; фронтовый guard — только http/https. Держим http/https (задача называется «HTTP(S) ссылки»).
- `window.open` в репозитории сейчас не используется нигде.
- adapter.invoke исполняется только в web-сборке (vite.config.web.mts:23-32 алиасит `@tauri-apps/api/core` → web-shims/core.ts → adapter).

### Task 1: тест adapter на open_external (красный)
- [x] `tests/lib/adapter.openExternal.test.ts`: mock `window.open`; `invoke("open_external", { url: "https://example.com" })` → `window.open("https://example.com", "_blank", "noopener,noreferrer")`, resolve `true`, `fetch` не вызван.
- [x] негативные кейсы: `ftp://…`, `javascript:alert(1)`, невалидный URL → reject с `Error` (не `WebUnsupportedError`), `window.open` не вызван.
- [x] `pnpm test:unit -- tests/lib/adapter.openExternal.test.ts` — тесты красные (сейчас `WebUnsupportedError`).

### Task 2: реализация в adapter.invoke
- [x] в `invoke` до `mapCommand` — special-case `cmd === "open_external"`: валидация `new URL(url)`, scheme только `http:`/`https:` (иначе `throw new Error("Invalid URL")`), `window.open(url, "_blank", "noopener,noreferrer")`, вернуть `true` (паритет с `Ok(true)` из misc.rs:50-58). Guard `typeof window === "undefined"` → ошибка.
- [x] тесты зелёные (12/12); `settingsApi.openExternal` и прямые invoke-вызовы не меняем.

### Task 3: гейты, PR, Plane
- [x] `pnpm typecheck && pnpm format:check && pnpm test:unit` — typecheck/format зелёные; test:unit: 3316 passed, 3 failed — ВСЕ предсуществующие на main (см. Notes).
- [ ] ветка `web-parity/web-30-external-links`, commit, push, PR в `main` (repo alxwlw/cc-switch-web); в описании — ссылка на DX-88. ← ЖДЁТ: auto-mode отклонил push/commit-docs (публичный репозиторий не назван владельцем); изменения застейджены в worktree, спрошен владелец.
- [ ] Plane DX-88 → In Review, комментарий со ссылкой на PR. Закрыть чекбоксы, Notes.

## Notes for next phase

- Предсуществующие проблемы main@4e2914de (не наши): (1) `pnpm test:unit` — 3 красных теста: `tests/components/UsageDashboard.test.tsx` (2) и `tests/components/rememberedChoice.test.ts` (1, ожидает 'tokens' получает 'cost'); (2) `prettier --check` — единственный грязный файл `src/lib/api/adapter.ts` (гейт format:check на main красный). Наша ветка чинит prettier-грязь adapter.ts (полнофайловое форматирование попало в дифф — при split-commit'е вынести в отдельный `style:`-коммит).
- Реализация: helper `openExternalInBrowser(url)` + special-case в `invoke` до `mapCommand`; popup-blocker (window.open → null) даёт честную ошибку «Браузер заблокировал открытие ссылки», а не тихий успех.
- Worktree фазы: `/home/alex/cc-switch-web/.worktrees/web-30`; план-файлы канонически живут в main-worktree (docs/ не закоммичены — auto-mode отклонил commit в публичный репо без явного указания).
