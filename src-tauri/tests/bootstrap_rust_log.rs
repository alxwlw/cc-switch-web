//! An explicit RUST_LOG must survive the headless bootstrap: the persisted log
//! level is only applied when RUST_LOG is unset (WEB-32). Own test binary: it
//! sets a process-wide env var and the global `log` max level.

mod support;

use cc_switch_lib::bootstrap::run_headless_bootstrap;
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn rust_log_is_not_capped_by_the_persisted_log_level() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    std::env::set_var("RUST_LOG", "debug");
    log::set_max_level(log::LevelFilter::Debug);

    // Default persisted config is `info`: applying it would lower the max level.
    run_headless_bootstrap().expect("bootstrap");
    assert_eq!(log::max_level(), log::LevelFilter::Debug);

    // A blank RUST_LOG is unset: the persisted level applies.
    std::env::set_var("RUST_LOG", "  ");
    log::set_max_level(log::LevelFilter::Debug);
    run_headless_bootstrap().expect("bootstrap");
    assert_eq!(log::max_level(), log::LevelFilter::Info);

    std::env::remove_var("RUST_LOG");
    log::set_max_level(log::LevelFilter::Debug);
    run_headless_bootstrap().expect("bootstrap");
    assert_eq!(log::max_level(), log::LevelFilter::Info);
}
