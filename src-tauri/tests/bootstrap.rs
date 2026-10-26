//! Shared headless bootstrap (WEB-32): first run, legacy upgrade, crash
//! recovery. Version-too-new lives in `web_api_init_status.rs`: it leaves an
//! init error in the process-global `init_status`, which no test in this
//! binary may see.

mod support;

use cc_switch_lib::bootstrap::{run_headless_bootstrap, StepOutcome};
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn first_run_creates_db_and_seeds_defaults() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();

    let boot = run_headless_bootstrap().expect("bootstrap on empty HOME");

    assert!(home.join(".cc-switch").join("cc-switch.db").exists());
    assert!(!boot.state.db.get_skill_repos().expect("repos").is_empty());
    assert_eq!(
        boot.report.step("skill_repos"),
        Some(&StepOutcome::Done),
        "{:?}",
        boot.report
    );
    assert!(boot.report.fresh_install);
    assert!(boot.report.failures().is_empty(), "{:?}", boot.report);
    assert!(!boot.report.legacy_config_migrated);
    assert_eq!(
        boot.report.step("official_providers"),
        Some(&StepOutcome::Done),
        "{:?}",
        boot.report
    );
    assert_eq!(
        boot.state
            .db
            .get_setting("official_providers_seeded")
            .expect("flag"),
        Some("true".to_string())
    );
    for app in ["claude", "codex", "gemini"] {
        let providers = boot.state.db.get_all_providers(app).expect("providers");
        assert!(!providers.is_empty(), "{app}: official providers seeded");
    }

    // Second run on the same HOME is idempotent: seeds are skipped, not duplicated.
    let repos = boot.state.db.get_skill_repos().expect("repos").len();
    drop(boot);
    let again = run_headless_bootstrap().expect("second bootstrap");
    assert_eq!(
        again.state.db.get_skill_repos().expect("repos").len(),
        repos
    );
    assert!(matches!(
        again.report.step("skill_repos"),
        Some(StepOutcome::Skipped(_))
    ));
}

#[test]
fn legacy_config_retry_keeps_the_decision_taken_before_the_first_attempt() {
    use cc_switch_lib::bootstrap::{load_legacy_config, needs_legacy_migration, BootstrapPaths};

    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let paths = BootstrapPaths::resolve();
    std::fs::create_dir_all(&paths.app_config_dir).expect("config dir");
    std::fs::write(&paths.json_path, "{ not json").expect("write invalid config");

    assert!(needs_legacy_migration(&paths));
    assert!(load_legacy_config().is_err());

    // config.json disappears while the error dialog is open; the retry still
    // loads (old setup: default config), it does not silently skip migration.
    std::fs::remove_file(&paths.json_path).expect("remove config");
    let retried = load_legacy_config().expect("retry loads a default config");
    assert_eq!(retried.mcp.claude.servers.len(), 0);
}

const LEGACY_CONFIG: &str = r#"{
  "version": 2,
  "claude": {
    "providers": {
      "legacy-a": {"id": "legacy-a", "name": "Legacy A", "settingsConfig": {"env": {"ANTHROPIC_AUTH_TOKEN": "sk-a", "ANTHROPIC_BASE_URL": "https://a.invalid"}}},
      "legacy-b": {"id": "legacy-b", "name": "Legacy B", "settingsConfig": {"env": {"ANTHROPIC_AUTH_TOKEN": "sk-b", "ANTHROPIC_BASE_URL": "https://b.invalid"}}}
    },
    "current": "legacy-a"
  },
  "codex": {"providers": {}, "current": ""}
}"#;

