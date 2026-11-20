//! `GET /api/update/check` in release mode (`web_update::check` with a build version and
//! no commit): the stub plays `/repos/o/r/releases/latest`; the compare and commits routes
//! answer 500 and are counted, so a release check that strays to them fails loudly. The
//! checkout mode is in `web_api_update.rs`.
#![cfg(not(target_os = "windows"))]

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use cc_switch_lib::{check_web_update, UpdateCheck, UpdateSourceConfig, WebVersion};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Reply {
    status: StatusCode,
    body: String,
}

impl Reply {
    fn json(status: StatusCode, body: Value) -> Self {
        Self {
            status,
            body: body.to_string(),
        }
    }
}

#[derive(Default)]
struct Hits {
    release: AtomicUsize,
    compare: AtomicUsize,
    head: AtomicUsize,
}

struct Shared {
    release: Mutex<Reply>,
    hits: Hits,
}

type Answer = (StatusCode, HeaderMap, String);

fn respond(reply: Reply) -> Answer {
    let mut headers = HeaderMap::new();
    headers.insert("content-type", "application/json".parse().unwrap());
    (reply.status, headers, reply.body)
}

/// Like the real API, a request without `User-Agent` is a 403.
fn require_user_agent(headers: &HeaderMap) -> Option<Reply> {
    headers
        .get("user-agent")
        .is_none()
        .then(|| Reply::json(StatusCode::FORBIDDEN, json!({"message": "Request forbidden by administrative rules. Please make sure your request has a User-Agent header"})))
}

