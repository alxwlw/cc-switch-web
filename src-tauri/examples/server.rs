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
//! - `TRUSTED_TAILNET=1` — required for any non-loopback bind.

use std::{env, net::SocketAddr, sync::Arc};

use cc_switch_lib::{store::AppState, web_api, Database};

fn env_truthy(name: &str) -> bool {
    env::var(name).is_ok_and(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

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

    let db = Arc::new(Database::init()?);
    let state: web_api::SharedState = Arc::new(AppState::new(db));
    let app = web_api::router(state);

    log::info!("Starting cc-switch-web server on http://{addr} (tailscale-only, no in-app auth)");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}
