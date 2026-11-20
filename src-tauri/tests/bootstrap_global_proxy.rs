//! Headless bootstrap initialises the global outbound HTTP client (WEB-23).
//! Own test binary: the `http_client` globals are process-wide OnceCells with
//! no reset. Both tests leave the client in a state the other overwrites.

mod support;

use cc_switch_lib::bootstrap::{run_headless_bootstrap, StepOutcome};
use cc_switch_lib::{get_upstream_proxy_status, Database};
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, test_mutex};

fn seed_proxy(url: &str) {
    let db = Database::init().expect("seed db");
    db.set_global_proxy_url(Some(url)).expect("seed proxy");
}

#[test]
#[serial]
fn saved_proxy_is_applied_to_the_client() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    seed_proxy("http://127.0.0.1:18555");

    let boot = run_headless_bootstrap().expect("bootstrap");

    let status = get_upstream_proxy_status();
    assert!(status.enabled);
    assert_eq!(status.proxy_url.as_deref(), Some("http://127.0.0.1:18555"));
    assert_eq!(
        boot.report.step("global_http_client"),
        Some(&StepOutcome::Done),
        "{:?}",
        boot.report
    );
    assert_eq!(
        boot.state.db.get_global_proxy_url().expect("db"),
        Some("http://127.0.0.1:18555".to_string())
    );
}

#[test]
#[serial]
fn broken_saved_proxy_is_cleared_and_client_goes_direct() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    // Unsupported scheme: build_client rejects it, Database does not validate.
    seed_proxy("ftp://127.0.0.1:18555");

    let boot = run_headless_bootstrap().expect("bootstrap must survive a broken proxy");

    assert_eq!(boot.state.db.get_global_proxy_url().expect("db"), None);
    let status = get_upstream_proxy_status();
    assert!(!status.enabled);
    assert_eq!(status.proxy_url, None);
    assert_eq!(
        boot.report.step("global_http_client"),
        Some(&StepOutcome::Done),
        "{:?}",
        boot.report
    );
}