async fn release_route(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Answer {
    shared.hits.release.fetch_add(1, Ordering::SeqCst);
    respond(require_user_agent(&headers).unwrap_or_else(|| shared.release.lock().unwrap().clone()))
}

async fn compare_route(State(shared): State<Arc<Shared>>) -> Answer {
    shared.hits.compare.fetch_add(1, Ordering::SeqCst);
    respond(Reply::json(StatusCode::INTERNAL_SERVER_ERROR, json!({})))
}

async fn head_route(State(shared): State<Arc<Shared>>) -> Answer {
    shared.hits.head.fetch_add(1, Ordering::SeqCst);
    respond(Reply::json(StatusCode::INTERNAL_SERVER_ERROR, json!({})))
}

/// GitHub stand-in whose latest release answers `release`.
struct Github {
    url: String,
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

impl Github {
    async fn start_release(release: Reply) -> Self {
        let shared = Arc::new(Shared {
            release: Mutex::new(release),
            hits: Hits::default(),
        });
        let app = axum::Router::new()
            .route(
                "/repos/o/r/releases/latest",
                axum::routing::get(release_route),
            )
            .route(
                "/repos/o/r/compare/:spec",
                axum::routing::get(compare_route),
            )
            .route("/repos/o/r/commits/main", axum::routing::get(head_route))
            .fallback(|| async { StatusCode::NOT_FOUND })
            .with_state(shared.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, shared, task }
    }

    fn cfg(&self) -> UpdateSourceConfig {
        UpdateSourceConfig {
            api_base: self.url.clone(),
            repo: "o/r".to_string(),
            branch: "main".to_string(),
        }
    }

    fn release_hits(&self) -> usize {
        self.shared.hits.release.load(Ordering::SeqCst)
    }

    fn compare_hits(&self) -> usize {
        self.shared.hits.compare.load(Ordering::SeqCst)
    }

    fn head_hits(&self) -> usize {
        self.shared.hits.head.load(Ordering::SeqCst)
    }
}

impl Drop for Github {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A 40-hex sha, distinct per `n`.
fn sha_of(n: u32) -> String {
    format!("{n:040x}")
}

fn ok(body: Value) -> Reply {
    Reply::json(StatusCode::OK, body)
}

fn as_json(check: &UpdateCheck) -> Value {
    serde_json::to_value(check).unwrap()
}

const RELEASE_URL: &str = "https://github.com/o/r/releases/tag/v4.0.35-web.2";

fn version(s: &str) -> Option<WebVersion> {
    Some(WebVersion::parse(s).expect("test version"))
}

fn release(tag: &str) -> Reply {
    ok(json!({
        "tag_name": tag,
        "html_url": RELEASE_URL,
        "published_at": "2026-11-20T09:00:00Z",
    }))
}

async fn run_release(gh: &Github, current: &str, force: bool) -> UpdateCheck {
    check_web_update(&gh.cfg(), None, version(current), force)
        .await
        .expect("release check")
}

#[tokio::test]
#[serial]
async fn a_newer_release_is_behind_with_its_version_and_notes_url() {
    let gh = Github::start_release(release("v4.0.35-web.2")).await;
    let result = as_json(&run_release(&gh, "4.0.35-web.1", false).await);

    assert_eq!(result["status"], json!("behind"));
    assert_eq!(result["mode"], json!("release"));
    assert_eq!(result["currentVersion"], json!("4.0.35-web.1"));
    assert_eq!(result["latestVersion"], json!("4.0.35-web.2"));
    assert_eq!(result["releaseUrl"], json!(RELEASE_URL));
    assert_eq!(result["publishedAt"], json!("2026-11-20T09:00:00Z"));
    assert_eq!(result["behindBy"], json!(0));
    assert_eq!(result["commits"], json!([]));
    assert_eq!(result["latestSha"], Value::Null);
    assert_eq!(result["currentSha"], Value::Null);
}

#[tokio::test]
#[serial]
async fn a_release_build_keeps_its_commit_as_current_sha() {
    let sha = sha_of(0x2001);
    let gh = Github::start_release(release("v4.0.35-web.2")).await;
    let result = as_json(
        &check_web_update(&gh.cfg(), Some(&sha), version("4.0.35-web.1"), false)
            .await
            .unwrap(),
    );
    assert_eq!(result["currentSha"], json!(sha));
}

#[tokio::test]
#[serial]
async fn the_installed_release_is_up_to_date() {
    let gh = Github::start_release(release("v4.0.35-web.1")).await;
    let result = as_json(&run_release(&gh, "4.0.35-web.1", false).await);
    assert_eq!(result["status"], json!("up-to-date"));
    assert_eq!(result["latestVersion"], json!("4.0.35-web.1"));
}

#[tokio::test]
#[serial]
async fn a_build_newer_than_the_latest_release_is_unknown() {
    let gh = Github::start_release(release("v4.0.35-web.3")).await;
    let result = as_json(&run_release(&gh, "4.0.36-web.1", false).await);
    assert_eq!(result["status"], json!("unknown"));
    assert_eq!(result["mode"], json!("release"));
    assert_eq!(result["currentVersion"], json!("4.0.36-web.1"));
}

#[tokio::test]
#[serial]
async fn no_release_yet_is_unknown() {
    let gh = Github::start_release(Reply::json(
        StatusCode::NOT_FOUND,
        json!({"message": "Not Found"}),
    ))
    .await;
    let result = as_json(&run_release(&gh, "4.0.35-web.1", false).await);
    assert_eq!(result["status"], json!("unknown"));
    assert_eq!(result["mode"], json!("release"));
    assert_eq!(result["latestVersion"], Value::Null);
    assert_eq!(result["releaseUrl"], Value::Null);
}

#[tokio::test]
#[serial]
async fn a_latest_tag_that_is_not_a_fork_version_is_unknown() {
    let gh = Github::start_release(release("v4.0.36")).await;
    let result = as_json(&run_release(&gh, "4.0.35-web.1", false).await);
    assert_eq!(result["status"], json!("unknown"));
    assert_eq!(result["latestVersion"], Value::Null);
    assert_eq!(result["releaseUrl"], Value::Null);
}

#[tokio::test]
#[serial]
async fn a_release_build_never_asks_the_compare_api() {
    let sha = sha_of(0x2002);
    let gh = Github::start_release(release("v4.0.35-web.2")).await;
    let _ = check_web_update(&gh.cfg(), Some(&sha), version("4.0.35-web.1"), false)
        .await
        .unwrap();
    assert_eq!(gh.compare_hits(), 0);
    assert_eq!(gh.head_hits(), 0);
    assert_eq!(gh.release_hits(), 1);
}

#[tokio::test]
#[serial]
async fn a_release_answer_is_cached_and_force_asks_again() {
    let gh = Github::start_release(release("v4.0.35-web.2")).await;
    let _ = run_release(&gh, "4.0.35-web.1", false).await;
    let _ = run_release(&gh, "4.0.35-web.1", false).await;
    assert_eq!(gh.release_hits(), 1);
    let _ = run_release(&gh, "4.0.35-web.1", true).await;
    assert_eq!(gh.release_hits(), 2);
}

#[tokio::test]
#[serial]
async fn a_cached_release_answer_is_not_served_to_another_version() {
    let gh = Github::start_release(release("v4.0.35-web.2")).await;
    let first = as_json(&run_release(&gh, "4.0.35-web.1", false).await);
    let second = as_json(&run_release(&gh, "4.0.35-web.2", false).await);
    assert_eq!(first["status"], json!("behind"));
    assert_eq!(second["status"], json!("up-to-date"));
    assert_eq!(gh.release_hits(), 2);
}

#[tokio::test]
#[serial]
async fn a_rate_limited_release_check_is_an_error_and_not_cached() {
    let gh = Github::start_release(Reply::json(
        StatusCode::FORBIDDEN,
        json!({"message": "API rate limit exceeded"}),
    ))
    .await;
    let err = check_web_update(&gh.cfg(), None, version("4.0.35-web.1"), false)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("rate limit"), "{err}");
    assert!(
        check_web_update(&gh.cfg(), None, version("4.0.35-web.1"), false)
            .await
            .is_err()
    );
    assert_eq!(gh.release_hits(), 2, "an error is not cached");
}

#[tokio::test]
#[serial]
async fn release_mode_instructions_rerun_the_installer_and_pull_the_image() {
    let gh = Github::start_release(release("v4.0.35-web.2")).await;
    let result = as_json(&run_release(&gh, "4.0.35-web.1", false).await);
    assert_eq!(
        result["instructions"]["systemd"],
        json!("curl -fsSL https://github.com/o/r/releases/latest/download/install.sh | bash")
    );
    assert_eq!(
        result["instructions"]["docker"],
        json!("docker compose pull && docker compose up -d")
    );
}
