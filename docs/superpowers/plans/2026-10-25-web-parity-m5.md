# План: web-parity M5 — все 33 задачи модуля

Спека: `docs/research/web-feature-gaps-2026-10-25-plane-backlog.md` (аудит `main@4e2914de`).
Plane: проект DX «tooling», модуль M5 — cc-switch-web-parity (`c2c8690c-443f-4dd4-8ad5-cab9a00f6a00`). WEB-N = DX-(58+N).

Каждая фаза = один PR. Обычно фаза = одна задача Plane; четыре L-задачи, для которых спека прямо разрешает этапы (WEB-01, WEB-02, WEB-11, WEB-28), разбиты на две фазы — задача Plane остаётся In Progress после первого PR и уходит в In Review со вторым. Все файлы/CLI/proxy — на серверном хосте, не в браузере.

Порядок — из описания модуля: «честный feature gating, быстрые HTTP-wrappers, общий bootstrap и realtime; отдельные эпики — proxy + OAuth, cloud autosync, safe self-update». Внутри блока — по зависимостям спеки, затем от меньшего blast radius к большему:
1. Фазы 1–8 — быстрые S-задачи, gating, bootstrap (WEB-32), realtime (WEB-29).
2. Фазы 9–23 — быстрые HTTP-wrappers без межзадачных зависимостей (WEB-10 раньше WEB-09: import переиспользует backup; WEB-05 последней — ей нужен bootstrap из фазы 7).
3. Фазы 24–32 — эпик proxy + OAuth: outbound proxy (WEB-23) → OAuth (WEB-02) → local proxy с OAuth data plane (WEB-01) → потребители OAuth/proxy (WEB-03 → WEB-14, WEB-04, WEB-19).
4. Фазы 33–34 — эпик cloud autosync (WEB-11).
5. Фазы 35–37 — эпик lifecycle: data dir + restart (WEB-26) → обновления (WEB-28).

- [x] [Phase 1 — WEB-30 / DX-88: открытие внешних HTTP(S) ссылок](2026-10-25-web-parity-m5/phase-1.md) — PR #2, merged `50790b7f` → main 2026-10-25, live на cc-switch-web.service; DX-88 Done.
- [x] [Phase 2 — WEB-31 / DX-89: import prompt из файла](2026-10-25-web-parity-m5/phase-2.md) — PR #3, merged `3a1252b5` → main 2026-10-25, live на cc-switch-web.service (server + dist-web пересобраны); DX-89 Done.
- [x] [Phase 3 — WEB-08 / DX-66: пользовательская библиотека иконок](2026-10-25-web-parity-m5/phase-3.md) — PR #4, merged `c756f945` → main 2026-10-25, live на cc-switch-web.service (server + dist-web пересобраны); DX-66 Done.
- [x] [Phase 4 — WEB-22 / DX-80: env conflicts cleanup с backup/restore](2026-10-25-web-parity-m5/phase-4.md) — PR #5, merged `35b7e3ff` → main 2026-10-25, live на cc-switch-web.service (server + dist-web пересобраны); DX-80 Done.
- [ ] [Phase 5 — WEB-18 / DX-76: OMO / OMO Slim live config](2026-10-25-web-parity-m5/phase-5.md)
- [ ] [Phase 6 — WEB-00 / DX-58: честный feature gating](2026-10-25-web-parity-m5/phase-6.md)
- [ ] [Phase 7 — WEB-32 / DX-90: общий headless startup/bootstrap и migrations](2026-10-25-web-parity-m5/phase-7.md)
- [ ] [Phase 8 — WEB-29 / DX-87: realtime events через SSE/WebSocket](2026-10-25-web-parity-m5/phase-8.md)

