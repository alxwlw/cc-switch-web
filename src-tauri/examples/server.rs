//! Headless web-server mode for the cc-switch-web fork.
//!
//! Tailscale-only: there is no in-app auth. The server refuses to bind a
//! non-loopback address unless `TRUSTED_TAILNET=1` is set (fail closed).
//!
//! Env:
//! - `HOST` (default `127.0.0.1`) — bind address, e.g. your Tailscale IP.
//! - `PORT` (default `18766`).
//! - `CC_SWITCH_CONFIG_DIR` (default `~/.cc-switch`) — fork data dir, e.g.
//!   `~/.cc-switch-web` to isolate from upstream/Laliet installs.
//! - `RUST_LOG` — env_logger filter; default `trace` (dependency noise capped),
//!   effective level = saved `LogConfig`. Logs go to stderr and `<config>/logs/cc-switch-web.log`.
//! - `TRUSTED_TAILNET=1` — required for any non-loopback bind.
//! - `CC_SWITCH_WEB_SUPERVISED=1` — tells the server that something restarts it when it
//!   exits (Docker with a restart policy, a unit not run by systemd). systemd sets
//!   `INVOCATION_ID` itself. With either, `POST /api/restart` shuts the server down the way
//!   SIGTERM does and exits with code 75; without, it answers 409.
//! - `CC_SWITCH_WEB_SELF_UPDATE=1` — lets `POST /api/update/install` run `scripts/self-update.sh`
//!   (fast-forward, build, swap, restart, roll back) in a transient systemd user unit. Needs
//!   systemd (`INVOCATION_ID`) and `CC_SWITCH_WEB_SOURCE_DIR` (absolute path of the checkout the
//!   service was installed from); the branch is `CC_SWITCH_WEB_UPDATE_BRANCH`. The unit gets only
//!   `HOST`, `PORT`, `WEB_DIST`, `XDG_STATE_HOME` and `CC_SWITCH_WEB_BUILD_PATH` (directories with
//!   `pnpm`/`node`/`cargo`) from this environment. Anyone who can reach the port can start a
//!   build: enable it only on a Tailscale-only host. `GET /api/update/status` reads the progress.
//! - `WEB_DIST` — directory with the `pnpm build:web` output (`dist-web`).
//!   Defaults to `dist-web` next to the repo root, resolved from the
//!   executable location. When missing, only `/api/*` is served.

use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use cc_switch_lib::{
    bootstrap::{
        run_headless_bootstrap_with, run_headless_startup, shutdown_cleanup, CodexHistory,
    },
    store::AppState,
    web_api::{self, ShutdownHandle},
};
use std::time::Duration;

/// Same bound as the desktop's system-exit cleanup: stopping the proxy has its
/// own 5 s timeout, repointing clients is a few file writes.
const SHUTDOWN_CLEANUP_TIMEOUT: Duration = Duration::from_secs(8);
/// How long in-flight requests get after cleanup. Long-lived `/api/events` SSE
/// streams never end by themselves, so without a bound the drain would hold
/// until systemd's SIGKILL.
const SHUTDOWN_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// What ended the server's life.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShutdownCause {
    /// Ctrl-C or SIGTERM: a plain stop, exit code 0.
    Signal,
    /// `POST /api/restart`: exit with [`web_api::RESTART_EXIT_CODE`] so the supervisor
    /// starts the server again.
    Restart,
}

/// Resolves on Ctrl-C, (unix) SIGTERM — what `systemctl stop` sends — or a restart request.
async fn shutdown_signal(restart: &ShutdownHandle) -> ShutdownCause {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {
            log::info!("Ctrl-C received, shutting down");
            ShutdownCause::Signal
        }
        _ = term => {
            log::info!("SIGTERM received, shutting down");
            ShutdownCause::Signal
        }
        _ = restart.requested() => {
            log::info!("Restart requested, shutting down");
            ShutdownCause::Restart
        }
    }
}

/// Restores the CLI clients and stops the proxy, but never blocks shutdown
/// longer than [`SHUTDOWN_CLEANUP_TIMEOUT`].
async fn bounded_shutdown_cleanup(state: Option<&AppState>) {
    let Some(state) = state else { return };
    if tokio::time::timeout(SHUTDOWN_CLEANUP_TIMEOUT, shutdown_cleanup(state))
        .await
        .is_err()
    {
        log::warn!(
            "Shutdown cleanup did not finish in {} s, exiting anyway",
            SHUTDOWN_CLEANUP_TIMEOUT.as_secs()
        );
    }
}

fn env_truthy(name: &str) -> bool {
    env::var(name).is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
}

