# Phase 37 — WEB-28 / DX-86 (этап 2): safe self-update; закрытие модуля M5

Цель: оператор, явно включивший self-update, обновляет systemd-развёртывание из web-консоли: fast-forward checkout, сборка, атомарная замена бинарника и `dist-web`, restart, health-check, откат при неудаче. Docker остаётся на инструкции из фазы 36. DX-86 закрывается; последняя задача фазы — закрытие модуля M5.

Спека: § WEB-28 (safe self-update — L, 5–10 дн.).

Scope: `scripts/self-update.sh` (новый), `scripts/tests/self-update.test.sh` (новый), `src-tauri/src/services/web_update.rs` (запуск и статус), `src-tauri/src/web_api.rs` (2 роута), `src/components/settings/AboutSection.tsx`, `src/contexts/UpdateContext.tsx`, `docs/deploy.md`.

Факты разведки:
- Развёртывание: `scripts/install-server.sh:25-26` собирает `cargo build --release --example server`, ставит в `~/.local/bin/cc-switch-web`, рендерит `scripts/cc-switch-web.service.template` в `~/.config/systemd/user/cc-switch-web.service` (`Restart=always`, `RestartSec=3`, `PORT=18766`, `CC_SWITCH_CONFIG_DIR=%h/.cc-switch-web`, `WEB_DIST=%h/cc-switch-web/dist-web`). `dist-web` собирается `pnpm build:web` в checkout и раздаётся с диска.
- Desktop `install_update_and_restart` (commands/settings.rs:277) эмитит `update-download-progress {downloaded,total}`; `DatabaseUpgrade.tsx:98` его слушает. Для web это событие не подходит: SSE рвётся на restart.
- После фазы 35 есть детект supervisor (`INVOCATION_ID` / `CC_SWITCH_WEB_SUPERVISED`); после фазы 36 — `BUILD_SHA`, `GET /api/update/check`, тексты инструкций.

### Task 1: скрипт обновления
- [ ] `scripts/self-update.sh <source_dir> <branch>`; каждый шаг пишет статус в `${XDG_STATE_HOME:-~/.local/state}/cc-switch-web/update-status.json` (`{stage, ok, message, fromSha, toSha, startedAt, finishedAt}`).
- [ ] Шаги: отказ при грязном дереве → `git fetch origin` → `git merge --ff-only origin/<branch>` (non-ff → отказ) → `pnpm install --frozen-lockfile && pnpm build:web` в `dist-web.new` → `cargo build --release --example server` с `CC_SWITCH_WEB_GIT_SHA` → `install -m755` в `cc-switch-web.new` → текущие бинарник и `dist-web` переименовать в `.prev`, новые — на их место (`mv` в пределах одной ФС) → `systemctl --user restart cc-switch-web` → до 60 с опрашивать `http://127.0.0.1:$PORT/api/capabilities` и сверять `buildSha` с новым HEAD.
- [ ] Неудачный health-check → вернуть `.prev` на место, restart, `stage: "rolled-back"`. Checkout остаётся на новом коммите (`git reset --hard` не делаем), это пишется в `message`.
- [ ] `scripts/tests/self-update.test.sh`: временные git-репозитории и заглушки `cargo`/`pnpm`/`systemctl`/`curl` в `PATH`; кейсы — успех, грязное дерево, non-ff, health-check провален → откат восстановил `.prev`.

### Task 2: запуск из сервера
- [ ] Включение только явно: `CC_SWITCH_WEB_SELF_UPDATE=1` + `CC_SWITCH_WEB_SOURCE_DIR` + systemd (`INVOCATION_ID`). Иначе `POST /api/update/install` → 409 с причиной; для Docker — текст инструкции.
- [ ] Запуск через `systemd-run --user --unit=cc-switch-web-update --collect scripts/self-update.sh …`: transient unit переживает restart основного сервиса. Повторный запуск при живом unit → 409 «обновление уже идёт».
- [ ] `GET /api/update/status` → содержимое `update-status.json` (или `null`).
- [ ] Spawner инъецируемый; тесты: 409 при выключенном self-update / без systemd / при идущем обновлении; при включённом — spawner получил ожидаемые аргументы; status парсит файл.

### Task 3: frontend
- [ ] `AboutSection` (web, self-update включён — признак в `/api/capabilities`): кнопка «обновить» при `behind`, подтверждение со списком коммитов из фазы 36.
- [ ] После запуска — опрос `GET /api/update/status` раз в 2 с (SSE не переживает restart); при недоступности сервера ждать и продолжать опрос; `done` → `location.reload()`; `rolled-back`/ошибка → показать `message`.
- [ ] Тесты: кнопка скрыта без self-update; поток `building → restarting → done` по mock-статусам; `rolled-back` показывает причину.

### Task 4: гейты, PR, Plane
- [ ] Полный набор гейтов + `bash scripts/tests/self-update.test.sh`; ручной прогон на agentbox: обновление с коммита N-1 на N через UI, затем искусственно сломанная сборка → откат.
- [ ] `docs/deploy.md`: включение self-update, что делает скрипт, как откатиться вручную (`.prev`).
- [ ] Ветка `web-parity/web-28-self-update`, PR; DX-86 → In Review + ссылка.

### Task 5: закрытие модуля M5
- [ ] После merge: все 33 задачи модуля в Done (сверка `module list_workitems`); модуль `c2c8690c-…` → status `completed`.
- [ ] Индекс `docs/superpowers/plans/2026-10-25-web-parity-m5.md`: все чекбоксы закрыты, итог в Notes; таблица инвентаризации фазы 6 — все строки закрыты или отмечены «desktop-only навсегда» (трей, native-диалоги, терминал, Agy keychain на Linux).

Риски для ревью:
- Self-update по HTTP без аутентификации: любой участник tailnet может запустить сборку. Поэтому opt-in через env, ff-only и только ветка из конфигурации; в `docs/deploy.md` это сказано прямо.
- Диск заполнен во время сборки → старые бинарник и `dist-web` не тронуты, так как замена идёт только после успешной сборки.

## Notes for next phase

- (заполнить по итогам)
