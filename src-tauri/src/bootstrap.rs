//! Handle-free startup bootstrap shared by the Tauri `setup` hook and the
//! headless web server (WEB-32).
//!
//! The work is split in two stages so the desktop can keep its native dialogs
//! between them:
//!
//! 1. **Pre-DB** ([`BootstrapPaths::resolve`], [`load_legacy_config`],
//!    [`precheck_db_version`], [`open_database`]): paths, legacy `config.json`
//!    validation, "DB too new" precheck, `Database::init`. These return typed
//!    [`BootstrapError`]s and never show UI. The desktop wraps them in its
//!    retry/exit dialogs; [`run_headless_bootstrap`] records the failure in
//!    `init_status` and returns it.
//! 2. **Post-DB** ([`apply_persisted_log_level`], [`migrate_legacy_config`],
//!    [`run_post_db_bootstrap`]): legacy migration, crash recovery, seeds and
//!    first-run imports. Never fatal: every step logs its problem and records
//!    it in the [`BootstrapReport`].
//!
//! Step order and log messages mirror the desktop `setup` hook this was
//! extracted from.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::app_config::{AppType, MultiAppConfig};
use crate::database::Database;
use crate::error::AppError;
use crate::init_status::{self, InitErrorPayload};
use crate::store::AppState;

/// Result of one bootstrap step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    /// The step ran and changed or imported something (or simply completed).
    Done,
    /// Nothing to do (table not empty, no live config, flag not set, ...).
    Skipped(String),
    /// The step failed; startup continued. Carries the error text.
    Failed(String),
    /// Handed to the caller's spawner; its result is not tracked here.
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepReport {
    pub name: &'static str,
    pub outcome: StepOutcome,
}

/// Per-step record of a bootstrap run, in execution order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootstrapReport {
    pub steps: Vec<StepReport>,
    /// `true` when `config.json` was migrated into the database in this run.
    pub legacy_config_migrated: bool,
    /// `true` when the providers table was empty before any import ran.
    pub fresh_install: bool,
}

impl BootstrapReport {
    fn push(&mut self, name: &'static str, outcome: StepOutcome) {
        self.steps.push(StepReport { name, outcome });
    }

    pub fn step(&self, name: &str) -> Option<&StepOutcome> {
        self.steps
            .iter()
            .find(|s| s.name == name)
            .map(|s| &s.outcome)
    }

    /// `(step name, error text)` for every failed step.
    pub fn failures(&self) -> Vec<(&'static str, &str)> {
        self.steps
            .iter()
            .filter_map(|s| match &s.outcome {
                StepOutcome::Failed(e) => Some((s.name, e.as_str())),
                _ => None,
            })
            .collect()
    }
}

