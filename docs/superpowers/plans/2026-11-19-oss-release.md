# План: cc-switch-web как самостоятельный OSS-продукт

**Status:** active

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** посторонний человек ставит консоль cc-switch на свой Linux-сервер без десктопа одной командой (бинарник из релиза или Docker), открывает её через SSH-туннель или Tailscale и обновляет из консоли.

**Architecture:** версия форка зашивается в бинарник при сборке; проверка обновлений и самообновление получают второй режим — «по релизам», рядом с нынешним «по коммитам» для сборок из исходников. Новый `scripts/install.sh` ставит архив релиза и systemd user unit; новый workflow `release-web.yml` по тегу собирает архивы, образ ghcr и публикует релиз. Собственные файлы форка (README, `.github/`, …) перечислены в `docs/upstream.lock` и защищаются скриптом синхронизации.

**Tech Stack:** Rust (axum, reqwest, serde; крейт `cc_switch_lib` + `examples/server.rs`), React + Vite + vitest, bash, GitHub Actions, Docker (buildx, ghcr).

**Spec:** `docs/superpowers/specs/2026-11-19-oss-release-design.md` — исполнитель читает её вместе с фазой.

Каждая фаза — один PR и одна сессия (`/phase`, задачи — субагентами implement→review с `plan-state`).

## Global Constraints

- Тег релиза: `v<апстрим>-web.<N>`, регулярка `^v[0-9]+\.[0-9]+\.[0-9]+-web\.[0-9]+$`; `<апстрим>` = `version` из `src-tauri/Cargo.toml`. Имя версии без `v` (`4.0.35-web.1`) — в именах архивов, образа и в `webVersion`.
- Версии в `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` не меняются: они апстримовские.
- Код форка — только за адаптерами: `src-tauri/src/web_api*`, `src-tauri/src/services/web_update.rs`, `src-tauri/examples/server.rs`, `scripts/`, `deploy/`, веб-специфичные файлы `src/` (`AboutSection.tsx` web-ветка, `SelfUpdatePanel.tsx`, `WebCommitList.tsx`, `src/lib/updater.ts`, `src/lib/api/{capabilities,selfUpdate}.ts`, `src/hooks/useSelfUpdate.ts`), ключи `web.*` в локалях.
- Сервер по умолчанию слушает `127.0.0.1:18766`; не-loopback адрес — только с `TRUSTED_TAILNET=1`. Ни установщик, ни образ, ни compose не открывают порт наружу по умолчанию.
- Только Linux, `x86_64` и `aarch64`; бинарники собираются на `ubuntu-22.04` и `ubuntu-22.04-arm`.
- Системные пакеты рантайма (точный список из `Dockerfile.web`): `libssl3 libgtk-3-0 libayatana-appindicator3-1 librsvg2-2 libwebkit2gtk-4.1-0`.
- Раскладка установки из релиза: бинарник `~/.local/bin/cc-switch-web`; каталог установки `~/.local/share/cc-switch-web/` (`dist-web/`, `install.sh`, `self-update.sh`); unit `~/.config/systemd/user/cc-switch-web.service`; данные `~/.cc-switch-web`.
- Репозиторий `alxwlw/cc-switch-web`; PR только `gh pr create --draft -R alxwlw/cc-switch-web --base main`.
- Тег релиза ставится только после явного «выпускай» владельца на этот релиз.
- В публикуемых файлах — только аккаунт `alxwlw`, без email и настоящего имени.
- README, docs и тексты `en.json` — по-английски; планы и журнал — по-русски.
- Фаза 2 начинается только после того, как синхронизация `sync/upstream-6da70837` слита в `main`.

## Review Focus

- Повторный запуск `install.sh` без флагов на установке с `--tailscale` сохраняет адрес tailnet, порт и выключенное самообновление (иначе доступ молча меняется).
- Переустановка и самообновление при работающем сервисе не пишут поверх запущенного бинарника («Text file busy»): только временный файл и `mv`.
- Битый или неполный архив, несовпадение `SHA256SUMS`, нет архива для этой архитектуры — ничего не установлено и не заменено, старая версия продолжает работать, причина названа.
- Последний релиз старше установленного или его тег не разбирается как версия форка — статус `unknown`, самообновление отказывает до замены.
- Контейнер, запущенный от UID хоста без записи в `/etc/passwd`, создаёт `~/.cc-switch-web` с владельцем — этим UID, не root.

## Фазы

- [x] [Phase 1 — версия, обновления по релизам, установщик, релизный workflow, Docker](2026-11-19-oss-release/phase-1.md) — можно начинать сразу; файлы фазы синхронизация не трогает, кроме одного места в `web_api.rs` (`capabilities_payload`, `update_check_with`).
  - PR: https://github.com/alxwlw/cc-switch-web/pull/43, branch feat-cld/oss-release-p1
- [x] [Phase 2 — консоль, лицо проекта, `.github/`, синхронизация](2026-11-19-oss-release/phase-2.md) — после слияния синхронизации с апстримом.
  - PR: https://github.com/alxwlw/cc-switch-web/pull/44, branch feat-cld/oss-release-p2
- [ ] [Phase 3 — первый релиз, переезд развёртываний, проверка самообновления](2026-11-19-oss-release/phase-3.md) — после слияния фазы 2; каждый тег — после «выпускай».

## Notes for next phase

- Способ исполнения: одна фаза за сессию через `/phase`.
- Plane: задачи под этот план не заведены. Если владелец захочет — модуль в проекте DX «tooling».
