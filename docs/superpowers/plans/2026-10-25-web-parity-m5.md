# План: web-parity M5 — восемь задач из Todo

Спека: `docs/research/web-feature-gaps-2026-10-25-plane-backlog.md` (аудит `main@4e2914de`).
Plane: проект DX «tooling», модуль M5 — cc-switch-web-parity (`c2c8690c-443f-4dd4-8ad5-cab9a00f6a00`).

Каждая фаза = одна задача Plane = один PR. Порядок: быстрые S-задачи (меньше риск конфликтов и сразу видимый parity), затем честный gating (меньше поверхность, которую прятать), затем инфраструктурные. Все файлы/CLI/proxy — на серверном хосте, не в браузере.

- [x] [Phase 1 — WEB-30 / DX-88: открытие внешних HTTP(S) ссылок](2026-10-25-web-parity-m5/phase-1.md) — PR #2, merged `50790b7f` → main 2026-10-25, live на cc-switch-web.service; DX-88 Done.
- [ ] [Phase 2 — WEB-31 / DX-89: import prompt из файла](2026-10-25-web-parity-m5/phase-2.md)
- [ ] [Phase 3 — WEB-08 / DX-66: пользовательская библиотека иконок](2026-10-25-web-parity-m5/phase-3.md)
- [ ] [Phase 4 — WEB-22 / DX-80: env conflicts cleanup с backup/restore](2026-10-25-web-parity-m5/phase-4.md)
- [ ] [Phase 5 — WEB-18 / DX-76: OMO / OMO Slim live config](2026-10-25-web-parity-m5/phase-5.md)
- [ ] [Phase 6 — WEB-00 / DX-58: честный feature gating](2026-10-25-web-parity-m5/phase-6.md)
- [ ] [Phase 7 — WEB-32 / DX-90: общий headless startup/bootstrap и migrations](2026-10-25-web-parity-m5/phase-7.md)
- [ ] [Phase 8 — WEB-29 / DX-87: realtime events через SSE/WebSocket](2026-10-25-web-parity-m5/phase-8.md)

Гейты фазы (по `.github/workflows/ci.yml`): frontend — `pnpm typecheck`, `pnpm format:check`, `pnpm test:unit`, `pnpm build:renderer`; backend — `cargo fmt --check --manifest-path src-tauri/Cargo.toml`, `cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings`, `cargo nextest run --manifest-path src-tauri/Cargo.toml --profile ci`. Перед закрытием фазы — полный прогон всего затронутого, не только новые тесты. `pnpm build:web` и сборка example server в CI отсутствуют — при правках web_api/server.rs собирать `cargo build --release --example server` локально.

Ветки фаз: `web-parity/web-NN-<slug>` от `main`, PR в origin (alxwlw/cc-switch-web). Задача Plane: In Progress при старте фазы, In Review + ссылка на PR комментарием при готовности.

## Notes for next phase

- План создан из 8 задач Todo модуля M5; заблокированные (WEB-01/02/04/05/11/14/16/19/23/26/28) не входят.
- Исполнитель в Plane: liqu.fi; после PR фазы — переводить соответствующую задачу DX в In Progress/In Review со ссылкой на PR.