/// Fatal startup problems (pre-DB stage only).
#[derive(Debug)]
pub enum BootstrapError {
    /// Legacy `config.json` exists (and no DB yet) but cannot be loaded.
    LegacyConfig(AppError),
    /// The on-disk database was written by a newer app.
    DbVersionTooNew { path: PathBuf, version: i32 },
    /// `Database::init` failed.
    DatabaseInit { path: PathBuf, message: String },
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LegacyConfig(e) => write!(f, "failed to load legacy config.json: {e}"),
            Self::DbVersionTooNew { path, version } => write!(
                f,
                "database {} has schema version {version}, newer than supported {}",
                path.display(),
                crate::database::SCHEMA_VERSION
            ),
            Self::DatabaseInit { path, message } => {
                write!(f, "failed to init database {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for BootstrapError {}

/// Where the legacy config and the database live.
#[derive(Debug, Clone)]
pub struct BootstrapPaths {
    pub app_config_dir: PathBuf,
    pub db_path: PathBuf,
    pub json_path: PathBuf,
}

impl BootstrapPaths {
    pub fn resolve() -> Self {
        let app_config_dir = crate::config::get_app_config_dir();
        Self {
            db_path: app_config_dir.join("cc-switch.db"),
            json_path: app_config_dir.join("config.json"),
            app_config_dir,
        }
    }
}

/// Whether a legacy `config.json` must be migrated: no DB yet and a
/// `config.json` on disk. Evaluate it ONCE, before the first load attempt, and
/// reuse the answer across retries (the desktop retry loop does).
pub fn needs_legacy_migration(paths: &BootstrapPaths) -> bool {
    !paths.db_path.exists() && paths.json_path.exists()
}

/// One attempt to load the legacy `config.json`. Call only when
/// [`needs_legacy_migration`] said so; retries call it again without
/// re-deciding.
///
/// Runs before the database is created so a failed load leaves no DB behind
/// and the next start can retry. The desktop loops around this with a dialog.
pub fn load_legacy_config() -> Result<MultiAppConfig, BootstrapError> {
    log::info!("检测到旧版配置文件，验证配置文件...");
    match MultiAppConfig::load() {
        Ok(config) => {
            log::info!("✓ 配置文件加载成功");
            Ok(config)
        }
        Err(e) => {
            log::error!("加载旧配置文件失败: {e}");
            Err(BootstrapError::LegacyConfig(e))
        }
    }
}

/// Refuse to touch a database newer than this build understands. On
/// `Err(DbVersionTooNew)` the `init_status` error is already recorded, so the
/// frontend can show the "upgrade the app" screen; the caller must stop.
/// A failing precheck itself is only logged (startup continues).
pub fn precheck_db_version(paths: &BootstrapPaths) -> Result<(), BootstrapError> {
    match Database::stored_user_version_exceeds_supported(&paths.db_path) {
        Ok(Some(version)) => {
            log::warn!("数据库版本过新（v{version}），引导用户在应用内升级应用");
            init_status::set_init_error(InitErrorPayload {
                path: paths.db_path.display().to_string(),
                error: format!(
                    "数据库版本过新（{version}），当前应用仅支持 {}，请升级应用后再尝试。",
                    crate::database::SCHEMA_VERSION
                ),
                kind: Some("db_version_too_new".to_string()),
                db_version: Some(version),
                supported_version: Some(crate::database::SCHEMA_VERSION),
            });
            Err(BootstrapError::DbVersionTooNew {
                path: paths.db_path.clone(),
                version,
            })
        }
        Ok(None) => Ok(()),
        Err(e) => {
            log::warn!("预检数据库版本失败，继续正常初始化流程: {e}");
            Ok(())
        }
    }
}

/// One attempt at `Database::init` (includes schema migration).
pub fn open_database(paths: &BootstrapPaths) -> Result<Arc<Database>, BootstrapError> {
    match Database::init() {
        Ok(db) => Ok(Arc::new(db)),
        Err(e) => {
            log::error!("Failed to init database: {e}");
            Err(BootstrapError::DatabaseInit {
                path: paths.db_path.clone(),
                message: e.to_string(),
            })
        }
    }
}

/// Apply the persisted log level as soon as the DB is readable. A corrupt
/// config fails closed to `info`.
pub fn apply_persisted_log_level(db: &Database) {
    match db.get_log_config() {
        Ok(log_config) => {
            log::set_max_level(log_config.to_level_filter());
            log::info!(
                "已加载日志配置: enabled={}, level={}",
                log_config.enabled,
                log_config.level
            );
        }
        Err(e) => {
            log::set_max_level(log::LevelFilter::Info);
            log::warn!("读取日志配置失败，已回退到 info: {e}");
        }
    }
}

/// Import a pre-validated legacy config into the DB, mark the success flag
/// and archive `config.json` as `config.json.migrated`.
pub fn migrate_legacy_config(
    db: &Database,
    paths: &BootstrapPaths,
    config: &MultiAppConfig,
) -> StepOutcome {
    log::info!("开始执行数据迁移...");
    match db.migrate_from_json(config) {
        Ok(_) => {
            log::info!("✓ 配置迁移成功");
            init_status::set_migration_success();
            let archive_path = paths.json_path.with_extension("json.migrated");
            if let Err(e) = std::fs::rename(&paths.json_path, &archive_path) {
                log::warn!("归档旧配置文件失败: {e}");
            } else {
                log::info!("✓ 旧配置已归档为 config.json.migrated");
            }
            StepOutcome::Done
        }
        Err(e) => {
            // 配置加载成功但迁移失败的情况极少（磁盘满等），仅记录日志
            log::error!("配置迁移失败: {e}，将从现有配置导入");
            StepOutcome::Failed(e.to_string())
        }
    }
}

/// What to do with the (blocking, file-system heavy) Codex history
/// migrations.
pub enum CodexHistory<'a> {
    /// Run them synchronously inside [`run_post_db_bootstrap`].
    Inline,
    /// Hand the work to the caller (desktop: Tauri's blocking pool). The step
    /// is reported as [`StepOutcome::Deferred`].
    Spawn(&'a dyn Fn(Box<dyn FnOnce() + Send + 'static>)),
}

/// The four Codex history migrations, in their startup order. Each logs its
/// own outcome; failures are warnings only.
pub fn run_codex_history_migrations(db: &Database) {
    use crate::codex_history_migration as m;

    match m::maybe_migrate_codex_third_party_history_provider_bucket(db) {
        Ok(outcome) => {
            if let Some(reason) = outcome.skipped_reason {
                log::debug!("○ Codex history provider bucket migration skipped: {reason}");
            } else {
                log::info!(
                    "✓ Codex history provider bucket migration completed: sources={}, jsonl_files={}, state_rows={}",
                    outcome.source_provider_ids.len(),
                    outcome.migrated_jsonl_files,
                    outcome.migrated_state_rows
                );
            }
        }
        Err(e) => {
            log::warn!("✗ Codex history provider bucket migration failed: {e}");
        }
    }

    match m::maybe_migrate_codex_provider_template_bucket(db) {
        Ok(outcome) => {
            if let Some(reason) = outcome.skipped_reason {
                log::debug!("○ Codex provider template bucket migration skipped: {reason}");
            } else if !outcome.migrated_provider_ids.is_empty() {
                log::info!(
                    "✓ Codex provider template bucket migration completed: providers={}",
                    outcome.migrated_provider_ids.len()
                );
            }
        }
        Err(e) => {
            log::warn!("✗ Codex provider template bucket migration failed: {e}");
        }
    }

    // 旧版官方路由桶（cc-switch-official）迁回 openai，须在统一迁移之前：
    // 开关开着时，紧接着的统一迁移会把它们按正常流程带进 custom。
    match m::maybe_migrate_codex_legacy_official_history_to_openai_bucket() {
        Ok(outcome) => {
            if let Some(reason) = outcome.skipped_reason {
                log::debug!("○ Codex legacy official history bucket migration skipped: {reason}");
            } else {
                log::info!(
                    "✓ Codex legacy official history bucket migration completed: jsonl_files={}, state_rows={}",
                    outcome.migrated_jsonl_files,
                    outcome.migrated_state_rows
                );
            }
        }
        Err(e) => {
            log::warn!("✗ Codex legacy official history bucket migration failed: {e}");
        }
    }

    // 统一会话开关的官方历史迁移：开关开启但上次未完成（如文件被占用
    // 中途失败）时在启动期重试；函数内部自门控，开关关闭时直接跳过。
    match m::maybe_migrate_codex_official_history_to_unified_bucket() {
        Ok(outcome) => {
            if let Some(reason) = outcome.skipped_reason {
                log::debug!("○ Codex official history unify migration skipped: {reason}");
            } else {
                log::info!(
                    "✓ Codex official history unify migration completed: jsonl_files={}, state_rows={}",
                    outcome.migrated_jsonl_files,
                    outcome.migrated_state_rows
                );
            }
        }
        Err(e) => {
            log::warn!("✗ Codex official history unify migration failed: {e}");
        }
    }
}

/// Everything after the DB is open and `AppState` exists: crash recovery,
/// seeds and first-run imports. Idempotent; never fails fatally. Steps are
/// appended to `report` in execution order.
pub fn run_post_db_bootstrap(
    state: &AppState,
    codex_history: CodexHistory<'_>,
    report: &mut BootstrapReport,
) {
    let db = &state.db;

    // 补完上次崩溃时写到一半的客户端文件（写前意图在 ~/.cc-switch/live-state.json），
    // 要在任何写客户端文件的启动步骤之前。
    crate::mode::operation::recover_on_startup(db);
    report.push("recover_on_startup", StepOutcome::Done);

    // 1. 初始化默认 Skills 仓库（已有内置检查：表非空则跳过）
    match db.init_default_skill_repos() {
        Ok(count) if count > 0 => {
            log::info!("✓ Initialized {count} default skill repositories");
            report.push("skill_repos", StepOutcome::Done);
        }
        Ok(_) => report.push(
            "skill_repos",
            StepOutcome::Skipped("table not empty".into()),
        ),
        Err(e) => {
            log::warn!("✗ Failed to initialize default skill repos: {e}");
            report.push("skill_repos", StepOutcome::Failed(e.to_string()));
        }
    }

    // 1.1. Skills 统一管理迁移：当数据库迁移到 v3 结构后，自动从各应用目录导入到 SSOT
    // 触发条件由 schema 迁移设置 settings.skills_ssot_migration_pending = true 控制。
    let ssot = match db.get_setting("skills_ssot_migration_pending") {
        Ok(Some(flag)) if flag == "true" || flag == "1" => {
            // 安全保护：如果用户已经有 v3 结构的 Skills 数据，就不要自动清空重建。
            let has_existing = db
                .get_all_installed_skills()
                .map(|skills| !skills.is_empty())
                .unwrap_or(false);

            if has_existing {
                log::info!(
                    "Detected skills_ssot_migration_pending but skills table not empty; skipping auto import."
                );
                let _ = db.set_setting("skills_ssot_migration_pending", "false");
                StepOutcome::Skipped("skills table not empty".into())
            } else {
                match crate::services::skill::migrate_skills_to_ssot(db) {
                    Ok(count) => {
                        log::info!("✓ Auto imported {count} skill(s) into SSOT");
                        if count > 0 {
                            init_status::set_skills_migration_result(count);
                        }
                        let _ = db.set_setting("skills_ssot_migration_pending", "false");
                        StepOutcome::Done
                    }
                    Err(e) => {
                        log::warn!("✗ Failed to auto import legacy skills to SSOT: {e}");
                        init_status::set_skills_migration_error(e.to_string());
                        // 保留 pending 标志，方便下次启动重试
                        StepOutcome::Failed(e.to_string())
                    }
                }
            }
        }
        Ok(_) => StepOutcome::Skipped("migration flag not set".into()), // 未开启迁移标志
        Err(e) => {
            log::warn!("✗ Failed to read skills migration flag: {e}");
            StepOutcome::Failed(e.to_string())
        }
    };
    report.push("skills_ssot_migration", ssot);

    // 1.5. 自动导入 live 配置 + seed 官方预设供应商（Claude / Codex / Gemini）
    //
    // 先 import 后 seed 是有意为之：先把用户手动配置的 settings.json / auth.json / .env
    // 落成 "default" provider 设为 current，再追加官方预设（is_current=false）。
    // 降级回旧版时，旧版的回填和整份写入仍靠这一行保住用户原来的 live 配置。
    //
    // 捕获首次运行快照：所有全新装用户都会看到欢迎弹窗介绍 CC Switch 的工作方式。
    // 读失败时默认不弹，宁可漏弹也不要因为故障打扰用户。
    let first_run_already_confirmed = crate::settings::get_settings()
        .first_run_notice_confirmed
        .unwrap_or(false);
    let fresh_install_at_startup = db.is_providers_empty().unwrap_or(false);
    report.fresh_install = fresh_install_at_startup;

    let mut imported_apps = 0usize;
    for app_type in AppType::all().filter(|t| !t.is_additive_mode()) {
        if !crate::services::provider::should_import_default_config_on_startup(state, &app_type)
            .unwrap_or(false)
        {
            log::debug!(
                "○ {} already has providers; live import skipped",
                app_type.as_str()
            );
            continue;
        }

        match crate::services::provider::import_default_config(state, app_type.clone()) {
            Ok(true) => {
                imported_apps += 1;
                log::info!(
                    "✓ Imported live config for {} as default provider",
                    app_type.as_str()
                );
            }
            Ok(false) => log::debug!(
                "○ {} already has providers; live import skipped",
                app_type.as_str()
            ),
            Err(e) => log::debug!("○ No live config to import for {}: {e}", app_type.as_str()),
        }
    }
    report.push(
        "default_live_import",
        if imported_apps > 0 {
            StepOutcome::Done
        } else {
            StepOutcome::Skipped("nothing imported".into())
        },
    );

    match db.init_default_official_providers() {
        Ok(count) if count > 0 => {
            log::info!("✓ Seeded {count} official provider(s)");
            report.push("official_providers", StepOutcome::Done);
        }
        Ok(_) => report.push(
            "official_providers",
            StepOutcome::Skipped("already seeded".into()),
        ),
        Err(e) => {
            log::warn!("✗ Failed to seed official providers: {e}");
            report.push("official_providers", StepOutcome::Failed(e.to_string()));
        }
    }

    match codex_history {
        CodexHistory::Inline => {
            run_codex_history_migrations(db);
            report.push("codex_history", StepOutcome::Done);
        }
        CodexHistory::Spawn(spawn) => {
            let db_for_codex_history_migration = db.clone();
            spawn(Box::new(move || {
                run_codex_history_migrations(&db_for_codex_history_migration)
            }));
            report.push("codex_history", StepOutcome::Deferred);
        }
    }

    // 老用户 / 已确认的路径由 `fresh_install_at_startup` 自行拦截，这里不做写入。
    // 字段只由前端在用户点击"我知道了"时 save_settings 回写，语义是"用户显式确认过"。
    if !first_run_already_confirmed && fresh_install_at_startup {
        log::info!("✓ First-run welcome notice pending");
    }

    // 1.6. 自动同步累加模式应用的原生 providers 到数据库
    //
    // additive 模式的 import 函数按 id 幂等——
    // 新 id 执行导入，已有 id 则更新 settings 和 display name，所以每次
    // 启动都跑是安全的：既保证新装用户开箱可见 live 中的供应商，也让外部
    // 修改的 live 文件能在重启后同步到数据库（与之前依赖前端"导入当前配置"
    // 按钮手动触发不同）。
    //
    // 底层 read_*_config 在文件不存在时返回默认空配置，因此新装且无
    // live 文件的用户走 Ok(0) 路径，不会产生错误日志噪音。
    let mut additive_changed = false;
    let mut additive_errors: Vec<String> = Vec::new();
    {
        use crate::services::provider as p;
        let mut record = |label: &str,
                          res: Result<usize, AppError>,
                          ok_msg: &dyn Fn(usize) -> String,
                          none_msg: &str| {
            match res {
                Ok(count) if count > 0 => {
                    additive_changed = true;
                    log::info!("{}", ok_msg(count));
                }
                Ok(_) => log::debug!("{none_msg}"),
                Err(e) => {
                    log::warn!("✗ Failed to import {label} providers: {e}");
                    additive_errors.push(format!("{label}: {e}"));
                }
            }
        };
        record(
            "OpenCode",
            p::import_opencode_providers_from_live(state),
            &|c| format!("✓ Synced {c} OpenCode provider(s) from live config"),
            "○ No OpenCode provider changes from live config",
        );
        record(
            "OpenClaw",
            p::import_openclaw_providers_from_live(state),
            &|c| format!("✓ Synced {c} OpenClaw provider(s) from live config"),
            "○ No OpenClaw provider changes from live config",
        );
        record(
            "Hermes",
            p::import_hermes_providers_from_live(state),
            &|c| format!("✓ Synced {c} Hermes provider(s) from live config"),
            "○ No Hermes provider changes from live config",
        );
        record(
            "Pi",
            p::import_pi_providers_from_live(state),
            &|c| format!("✓ Synced {c} Pi provider(s) from native config"),
            "○ No Pi provider changes from native config",
        );
        record(
            "Oh My Pi",
            p::import_omp_providers_from_live(state),
            &|c| format!("✓ Synced {c} Oh My Pi provider(s) from native config"),
            "○ No Oh My Pi provider changes from native config",
        );
    }
    report.push(
        "additive_live_import",
        if !additive_errors.is_empty() {
            StepOutcome::Failed(additive_errors.join("; "))
        } else if additive_changed {
            StepOutcome::Done
        } else {
            StepOutcome::Skipped("no provider changes".into())
        },
    );

    // 2. OMO 配置导入（当数据库中无 OMO provider 时，从本地文件导入）
    let has_omo = db
        .get_all_providers("opencode")
        .map(|providers| {
            providers
                .values()
                .any(|p| p.category.as_deref() == Some("omo"))
        })
        .unwrap_or(false);
    if has_omo {
        report.push("omo", StepOutcome::Skipped("omo provider exists".into()));
    } else {
        let outcome = match crate::services::OmoService::import_from_local(
            state,
            &crate::services::omo::STANDARD,
        ) {
            Ok(provider) => {
                log::info!(
                    "✓ Imported OMO config from local as provider '{}'",
                    provider.name
                );
                StepOutcome::Done
            }
            Err(AppError::OmoConfigNotFound) => {
                log::debug!("○ No OMO config to import");
                StepOutcome::Skipped("no OMO config".into())
            }
            Err(e) => {
                log::warn!("✗ Failed to import OMO config from local: {e}");
                StepOutcome::Failed(e.to_string())
            }
        };
        report.push("omo", outcome);
    }

    // 2.3 OMO Slim config import (when no omo-slim provider in DB, import from local)
    let has_omo_slim = db
        .get_all_providers("opencode")
        .map(|providers| {
            providers
                .values()
                .any(|p| p.category.as_deref() == Some("omo-slim"))
        })
        .unwrap_or(false);
    if has_omo_slim {
        report.push(
            "omo_slim",
            StepOutcome::Skipped("omo-slim provider exists".into()),
        );
    } else {
        let outcome = match crate::services::OmoService::import_from_local(
            state,
            &crate::services::omo::SLIM,
        ) {
            Ok(provider) => {
                log::info!(
                    "✓ Imported OMO Slim config from local as provider '{}'",
                    provider.name
                );
                StepOutcome::Done
            }
            Err(AppError::OmoConfigNotFound) => {
                log::debug!("○ No OMO Slim config to import");
                StepOutcome::Skipped("no OMO Slim config".into())
            }
            Err(e) => {
                log::warn!("✗ Failed to import OMO Slim config from local: {e}");
                StepOutcome::Failed(e.to_string())
            }
        };
        report.push("omo_slim", outcome);
    }

    // 3. 导入 MCP 服务器配置（表空时触发）
    if db.is_mcp_table_empty().unwrap_or(false) {
        log::info!("MCP table empty, importing from live configurations...");
        use crate::services::mcp::McpService as M;

        let mut imported = false;
        let mut errors: Vec<String> = Vec::new();
        let mut record = |label: &str, res: Result<usize, AppError>, ok_label: &str| match res {
            Ok(count) if count > 0 => {
                imported = true;
                log::info!("✓ Imported {count} MCP server(s) from {ok_label}");
            }
            Ok(_) => log::debug!("○ No {ok_label} MCP servers found to import"),
            Err(e) => {
                log::warn!("✗ Failed to import {ok_label} MCP: {e}");
                errors.push(format!("{label}: {e}"));
            }
        };
        record("claude", M::import_from_claude(state), "Claude");
        record("codex", M::import_from_codex(state), "Codex");
        record("gemini", M::import_from_gemini(state), "Gemini");
        record("grokbuild", M::import_from_grokbuild(state), "Grok Build");
        record("opencode", M::import_from_opencode(state), "OpenCode");
        record("hermes", M::import_from_hermes(state), "Hermes");

        report.push(
            "mcp_import",
            if !errors.is_empty() {
                StepOutcome::Failed(errors.join("; "))
            } else if imported {
                StepOutcome::Done
            } else {
                StepOutcome::Skipped("no MCP servers found".into())
            },
        );
    } else {
        report.push("mcp_import", StepOutcome::Skipped("table not empty".into()));
    }

    // 4. 导入提示词文件（表空时触发）
    if db.is_prompts_table_empty().unwrap_or(false) {
        log::info!("Prompts table empty, importing from live configurations...");

        let mut imported = false;
        let mut errors: Vec<String> = Vec::new();
        for app in [
            AppType::Claude,
            AppType::Codex,
            AppType::Gemini,
            AppType::GrokBuild,
            AppType::OpenCode,
            AppType::OpenClaw,
            AppType::Hermes,
            AppType::Pi,
            AppType::Mcode,
        ] {
            match crate::services::prompt::PromptService::import_from_file_on_first_launch(
                state,
                app.clone(),
            ) {
                Ok(count) if count > 0 => {
                    imported = true;
                    log::info!("✓ Imported {count} prompt(s) for {}", app.as_str());
                }
                Ok(_) => log::debug!("○ No prompt file found for {}", app.as_str()),
                Err(e) => {
                    log::warn!("✗ Failed to import prompt for {}: {e}", app.as_str());
                    errors.push(format!("{}: {e}", app.as_str()));
                }
            }
        }
        report.push(
            "prompt_import",
            if !errors.is_empty() {
                StepOutcome::Failed(errors.join("; "))
            } else if imported {
                StepOutcome::Done
            } else {
                StepOutcome::Skipped("no prompt files found".into())
            },
        );
    } else {
        report.push(
            "prompt_import",
            StepOutcome::Skipped("table not empty".into()),
        );
    }
}

/// Result of a successful headless bootstrap.
pub struct Bootstrapped {
    pub state: AppState,
    pub report: BootstrapReport,
}

/// [`run_headless_bootstrap_with`] with the Codex history migrations run
/// inline (what tests want: everything is done when this returns).
pub fn run_headless_bootstrap() -> Result<Bootstrapped, BootstrapError> {
    run_headless_bootstrap_with(CodexHistory::Inline)
}

/// Full headless startup: paths, legacy config, version precheck, DB open,
/// log level, legacy migration, `AppState`, post-DB steps. No dialogs, no
/// Tauri runtime. `codex_history` decides where the (slow) Codex history
/// migrations run, see [`CodexHistory`].
///
/// Blocking and may drive `block_on` internally: from async code call it
/// through `tokio::task::spawn_blocking`.
///
/// Errors are fatal pre-DB problems; they are also recorded in `init_status`
/// (so `get_init_error` reports them) before being returned.
pub fn run_headless_bootstrap_with(
    codex_history: CodexHistory<'_>,
) -> Result<Bootstrapped, BootstrapError> {
    let paths = BootstrapPaths::resolve();

    let legacy = if needs_legacy_migration(&paths) {
        Some(load_legacy_config().inspect_err(|e| record_init_error(&paths, e))?)
    } else {
        None
    };
    precheck_db_version(&paths)?;
    let db = open_database(&paths).inspect_err(|e| record_init_error(&paths, e))?;
    // An explicit (non-blank) RUST_LOG wins over the persisted level: the persisted one
    // would otherwise cap `log::max_level` under the env_logger of the server.
    if crate::server_log::explicit_rust_log().is_some() {
        log::info!("RUST_LOG is set, skipping the persisted log level");
    } else {
        apply_persisted_log_level(&db);
    }

    let mut report = BootstrapReport::default();
    if let Some(config) = legacy {
        let outcome = migrate_legacy_config(&db, &paths, &config);
        report.legacy_config_migrated = outcome == StepOutcome::Done;
        report.push("legacy_config_migration", outcome);
    }

    let state = AppState::new(db);
    crate::usage_events::init(state.events.clone());
    run_post_db_bootstrap(&state, codex_history, &mut report);
    Ok(Bootstrapped { state, report })
}

fn record_init_error(paths: &BootstrapPaths, err: &BootstrapError) {
    if matches!(err, BootstrapError::DbVersionTooNew { .. }) {
        return; // already recorded by the precheck, with its structured fields
    }
    init_status::set_init_error(InitErrorPayload {
        path: match err {
            BootstrapError::LegacyConfig(_) => paths.json_path.display().to_string(),
            _ => paths.db_path.display().to_string(),
        },
        error: err.to_string(),
        kind: None,
        db_version: None,
        supported_version: None,
    });
}

/// Periodic maintenance interval (also the auto-backup check cadence).
const PERIODIC_MAINTENANCE_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// Shared periodic-backup worker (desktop `lib.rs` and headless server): one
/// `periodic_backup_if_needed` round at start, then one every 24 h (the first
/// interval tick is skipped, the startup round covers it). The blocking call
/// goes through `spawn_blocking`; failures are only logged.
pub async fn run_periodic_backup_worker(db: Arc<Database>) {
    async fn round(db: &Arc<Database>, what: &str) {
        let db = Arc::clone(db);
        match tokio::task::spawn_blocking(move || db.periodic_backup_if_needed()).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("Periodic backup failed ({what}): {e}"),
            Err(e) => log::warn!("Periodic backup task aborted ({what}): {e}"),
        }
    }

    round(&db, "startup").await;
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        PERIODIC_MAINTENANCE_INTERVAL_SECS,
    ));
    interval.tick().await; // first tick is immediate; startup round already ran
    loop {
        interval.tick().await;
        round(&db, "timer").await;
    }
}

/// Headless entry for [`run_periodic_backup_worker`]. Needs a running tokio
/// runtime; the returned handle lets the caller abort it.
pub fn spawn_periodic_backup_worker(db: Arc<Database>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(run_periodic_backup_worker(db))
}

/// Headless counterpart of the desktop session-usage worker (`lib.rs` setup):
/// one startup round (cost backfill + scan), then a scan every ten minutes.
/// Needs a running tokio runtime; the returned handle lets the caller abort it.
pub fn spawn_session_usage_worker(db: Arc<Database>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(crate::services::session_usage::run_background_worker(db))
}
