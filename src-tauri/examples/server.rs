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
//! - `WEB_DIST` — directory with the `pnpm build:web` output (`dist-web`).
//!   Defaults to `dist-web` next to the repo root, resolved from the
//!   executable location. When missing, only `/api/*` is served.

use std::{env, net::SocketAddr, path::PathBuf, sync::Arc};

use cc_switch_lib::{store::AppState, web_api, Database};

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
    let api = web_api::router(state);

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
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}