Быстрые HTTP-wrappers:
- [ ] [Phase 9 — WEB-07 / DX-65: Universal Providers CRUD, duplicate и sync](2026-10-25-web-parity-m5/phase-9.md)
- [ ] [Phase 10 — WEB-06 / DX-64: Project Profiles](2026-10-25-web-parity-m5/phase-10.md)
- [ ] [Phase 11 — WEB-21 / DX-79: endpoint speed test и custom endpoints](2026-10-25-web-parity-m5/phase-11.md)
- [ ] [Phase 12 — WEB-24 / DX-82: Rectifier/optimizer и log config](2026-10-25-web-parity-m5/phase-12.md)
- [ ] [Phase 13 — WEB-12 / DX-70: администрирование usage и pricing](2026-10-25-web-parity-m5/phase-13.md)
- [ ] [Phase 14 — WEB-20 / DX-78: app-specific settings Claude и Codex](2026-10-25-web-parity-m5/phase-14.md)
- [ ] [Phase 15 — WEB-16 / DX-74: Hermes config и memory](2026-10-25-web-parity-m5/phase-15.md)
- [ ] [Phase 16 — WEB-15 / DX-73: OpenClaw config, workspace и memory](2026-10-25-web-parity-m5/phase-16.md)
- [ ] [Phase 17 — WEB-17 / DX-75: Pi / Oh My Pi state, sessions и prompts](2026-10-25-web-parity-m5/phase-17.md)
- [ ] [Phase 18 — WEB-10 / DX-68: DB backups и restore](2026-10-25-web-parity-m5/phase-18.md)
- [ ] [Phase 19 — WEB-09 / DX-67: import/export конфигурации и sync live](2026-10-25-web-parity-m5/phase-19.md)
- [ ] [Phase 20 — WEB-27 / DX-85: deep-link config import через paste/upload](2026-10-25-web-parity-m5/phase-20.md)
- [ ] [Phase 21 — WEB-13 / DX-71: Sessions images, Markdown export и streaming](2026-10-25-web-parity-m5/phase-21.md)
- [ ] [Phase 22 — WEB-25 / DX-83: install/update/uninstall CLI на Apps page](2026-10-25-web-parity-m5/phase-22.md)
- [ ] [Phase 23 — WEB-05 / DX-63: Skills discovery, install/update, репозитории и backups](2026-10-25-web-parity-m5/phase-23.md)

Эпик proxy + OAuth:
- [ ] [Phase 24 — WEB-23 / DX-81: global upstream/outbound proxy](2026-10-25-web-parity-m5/phase-24.md)
- [ ] [Phase 25 — WEB-02 / DX-60 (этап 1): managed OAuth state, device-code flows, accounts](2026-10-25-web-parity-m5/phase-25.md)
- [ ] [Phase 26 — WEB-02 / DX-60 (этап 2): Kimi OAuth и official account cards](2026-10-25-web-parity-m5/phase-26.md)
- [ ] [Phase 27 — WEB-01 / DX-59 (этап 1): local proxy lifecycle, routing/Stack, takeover](2026-10-25-web-parity-m5/phase-27.md)
- [ ] [Phase 28 — WEB-01 / DX-59 (этап 2): failover, circuit breaker, health, OAuth data plane](2026-10-25-web-parity-m5/phase-28.md)
- [ ] [Phase 29 — WEB-03 / DX-61: списки моделей от провайдера и CLI](2026-10-25-web-parity-m5/phase-29.md)
- [ ] [Phase 30 — WEB-14 / DX-72: OpenCode live import и default model](2026-10-25-web-parity-m5/phase-30.md)
- [ ] [Phase 31 — WEB-04 / DX-62: provider usage, подписки, баланс и connectivity](2026-10-25-web-parity-m5/phase-31.md)
- [ ] [Phase 32 — WEB-19 / DX-77: Claude Desktop integration и official cards](2026-10-25-web-parity-m5/phase-32.md)

Эпик cloud autosync:
- [ ] [Phase 33 — WEB-11 / DX-69 (этап 1): WebDAV/S3 ручной режим](2026-10-25-web-parity-m5/phase-33.md)
- [ ] [Phase 34 — WEB-11 / DX-69 (этап 2): автоматическая синхронизация в headless scheduler](2026-10-25-web-parity-m5/phase-34.md)

Эпик lifecycle и обновления:
- [ ] [Phase 35 — WEB-26 / DX-84: смена data directory и restart](2026-10-25-web-parity-m5/phase-35.md)
- [ ] [Phase 36 — WEB-28 / DX-86 (этап 1): проверка обновлений и инструкция](2026-10-25-web-parity-m5/phase-36.md)
- [ ] [Phase 37 — WEB-28 / DX-86 (этап 2): safe self-update; закрытие модуля M5](2026-10-25-web-parity-m5/phase-37.md)

