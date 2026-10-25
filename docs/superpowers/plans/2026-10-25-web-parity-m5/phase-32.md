# Phase 32 — WEB-19 / DX-77: Claude Desktop integration и official cards

Цель: в web работают статус Claude Desktop, default routes, import провайдеров из Claude, создание официальных seed-провайдеров (Claude Desktop, Codex, GrokBuild) и route mode Claude Desktop через local proxy. UI прямо говорит, что речь об установке Claude Desktop на сервере, а не на машине пользователя.

Спека: § WEB-19. Оценка M (3–5 дн.). Route mode опирается на фазы 27–28.

⚠️ Уточнение к спеке: от WEB-01 зависит только полный route mode. Шесть команд — DB-only или чтение файлов; на диск Claude Desktop пишет не эти команды, а apply провайдера (switch/takeover/route resync) через `write_direct`/`apply_provider`, уже доступные в web.

Scope: `src-tauri/src/web_api.rs` (6 роутов), `src-tauri/tests/web_api_claude_desktop.rs`, `src/lib/api/adapter.ts` (6 case), `src/components/apps/AppsPage.tsx`, `src/components/providers/mode/*` (server-host подписи), тесты фронта.

Факты разведки:
- Фронт providers.ts:160-180 (без аргументов): `import_claude_desktop_providers_from_claude`, `ensure_claude_desktop_official_provider`, `ensure_codex_official_provider`, `ensure_grokbuild_official_provider`, `get_claude_desktop_status`, `get_claude_desktop_default_routes`; тип `ClaudeDesktopStatus` (providers.ts:~212-229). Вызовы: `AppsPage.tsx:182`, `useDesktopConfigMessages.ts:14`, `ClaudeDesktopProviderForm.tsx:322`, `ProviderList.tsx:223`, `lib/query/mutations.ts:47-58`.
- Backend commands/provider.rs, без AppHandle (регистрация lib.rs:1529-1534): :279 `get_claude_desktop_status(state)` (async) → `claude_desktop_config::get_status(&Database, proxy_running)` (claude_desktop_config.rs:215), `proxy_running = state.proxy_service.is_running()`; :288 `get_claude_desktop_default_routes()` → `default_proxy_routes()` (:305); :294 `import_claude_desktop_providers_from_claude(state) -> usize` — копирует Claude-провайдеров в таблицу claude-desktop, затем `ensure_official_seed_by_id`; :348/:359/:367 `ensure_{claude_desktop|codex|grokbuild}_official_provider(state) -> bool` → `Database::ensure_official_seed_by_id` (dao/providers.rs:846).
- Файлы при apply: `<Claude dir>/claude_desktop_config.json`, `<Claude-3p dir>/claude_desktop_config.json`, `<Claude-3p>/configLibrary/00000000-0000-4000-8000-000000157210.json` (профиль «CC Switch», PROFILE_ID claude_desktop_config.rs:22), `<Claude-3p>/configLibrary/_meta.json`. Gateway token — DB-настройка `claude_desktop_gateway_token`; префикс пути proxy — `/claude-desktop` (:28-29).
- Linux поддерживается: `is_supported_platform()` (:1487) — macOS, Windows, Linux; на Linux — `$XDG_CONFIG_HOME` (если абсолютный) или `~/.config`, затем `{Claude, Claude-3p}`, во Flatpak всегда `~/.config` (:1525-1549). `installed` определяется по каталогу данных (`detect_installed`, :1471).
- Семантика web: файлы пишутся на сервере, а клиент Claude Desktop обычно работает на машине пользователя — без подписи это ловушка.
- DTO `ClaudeDesktopStatus` (:102), `ClaudeDesktopDefaultRoute` (:48-52, `supports1m`) — camelCase.
- Тесты: in-file `mod tests` claude_desktop_config.rs (:1629+, включая `detect_installed_reads_data_dir_or_app_bundle` и linux config dir).

### Task 1: backend — роуты + тесты
- [ ] `GET /api/claude-desktop/status`, `GET /api/claude-desktop/default-routes`, `POST /api/claude-desktop/import-from-claude`, `POST /api/official-seeds/:app/ensure` (`app ∈ {claude-desktop, codex, grokbuild}`, иначе 400).
- [ ] `src-tauri/tests/web_api_claude_desktop.rs` (temp HOME, `XDG_CONFIG_HOME` → temp): без каталога Claude → `installed:false`, `supported:true`; с каталогом → `installed:true`; status при запущенном proxy (фаза 27) → `proxyRunning:true`; import копирует N Claude-провайдеров и создаёт официальный seed; ensure — идемпотентен (`true`, затем `false`); takeover Claude Desktop в route mode через роуты фазы 27 → профиль `configLibrary/…157210.json` создан с proxy URL и префиксом `/claude-desktop`, выход из takeover → файлы восстановлены.

### Task 2: frontend
- [ ] 6 case в `mapCommand`.
- [ ] `AppsPage`/карточки Claude Desktop в web: подпись «Claude Desktop на сервере cc-switch-web»; при `installed:false` — «Claude Desktop не найден на сервере; настройки применятся, когда он появится там», без тревожного вида.
- [ ] Тесты: `tests/lib/claudeDesktopApi.test.ts`; `AppsPage` в web показывает server-host подпись и состояние `installed:false`.
- [ ] Снять WEB-00-гейты Claude Desktop (таблица фазы 6).

### Task 3: гейты, PR, Plane
- [ ] Полный набор гейтов + `cargo build --release --example server`.
- [ ] Ветка `web-parity/web-19-claude-desktop`, PR; DX-77 → In Review + ссылка. Закрыть чекбоксы, Notes.

Риски для ревью:
- Route mode Claude Desktop ведёт клиента на `127.0.0.1:<порт proxy>` сервера: если Claude Desktop стоит на другой машине, он туда не достучится. UI route mode в web должен это сказать (и подсказать про client host из фазы 27), а тест — зафиксировать, что URL строится из настроенного client host.

## Notes for next phase

- (заполнить по итогам)