fn default_web_dist() -> PathBuf {
    // examples/server lives at <root>/src-tauri/target/<profile>/examples,
    // so dist-web at <root>/dist-web is 4 levels up from the exe dir.
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let mut candidate = dir.to_path_buf();
            for _ in 0..4 {
                if !candidate.pop() {
                    break;
                }
            }
            candidate.push("dist-web");
            return candidate;
        }
    }
    PathBuf::from("dist-web")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The auto-sync workers (and the other `tauri::async_runtime` users in the
    // library) must live on this runtime. The call has to be the very first
    // thing in `main`: it panics once anything has touched the global runtime,
    // and `tests/server_runtime_bootstrap.rs` pins this order.
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    // The logger opens `<config dir>/logs/cc-switch-web.log`, so the headless
    // override of the config dir must be in the resolver's cache before it runs
    // (the bootstrap loads it again: loading is idempotent, and its warnings
    // reach the log there, which does not exist yet here).
    cc_switch_lib::app_store::load_headless_override();

    cc_switch_lib::server_log::init();

    let port: u16 = env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(18766);
    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr: SocketAddr = format!("{host}:{port}").parse().unwrap_or_else(|_| {
        log::warn!("Invalid HOST `{host}`, falling back to 127.0.0.1");
        SocketAddr::from(([127, 0, 0, 1], port))
    });

    if !addr.ip().is_loopback() && !env_truthy("TRUSTED_TAILNET") {
        log::error!(
            "Refusing to bind non-loopback {addr} without TRUSTED_TAILNET=1. \
             This server has no in-app auth and must only run inside your Tailnet."
        );
        std::process::exit(1);
    }

    // Same startup bootstrap as the desktop `setup` hook (legacy config
    // migration, seeds, first-run imports, crash recovery). It is blocking and
    // may reach `block_on`, so it runs on the blocking pool, not on this
    // runtime thread. The Codex history migrations go to the blocking pool too
    // so a large history does not delay the listener.
    let bootstrapped = tokio::task::spawn_blocking(|| {
        run_headless_bootstrap_with(CodexHistory::Spawn(&|job| {
            tokio::task::spawn_blocking(job);
        }))
    })
    .await?;
    let mut cleanup_state: Option<Arc<AppState>> = None;
    let restart = ShutdownHandle::new();
    let api = match bootstrapped {
        Ok(boot) => {
            for (step, err) in boot.report.failures() {
                log::warn!("startup step `{step}` failed: {err}");
            }
            let state = Arc::new(boot.state);
            // Same background session-usage scan as the desktop app.
            cc_switch_lib::bootstrap::spawn_session_usage_worker(state.db.clone());
            // Same periodic DB backup (startup check + daily) as the desktop app.
            cc_switch_lib::bootstrap::spawn_periodic_backup_worker(state.db.clone());
            // Settle direct/proxy modes and re-attach proxied clients (crash
            // recovery for takeover) before the first request is served.
            run_headless_startup(&state).await;
            cleanup_state = Some(state.clone());
            web_api::router_with_update(
                state,
                Some(restart.clone()),
                Some(Arc::new(web_api::UpdateRunner::new(Arc::new(
                    web_api::SystemdRunSpawner::default(),
                )))),
            )
        }
        Err(e) => {
            // init_status already holds the error; keep serving so the console
            // can show the upgrade / config-error screen.
            log::error!("Startup bootstrap failed, serving in init-error mode: {e}");
            web_api::init_error_router()
        }
    };

    let web_dist = env::var("WEB_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_web_dist());
    let app = if web_dist.join("index.html").exists() {
        log::info!("Serving console from {}", web_dist.display());
        let index = web_dist.join("index.html");
        api.fallback_service(
            tower_http::services::ServeDir::new(&web_dist)
                .append_index_html_on_directories(true)
                .not_found_service(tower::service_fn(
                    move |_req: axum::http::Request<axum::body::Body>| {
                        let index = index.clone();
                        async move {
                            let (status, body) = match tokio::fs::read(&index).await {
                                Ok(html) => (200u16, html),
                                Err(_) => (500u16, b"console unavailable".to_vec()),
                            };
                            Ok::<_, std::convert::Infallible>(
                                axum::http::Response::builder()
                                    .status(status)
                                    .header("content-type", "text/html")
                                    .body(axum::body::Body::from(body))
                                    .unwrap_or_default(),
                            )
                        }
                    },
                )),
        )
    } else {
        log::info!(
            "Console dist not found at {} — serving API only. Run `pnpm build:web` first.",
            web_dist.display()
        );
        api
    };

    log::info!("Starting cc-switch-web server on http://{addr} (tailscale-only, no in-app auth)");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let (drain_tx, drain_rx) = tokio::sync::oneshot::channel::<()>();
    // Set before the drain timer can fire, read after `select!` by both of its branches.
    let exit_for_restart = Arc::new(AtomicBool::new(false));
    let exit_for_restart_flag = exit_for_restart.clone();
    let serve = axum::serve(listener, app.into_make_service()).with_graceful_shutdown(async move {
        if shutdown_signal(&restart).await == ShutdownCause::Restart {
            exit_for_restart_flag.store(true, Ordering::SeqCst);
        }
        // Cleanup runs even if the drain below hangs on a long-lived stream.
        bounded_shutdown_cleanup(cleanup_state.as_deref()).await;
        let _ = drain_tx.send(());
    });
    tokio::select! {
        res = serve => res?,
        _ = async {
            if drain_rx.await.is_ok() {
                tokio::time::sleep(SHUTDOWN_DRAIN_TIMEOUT).await;
            } else {
                std::future::pending::<()>().await;
            }
        } => log::warn!(
            "Open connections did not drain in {} s, exiting anyway",
            SHUTDOWN_DRAIN_TIMEOUT.as_secs()
        ),
    }
    if exit_for_restart.load(Ordering::SeqCst) {
        log::info!(
            "Exiting with code {} for the supervisor to restart the server",
            web_api::RESTART_EXIT_CODE
        );
        log::logger().flush();
        std::process::exit(web_api::RESTART_EXIT_CODE);
    }
    Ok(())
}
