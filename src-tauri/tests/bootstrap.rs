//! Smoke test for the shared headless bootstrap (WEB-32 Task 1). The full
//! first-run / upgrade / version-too-new suite is added in Task 3.

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
