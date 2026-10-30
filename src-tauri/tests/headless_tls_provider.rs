//! Own test binary (own process): the headless bootstrap must install the
//! rustls `CryptoProvider` itself, as the desktop `setup` hook does. Keep this
//! the only test here, so nothing else installs a provider first.

mod support;

use cc_switch_lib::bootstrap::run_headless_bootstrap;
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[test]
fn headless_bootstrap_installs_rustls_provider() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    assert!(
        rustls::crypto::CryptoProvider::get_default().is_none(),
        "precondition: no provider before the bootstrap"
    );

    run_headless_bootstrap().expect("headless bootstrap");

    assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    // Would panic without a process default provider.
    let _ = rustls::ClientConfig::builder();
}
