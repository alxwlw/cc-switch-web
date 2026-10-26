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
