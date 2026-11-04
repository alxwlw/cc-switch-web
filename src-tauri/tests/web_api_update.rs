//! `GET /api/update/check` and `web_update::check` (WEB-28 T2).
//!
//! The stub plays the GitHub compare API. The test binary is built without
//! `CC_SWITCH_WEB_GIT_SHA`, so the statuses are exercised by calling the service with a
//! build commit; the route is pinned for the "unknown build" path. Like the real API the
//! stub answers 403 to a request without `User-Agent`, and counts ONLY the expected
//! method + path (a local prober sends `GET /` to loopback listeners).
#![cfg(not(target_os = "windows"))]

mod support;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use cc_switch_lib::{check_web_update, AppState, UpdateCheck, UpdateSourceConfig};
use serde_json::{json, Value};
use serial_test::serial;
use std::ffi::OsString;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex, web_call};

/// Save/restore one process-global env var. Precedent: web_api_usage_live.rs.
struct EnvGuard {
    key: &'static str,
    prev: Option<OsString>,
}

impl EnvGuard {
    fn set(key: &'static str, val: &str) -> Self {
        let prev = std::env::var_os(key);
        std::env::set_var(key, val);
        Self { key, prev }
    }

    fn unset(key: &'static str) -> Self {
        let prev = std::env::var_os(key);
        std::env::remove_var(key);
        Self { key, prev }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

#[derive(Clone)]
struct Reply {
    status: StatusCode,
    headers: Vec<(&'static str, String)>,
    body: String,
}

impl Reply {
    fn json(status: StatusCode, body: Value) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.to_string(),
        }
    }
}

#[derive(Default)]
struct Seen {
    compare_specs: Mutex<Vec<String>>,
    user_agents: Mutex<Vec<String>>,
    accepts: Mutex<Vec<String>>,
}

struct Shared {
    compare: Mutex<Reply>,
    head: Reply,
    delay: Duration,
    compare_hits: Arc<AtomicUsize>,
    head_hits: Arc<AtomicUsize>,
    seen: Arc<Seen>,
}

/// GitHub stand-in for `/repos/o/r/compare/:spec` and `/repos/o/r/commits/main`.
struct Github {
    url: String,
    compare_hits: Arc<AtomicUsize>,
    head_hits: Arc<AtomicUsize>,
    seen: Arc<Seen>,
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

fn require_user_agent(headers: &HeaderMap) -> Option<Reply> {
    headers
        .get("user-agent")
        .is_none()
        .then(|| Reply::json(StatusCode::FORBIDDEN, json!({"message": "Request forbidden by administrative rules. Please make sure your request has a User-Agent header"})))
}

type Answer = (StatusCode, HeaderMap, String);

fn respond(reply: Reply) -> Answer {
    let mut headers = HeaderMap::new();
    for (name, value) in reply.headers {
        headers.insert(name, value.parse().unwrap());
    }
    headers.insert("content-type", "application/json".parse().unwrap());
    (reply.status, headers, reply.body)
}

async fn compare_route(
    State(shared): State<Arc<Shared>>,
    Path(spec): Path<String>,
    headers: HeaderMap,
) -> Answer {
    shared.compare_hits.fetch_add(1, Ordering::SeqCst);
    shared.seen.compare_specs.lock().unwrap().push(spec);
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    if let Some(ua) = header("user-agent") {
        shared.seen.user_agents.lock().unwrap().push(ua);
    }
    if let Some(accept) = header("accept") {
        shared.seen.accepts.lock().unwrap().push(accept);
    }
    // Overlap window for the parallel-requests test; the assertion is on the hit count.
    tokio::time::sleep(shared.delay).await;
    respond(require_user_agent(&headers).unwrap_or_else(|| shared.compare.lock().unwrap().clone()))
}

async fn head_route(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Answer {
    shared.head_hits.fetch_add(1, Ordering::SeqCst);
    respond(require_user_agent(&headers).unwrap_or_else(|| shared.head.clone()))
}

impl Github {
    async fn start(compare: Reply) -> Self {
        Self::start_with(
            compare,
            Reply::json(StatusCode::OK, json!({"sha": sha_of(9999)})),
            Duration::ZERO,
        )
        .await
    }

    async fn start_with(compare: Reply, head: Reply, delay: Duration) -> Self {
        let compare_hits = Arc::new(AtomicUsize::new(0));
        let head_hits = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Seen::default());
        let shared = Arc::new(Shared {
            compare: Mutex::new(compare),
            head,
            delay,
            compare_hits: compare_hits.clone(),
            head_hits: head_hits.clone(),
            seen: seen.clone(),
        });
        let app = axum::Router::new()
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
        Self {
            url,
            compare_hits,
            head_hits,
            seen,
            shared,
            task,
        }
    }

    /// The source answers `compare` with `reply` from now on.
    fn set_compare(&self, reply: Reply) {
        *self.shared.compare.lock().unwrap() = reply;
    }

    fn cfg(&self) -> UpdateSourceConfig {
        UpdateSourceConfig {
            api_base: self.url.clone(),
            repo: "o/r".to_string(),
            branch: "main".to_string(),
        }
    }

    fn compare_hits(&self) -> usize {
        self.compare_hits.load(Ordering::SeqCst)
    }

    fn head_hits(&self) -> usize {
        self.head_hits.load(Ordering::SeqCst)
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

/// Compare body: `listed` commits, oldest first (`from`, `from + 1`, …), each with a
/// multi-line message so the first-line rule is visible.
fn compare_body(ahead_by: u32, behind_by: u32, from: u32, listed: u32) -> Value {
    let commits: Vec<Value> = (from..from + listed)
        .map(|n| {
            json!({
                "sha": sha_of(n),
                "commit": {
                    "message": format!("subject {n}\n\nbody of {n}\nmore"),
                    "committer": {"date": format!("2026-11-{:02}T10:00:00Z", 1 + n % 28)},
                    "author": {"date": "1999-01-01T00:00:00Z"},
                },
            })
        })
        .collect();
    json!({
        "status": "ahead",
        "ahead_by": ahead_by,
        "behind_by": behind_by,
        "total_commits": ahead_by,
        "commits": commits,
    })
}

fn ok(body: Value) -> Reply {
    Reply::json(StatusCode::OK, body)
}

async fn run(gh: &Github, sha: &str, force: bool) -> UpdateCheck {
    check_web_update(&gh.cfg(), Some(sha), force)
        .await
        .expect("update check")
}

fn as_json(check: &UpdateCheck) -> Value {
    serde_json::to_value(check).unwrap()
}

#[tokio::test]
#[serial]
async fn three_commits_ahead_is_behind_with_three_newest_first() {
    let sha = sha_of(0x1001);
    let gh = Github::start(ok(compare_body(3, 0, 1, 3))).await;
    let result = as_json(&run(&gh, &sha, false).await);

    assert_eq!(result["status"], json!("behind"));
    assert_eq!(result["behindBy"], json!(3));
    assert_eq!(result["currentSha"], json!(sha));
    assert_eq!(result["latestSha"], json!(sha_of(3)));
    assert_eq!(result["currentVersion"], json!(env!("CARGO_PKG_VERSION")));
    assert_eq!(result["branch"], json!("main"));
    let shas: Vec<&str> = result["commits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["sha"].as_str().unwrap())
        .collect();
    assert_eq!(shas, [sha_of(3), sha_of(2), sha_of(1)]);
    // The committer date wins over the author date.
    assert_eq!(result["commits"][0]["date"], json!("2026-11-04T10:00:00Z"));
    assert_eq!(result["commits"][0]["message"], json!("subject 3"));
    assert!(result["checkedAt"].as_str().unwrap().ends_with('Z'));
    assert!(result["instructions"]["systemd"]
        .as_str()
        .unwrap()
        .contains("scripts/install-server.sh"));
    // The script installs the binary but does not restart the unit, and the unit file it
    // rewrites is only read after a daemon-reload.
    assert!(result["instructions"]["systemd"]
        .as_str()
        .unwrap()
        .ends_with("&& systemctl --user daemon-reload && systemctl --user restart cc-switch-web"));
    assert!(result["instructions"]["docker"]
        .as_str()
        .unwrap()
        .contains("Dockerfile.web"));
    // The clean-tree guard of the Docker path lives in this expression.
    assert!(result["instructions"]["docker"].as_str().unwrap().contains(
        "--build-arg CC_SWITCH_WEB_GIT_SHA=$(git diff --quiet HEAD && git rev-parse HEAD)"
    ));

    // The request itself: path, and the headers the real API insists on.
    assert_eq!(
        *gh.seen.compare_specs.lock().unwrap(),
        [format!("{sha}...main")]
    );
    assert_eq!(*gh.seen.user_agents.lock().unwrap(), ["cc-switch-web"]);
    assert_eq!(
        *gh.seen.accepts.lock().unwrap(),
        ["application/vnd.github+json"]
    );
    assert_eq!(
        gh.head_hits(),
        0,
        "the list covers ahead_by: no head lookup"
    );
}

#[tokio::test]
#[serial]
async fn same_commit_is_up_to_date() {
    let sha = sha_of(0x1002);
    let gh = Github::start(ok(compare_body(0, 0, 0, 0))).await;
    let result = as_json(&run(&gh, &sha, false).await);

    assert_eq!(result["status"], json!("up-to-date"));
    assert_eq!(result["behindBy"], json!(0));
    assert_eq!(result["latestSha"], json!(sha));
    assert_eq!(result["currentSha"], json!(sha));
    assert_eq!(result["commits"], json!([]));
}

#[tokio::test]
#[serial]
async fn sha_unknown_to_the_source_is_unknown_and_cached() {
    let sha = sha_of(0x1003);
    let gh = Github::start(Reply::json(
        StatusCode::NOT_FOUND,
        json!({"message": "Not Found"}),
    ))
    .await;
    let first = as_json(&run(&gh, &sha, false).await);
    assert_eq!(first["status"], json!("unknown"));
    assert_eq!(first["currentSha"], json!(sha));
    assert_eq!(first["latestSha"], Value::Null);
    assert_eq!(first["behindBy"], json!(0));
    assert_eq!(first["commits"], json!([]));

    let second = as_json(&run(&gh, &sha, false).await);
    assert_eq!(second, first);
    assert_eq!(gh.compare_hits(), 1, "a 404 answer is cached");
}

#[tokio::test]
#[serial]
async fn compare_422_is_unknown() {
    let sha = sha_of(0x1004);
    let gh = Github::start(Reply::json(
        StatusCode::UNPROCESSABLE_ENTITY,
        json!({"message": "No common ancestor between main and the sha"}),
    ))
    .await;
    let result = as_json(&run(&gh, &sha, false).await);
    assert_eq!(result["status"], json!("unknown"));
    assert_eq!(result["currentSha"], json!(sha));
}

#[tokio::test]
#[serial]
async fn diverged_build_is_unknown_not_behind() {
    let sha = sha_of(0x1005);
    // The branch has 2 commits the build lacks, the build has 1 the branch lacks.
    let gh = Github::start(ok(compare_body(2, 1, 1, 2))).await;
    let result = as_json(&run(&gh, &sha, false).await);
    assert_eq!(result["status"], json!("unknown"));
    assert_eq!(result["behindBy"], json!(0));
    assert_eq!(result["commits"], json!([]));
}

#[tokio::test]
#[serial]
async fn build_ahead_of_the_branch_is_unknown_not_up_to_date() {
    let sha = sha_of(0x1006);
    // ahead_by 0 with behind_by 4: the build holds 4 commits the branch lacks.
    let gh = Github::start(ok(compare_body(0, 4, 0, 0))).await;
    let result = as_json(&run(&gh, &sha, false).await);
    assert_eq!(result["status"], json!("unknown"));
    assert_eq!(result["latestSha"], Value::Null);
}

#[tokio::test]
#[serial]
async fn rate_limit_403_is_an_error_with_text_and_not_cached() {
    let sha = sha_of(0x1007);
    let mut reply = Reply::json(
        StatusCode::FORBIDDEN,
        json!({"message": "API rate limit exceeded for 203.0.113.9."}),
    );
    // 2026-11-04T12:34:56Z
    reply
        .headers
        .push(("x-ratelimit-reset", "1793795696".to_string()));
    let gh = Github::start(reply).await;

    let err = check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("HTTP 403"), "{text}");
    assert!(text.contains("rate limit"), "{text}");
    assert!(text.contains("retry after 12:34 UTC"), "{text}");

    assert!(check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .is_err());
    assert_eq!(gh.compare_hits(), 2, "an error is not cached");
}

#[tokio::test]
#[serial]
async fn rate_limit_429_is_an_error_with_text() {
    let sha = sha_of(0x1008);
    let gh = Github::start(Reply::json(
        StatusCode::TOO_MANY_REQUESTS,
        json!({"message": "slow down"}),
    ))
    .await;
    let text = check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .unwrap_err()
        .to_string();
    assert!(text.contains("HTTP 429"), "{text}");
    assert!(text.contains("rate limit"), "{text}");
    assert_eq!(gh.compare_hits(), 1);
}

#[tokio::test]
#[serial]
async fn server_error_and_dead_source_are_errors_not_cached() {
    let sha = sha_of(0x1009);
    let gh = Github::start(Reply::json(
        StatusCode::INTERNAL_SERVER_ERROR,
        json!({"message": "boom"}),
    ))
    .await;
    let text = check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .unwrap_err()
        .to_string();
    assert!(text.contains("HTTP 500"), "{text}");
    assert!(check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .is_err());
    assert_eq!(gh.compare_hits(), 2);

    let dead = {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };
    let cfg = UpdateSourceConfig {
        api_base: format!("http://127.0.0.1:{dead}"),
        repo: "o/r".to_string(),
        branch: "main".to_string(),
    };
    let text = check_web_update(&cfg, Some(&sha), false)
        .await
        .unwrap_err()
        .to_string();
    assert!(text.contains("could not reach"), "{text}");
}

#[tokio::test]
#[serial]
async fn ahead_by_beyond_the_list_asks_the_branch_head() {
    let sha = sha_of(0x100a);
    // GitHub lists at most 250 commits, oldest first, so the newest are missing.
    let gh = Github::start_with(
        ok(compare_body(300, 0, 1, 250)),
        Reply::json(StatusCode::OK, json!({"sha": sha_of(0xfeed)})),
        Duration::ZERO,
    )
    .await;
    let result = as_json(&run(&gh, &sha, false).await);

    assert_eq!(result["status"], json!("behind"));
    assert_eq!(
        result["behindBy"],
        json!(300),
        "ahead_by, not the list length"
    );
    assert_eq!(result["latestSha"], json!(sha_of(0xfeed)));
    assert_eq!(result["commits"].as_array().unwrap().len(), 20);
    assert_eq!(gh.head_hits(), 1);
}

#[tokio::test]
#[serial]
async fn more_than_twenty_commits_give_the_newest_twenty_first_lines_only() {
    let sha = sha_of(0x100b);
    let gh = Github::start(ok(compare_body(25, 0, 1, 25))).await;
    let result = as_json(&run(&gh, &sha, false).await);

    assert_eq!(result["behindBy"], json!(25));
    assert_eq!(result["latestSha"], json!(sha_of(25)));
    let commits = result["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 20);
    assert_eq!(commits[0]["sha"], json!(sha_of(25)));
    assert_eq!(commits[0]["message"], json!("subject 25"));
    assert_eq!(commits[19]["sha"], json!(sha_of(6)));
    assert_eq!(commits[19]["message"], json!("subject 6"));
    assert_eq!(gh.head_hits(), 0);
}

#[tokio::test]
#[serial]
async fn parallel_requests_cost_one_source_request() {
    let sha = sha_of(0x100c);
    let gh = Github::start_with(
        ok(compare_body(1, 0, 1, 1)),
        Reply::json(StatusCode::OK, json!({"sha": sha_of(1)})),
        Duration::from_millis(150),
    )
    .await;
    let cfg = gh.cfg();
    let (a, b, c, d) = tokio::join!(
        check_web_update(&cfg, Some(&sha), false),
        check_web_update(&cfg, Some(&sha), false),
        check_web_update(&cfg, Some(&sha), false),
        check_web_update(&cfg, Some(&sha), false),
    );
    for result in [a, b, c, d] {
        assert_eq!(as_json(&result.unwrap())["status"], json!("behind"));
    }
    assert_eq!(gh.compare_hits(), 1);
}

#[tokio::test]
#[serial]
async fn a_second_request_within_the_hour_is_served_from_the_cache() {
    let sha = sha_of(0x100d);
    let gh = Github::start(ok(compare_body(2, 0, 1, 2))).await;
    let first = run(&gh, &sha, false).await;
    let second = run(&gh, &sha, false).await;
    assert_eq!(second, first, "same answer, same checkedAt");
    assert_eq!(gh.compare_hits(), 1);
}

#[tokio::test]
#[serial]
async fn force_asks_the_source_again() {
    let sha = sha_of(0x100e);
    let gh = Github::start(ok(compare_body(2, 0, 1, 2))).await;
    let stale = run(&gh, &sha, false).await;
    assert_eq!(as_json(&stale)["status"], json!("behind"));
    // The source moves on: the build is now level with the branch.
    gh.set_compare(ok(compare_body(0, 0, 0, 0)));
    let forced = run(&gh, &sha, true).await;
    assert_eq!(as_json(&forced)["status"], json!("up-to-date"));
    assert_eq!(gh.compare_hits(), 2);
    // The forced answer replaces the cached one: a plain call returns it, not the old one.
    let after = run(&gh, &sha, false).await;
    assert_eq!(after, forced);
    assert_eq!(gh.compare_hits(), 2);
}

#[tokio::test]
#[serial]
async fn a_commit_without_a_committer_takes_the_author_date() {
    let sha = sha_of(0x1011);
    let mut body = compare_body(2, 0, 1, 2);
    // Commit 2 has no committer; commit 1 keeps both stamps.
    body["commits"][1]["commit"]["committer"] = Value::Null;
    let gh = Github::start(ok(body)).await;
    let result = as_json(&run(&gh, &sha, false).await);

    assert_eq!(result["commits"][0]["sha"], json!(sha_of(2)));
    assert_eq!(result["commits"][0]["date"], json!("1999-01-01T00:00:00Z"));
    assert_eq!(result["commits"][1]["date"], json!("2026-11-02T10:00:00Z"));
}

#[tokio::test]
#[serial]
async fn an_answer_without_the_counters_is_an_error_and_not_cached() {
    let sha = sha_of(0x1012);
    let gh = Github::start(ok(json!({}))).await;

    let text = check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .unwrap_err()
        .to_string();
    assert!(text.contains("unreadable answer"), "{text}");
    // Neither counter alone is enough.
    gh.set_compare(ok(json!({"ahead_by": 0})));
    assert!(check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .is_err());
    gh.set_compare(ok(json!({"behind_by": 0})));
    assert!(check_web_update(&gh.cfg(), Some(&sha), false)
        .await
        .is_err());
    assert_eq!(gh.compare_hits(), 3, "an unreadable answer is not cached");
}

#[tokio::test]
#[serial]
async fn a_cached_answer_is_not_shared_with_another_build_or_source() {
    let first_sha = sha_of(0x100f);
    let other_sha = sha_of(0x1010);
    let gh = Github::start(ok(compare_body(1, 0, 1, 1))).await;
    run(&gh, &first_sha, false).await;
    run(&gh, &other_sha, false).await;
    assert_eq!(gh.compare_hits(), 2, "another build asks again");

    let elsewhere = Github::start(ok(compare_body(1, 0, 1, 1))).await;
    run(&elsewhere, &other_sha, false).await;
    assert_eq!(elsewhere.compare_hits(), 1, "another source asks again");
}

#[tokio::test]
#[serial]
async fn a_cached_answer_names_the_branch_it_was_computed_for() {
    let sha = sha_of(0x1011);
    let gh = Github::start(ok(compare_body(1, 0, 1, 1))).await;
    let main = gh.cfg();
    let dev = UpdateSourceConfig {
        branch: "dev".to_string(),
        ..gh.cfg()
    };

    let first = as_json(&check_web_update(&main, Some(&sha), false).await.unwrap());
    assert_eq!(first["branch"], json!("main"));
    // Same source, same build, another branch: not served from main's entry.
    let other = as_json(&check_web_update(&dev, Some(&sha), false).await.unwrap());
    assert_eq!(other["branch"], json!("dev"));
    assert_eq!(gh.compare_hits(), 2, "another branch asks again");
    // And the one-hour cache still serves the branch it holds.
    let again = as_json(&check_web_update(&dev, Some(&sha), false).await.unwrap());
    assert_eq!(again["branch"], json!("dev"));
    assert_eq!(gh.compare_hits(), 2);
}

#[tokio::test]
#[serial]
async fn a_build_without_a_usable_commit_is_unknown_and_asks_nobody() {
    let gh = Github::start(ok(compare_body(3, 0, 1, 3))).await;
    let cfg = gh.cfg();

    let none = as_json(&check_web_update(&cfg, None, false).await.unwrap());
    assert_eq!(none["status"], json!("unknown"));
    assert_eq!(none["currentSha"], Value::Null);
    assert_eq!(none["latestSha"], Value::Null);
    assert_eq!(none["behindBy"], json!(0));
    assert_eq!(none["commits"], json!([]));
    assert_eq!(none["branch"], json!("main"), "even with no build commit");

    // Text that is not a commit hash never reaches the URL.
    for bad in ["", "main", "abc", "abc1234/../../x", "abc1234?x=1"] {
        let result = as_json(&check_web_update(&cfg, Some(bad), true).await.unwrap());
        assert_eq!(result["status"], json!("unknown"), "{bad:?}");
    }
    assert_eq!(gh.compare_hits(), 0);
}

#[test]
#[serial]
fn source_config_reads_the_environment_with_defaults() {
    let _api = EnvGuard::unset("CC_SWITCH_WEB_UPDATE_API");
    let _repo = EnvGuard::unset("CC_SWITCH_WEB_UPDATE_REPO");
    let _branch = EnvGuard::unset("CC_SWITCH_WEB_UPDATE_BRANCH");
    let defaults = UpdateSourceConfig::from_env();
    assert_eq!(defaults.api_base, "https://api.github.com");
    assert_eq!(defaults.repo, "alxwlw/cc-switch-web");
    assert_eq!(defaults.branch, "main");

    let _api = EnvGuard::set("CC_SWITCH_WEB_UPDATE_API", " http://127.0.0.1:9/ ");
    let _repo = EnvGuard::set("CC_SWITCH_WEB_UPDATE_REPO", "me/fork");
    let _branch = EnvGuard::set("CC_SWITCH_WEB_UPDATE_BRANCH", "  ");
    let set = UpdateSourceConfig::from_env();
    assert_eq!(set.api_base, "http://127.0.0.1:9");
    assert_eq!(set.repo, "me/fork");
    assert_eq!(set.branch, "main", "a blank value is not set");
}

/// The test binary is built without the commit, so the route cannot know its build:
/// `unknown`, and the source is not asked (not even with `force`).
#[tokio::test]
#[serial]
async fn route_without_a_build_commit_is_unknown_and_asks_nobody() {
    assert!(
        option_env!("CC_SWITCH_WEB_GIT_SHA").map_or(true, |v| v.trim().is_empty()),
        "this test needs a build without CC_SWITCH_WEB_GIT_SHA"
    );
    let _lock = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let state: Arc<AppState> = Arc::new(create_test_state().expect("state"));
    let router = cc_switch_lib::web_api::router(state);
    let gh = Github::start(ok(compare_body(3, 0, 1, 3))).await;
    let _api = EnvGuard::set("CC_SWITCH_WEB_UPDATE_API", &gh.url);
    let _repo = EnvGuard::set("CC_SWITCH_WEB_UPDATE_REPO", "o/r");
    let _branch = EnvGuard::set("CC_SWITCH_WEB_UPDATE_BRANCH", "main");

    for uri in ["/api/update/check", "/api/update/check?force=1"] {
        let (status, body) = web_call(&router, "GET", uri, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert_eq!(body["ok"], json!(true), "{uri}: {body}");
        assert_eq!(body["data"]["status"], json!("unknown"), "{uri}: {body}");
        assert_eq!(body["data"]["currentSha"], Value::Null, "{uri}: {body}");
        assert_eq!(
            body["data"]["currentVersion"],
            json!(env!("CARGO_PKG_VERSION"))
        );
        assert!(body["data"]["instructions"]["systemd"].is_string());
    }
    assert_eq!(gh.compare_hits(), 0);
}