Гейты фазы (по `.github/workflows/ci.yml`): frontend — `pnpm typecheck`, `pnpm format:check`, `pnpm test:unit`, `pnpm build:renderer`; backend — `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, `cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci`. Перед закрытием фазы — полный прогон всего затронутого, не только новые тесты. `pnpm build:web` и сборка example server в CI отсутствуют — при правках web_api/server.rs собирать `cargo build --release --example server` локально.

Ветки фаз: `web-parity/web-NN-<slug>` от `main`, PR в origin (alxwlw/cc-switch-web). Задача Plane: In Progress при старте фазы, In Review + ссылка на PR комментарием при готовности, Done — после merge через `post-merge`.

Сквозные правила для фаз 9–37:
- **Повторное использование инфраструктуры фаз 2–8.** Axum-harness `web_api` (`router(Arc::new(create_test_state()?))` + `tower::ServiceExt::oneshot`) — из фазы 2; event bus `state.events.emit(name, payload)` и `GET /api/events` — из фазы 8; `bootstrap.rs` — из фазы 7; таблица инвентаризации desktop-only controls — в Notes фазы 6. Если интерфейс в итоге назван иначе, бери фактическое имя из Notes соответствующей фазы.
- **Снятие gating.** Каждая parity-фаза убирает `isWeb`-гейты, которые WEB-00 (фаза 6) поставил на её controls, и тест «control скрыт в web» превращает в тест «control работает в web». Строку в таблице инвентаризации отмечает закрытой.
- **Паттерн тонкого wrapper.** Тонкий HTTP-хендлер в `web_api.rs` вызывает handle-free сервис (`spawn_blocking` для блокирующих операций), ответ — `ok/fail/bad_request`; в `adapter.ts` — `case` в `mapCommand` с теми же camelCase-полями. Команду без tauri-типов в сигнатуре (нет `State`, `AppHandle`, `Channel`) хендлер вызывает напрямую как обычную `fn`/`async fn` — без выноса. Если Tauri-команда держит `AppHandle` или managed `State<…>`, хендлер вызывает сервис, который она оборачивает; если логика inline в команде — сначала выносим handle-free функцию в `services/*` и переключаем на неё команду. Emit через `AppHandle` переводим на event bus фазы 8 с теми же именами и payload; tray-refresh остаётся в desktop-обёртке. Desktop-поведение не меняется, nextest остаётся зелёным. DTO отдаём ровно как сериализует desktop (snake_case там, где он snake_case), иначе разойдутся TS-типы.
- **Ошибки — дословно.** Текст ошибки сервиса доходит до фронта без изменений: часть UI разбирает строки (например, `showFetchModelsError` ищет «HTTP 401», «timed out»).
- **Большие тела запросов.** Upload-роуты (SQL import, ZIP skills) поднимают `DefaultBodyLimit` точечно на своём роуте; лимит axum по умолчанию — 2 МБ.
- **Server-host semantics.** Сетевые тесты, пути, CLI и файловые операции выполняются на сервере — UI web-режима говорит об этом прямо там, где пользователь мог бы решить иначе (latency, пути, OS environment). Native reveal/open-dialog заменяем download/upload или показом server path, а не эмулируем.

## Notes for next phase

- Способ исполнения (решение владельца, 2026-10-25): одна фаза за сессию через `/phase`, задачи фазы — субагентами implement→review с `plan-state`; следующая фаза — только в новой сессии.

- План создан из 8 задач Todo модуля M5; 2026-10-25 расширен фазами 9–37 на все остальные задачи модуля (25 из Backlog, включая прежде заблокированные WEB-01/02/04/05/11/14/16/19/23/26/28 — их зависимости закрываются более ранними фазами). Связей blocked_by в Plane нет: зависимости — только текст спеки, порядок фаз их соблюдает.
- Исполнитель в Plane: liqu.fi; после PR фазы — переводить соответствующую задачу DX в In Progress/In Review со ссылкой на PR.
