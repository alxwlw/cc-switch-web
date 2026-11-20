//! WEB-26: `POST /api/restart` support — the shutdown handle, the supervisor check and the
//! per-process instance id.
//!
//! A restart is "exit with [`RESTART_EXIT_CODE`] and let the supervisor start the server
//! again". The route never kills the process: it asks through a [`ShutdownHandle`] that
//! `examples/server.rs` also listens on next to SIGTERM, so the restart takes the same road
//! out as `systemctl stop` (cleanup of the CLI clients, bounded drain) and only `main` calls
//! `process::exit`. A test injects its own handle and watches the signal.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use tokio::sync::watch;

/// `EX_TEMPFAIL`: not 0 and not a crash, so both `Restart=always` and `Restart=on-failure`
/// start the unit again, and Docker's `unless-stopped` / `on-failure` policies do too.
pub const RESTART_EXIT_CODE: i32 = 75;

/// How long the route waits before it raises the signal, so the 202 is already on its way
/// to the socket when the graceful shutdown starts to close connections.
pub const RESTART_SIGNAL_DELAY: Duration = Duration::from_millis(100);

/// Set by systemd for every unit it starts.
const SYSTEMD_ENV: &str = "INVOCATION_ID";
/// For Docker and every other supervisor: the operator promises a restart policy.
const SUPERVISED_ENV: &str = "CC_SWITCH_WEB_SUPERVISED";

/// Whether something will start this process again after it exits: systemd
/// (`INVOCATION_ID`, non-empty) or the operator's word (`CC_SWITCH_WEB_SUPERVISED=1`).
/// Read at call time, so the answer follows the environment the process was started with.
pub fn is_supervised() -> bool {
    std::env::var_os(SYSTEMD_ENV).is_some_and(|v| !v.is_empty())
        || std::env::var(SUPERVISED_ENV).is_ok_and(|v| v == "1")
}

/// Random per process and stable inside it. `/api/health` and the 202 of `POST /api/restart`
/// carry it, so a client that polls after the restart can tell the new process from the old
/// one, which keeps answering until its drain ends.
pub fn instance_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

/// Latched "a restart was asked for". Cloning shares the latch; raising it twice is the
/// same as once, so a second click during the shutdown changes nothing.
#[derive(Clone)]
pub struct ShutdownHandle {
    tx: Arc<watch::Sender<bool>>,
}

impl Default for ShutdownHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ShutdownHandle {
    pub fn new() -> Self {
        Self {
            tx: Arc::new(watch::channel(false).0),
        }
    }

    /// Raises the latch now.
    pub fn request(&self) {
        self.tx.send_replace(true);
    }

    /// Raises the latch after [`RESTART_SIGNAL_DELAY`], from a spawned task (needs a runtime).
    pub fn request_after_response(&self) {
        let handle = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(RESTART_SIGNAL_DELAY).await;
            handle.request();
        });
    }

    pub fn is_requested(&self) -> bool {
        *self.tx.borrow()
    }

    /// Resolves once the latch is raised (immediately when it already is).
    pub async fn requested(&self) {
        let mut rx = self.tx.subscribe();
        // The sender lives in `self`, so the channel cannot close under the wait.
        let _ = rx.wait_for(|raised| *raised).await;
    }
}

/// What the router layer carries: the handle of the process that serves it, or nothing
/// (`router(state)` as the tests and the init-error mode build it).
#[derive(Clone)]
pub(super) struct RestartControl(pub Option<ShutdownHandle>);