#[tokio::test]
async fn legacy_config_is_migrated_once_and_archived() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let dir = home.join(".cc-switch");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::write(dir.join("config.json"), LEGACY_CONFIG).expect("write legacy config");

    let boot = run_headless_bootstrap().expect("bootstrap with legacy config");

    assert_eq!(
        boot.report.step("legacy_config_migration"),
        Some(&StepOutcome::Done),
        "{:?}",
        boot.report
    );
    assert!(boot.report.legacy_config_migrated);
    assert!(
        !boot.report.fresh_install,
        "migrated providers are not a fresh install"
    );
    let providers = boot
        .state
        .db
        .get_all_providers("claude")
        .expect("providers");
    assert_eq!(providers["legacy-a"].name, "Legacy A");
    assert_eq!(providers["legacy-b"].name, "Legacy B");
    assert_eq!(
        boot.state
            .db
            .get_current_provider("claude")
            .expect("current"),
        Some("legacy-a".to_string())
    );
    assert!(!dir.join("config.json").exists());
    // `MultiAppConfig::load` normalizes the file before the archive rename, so
    // compare content, not bytes.
    let archived: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("config.json.migrated")).expect("archive"))
            .expect("archive json");
    assert_eq!(
        archived["claude"]["providers"]["legacy-b"]["name"],
        serde_json::json!("Legacy B")
    );
    let count = providers.len();

    // The success flag is reported once.
    let router = support::web_router();
    let (_, body) = support::web_call(&router, "GET", "/api/migrations/status", None).await;
    assert_eq!(body["data"]["migration"], serde_json::json!(true));
    let (_, body) = support::web_call(&router, "GET", "/api/migrations/status", None).await;
    assert_eq!(body["data"]["migration"], serde_json::json!(false));
    drop(boot);

    // A second start does not migrate again and does not duplicate providers.
    let again = run_headless_bootstrap().expect("second bootstrap");
    assert_eq!(again.report.step("legacy_config_migration"), None);
    assert!(!again.report.legacy_config_migrated);
    assert_eq!(
        again
            .state
            .db
            .get_all_providers("claude")
            .expect("providers")
            .len(),
        count
    );
    let (_, body) = support::web_call(&router, "GET", "/api/migrations/status", None).await;
    assert_eq!(body["data"]["migration"], serde_json::json!(false));
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A crash after the publish marker was recorded but before the file was
/// swapped: `live-state.json` holds the intent, the staged file holds the new
/// content. The bootstrap must roll it forward (`recover_on_startup`).
#[test]
fn bootstrap_completes_a_half_written_client_file_from_live_state() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let dir = home.join(".cc-switch");
    std::fs::create_dir_all(&dir).expect("config dir");
    std::fs::create_dir_all(home.join(".claude")).expect("claude dir");
    let target = home.join(".claude").join("settings.json");
    let staged = dir.join("settings.json.staged");
    let old = br#"{"env":{"ANTHROPIC_BASE_URL":"https://old.invalid"}}"#;
    let new = br#"{"env":{"ANTHROPIC_BASE_URL":"https://new.invalid"}}"#;
    std::fs::write(&target, old).expect("old live file");
    std::fs::write(&staged, new).expect("staged file");
    let intent = serde_json::json!({
        "version": 1,
        "apps": {"claude": {"pending": {
            "op": "switch",
            "published": true,
            "files": [{
                "path": target,
                "pre": sha256_hex(old),
                "planned": sha256_hex(new),
                "staged": staged,
            }],
            "target": {},
        }}},
    });
    std::fs::write(dir.join("live-state.json"), intent.to_string()).expect("live-state.json");

    let boot = run_headless_bootstrap().expect("bootstrap");

    assert_eq!(
        boot.report.step("recover_on_startup"),
        Some(&StepOutcome::Done)
    );
    assert_eq!(std::fs::read(&target).expect("live file"), new.to_vec());
    assert!(!staged.exists(), "the staged file was swapped in");
    let state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("live-state.json")).expect("state"))
            .expect("state json");
    assert!(
        state
            .pointer("/apps/claude/pending")
            .is_none_or(|p| p.is_null()),
        "intent cleared: {state}"
    );
}
