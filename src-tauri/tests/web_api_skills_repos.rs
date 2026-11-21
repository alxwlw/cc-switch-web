//! `/api/skills/*` repo access tokens, per-repo discovery and local metadata refresh.
//! Split from `web_api_skills.rs` to keep both files under the size limit. The token
//! responses never carry a token value; repo downloads come from a local directory
//! through `skill_repo_fixture_test_hook`, so GitHub is never reached.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use support::{reset_test_fs, test_mutex, web_call, web_router};

/// Clears the process-global seam even when a test panics.
struct HookReset;

impl Drop for HookReset {
    fn drop(&mut self) {
        cc_switch_lib::web_api::skill_repo_fixture_test_hook(None);
    }
}

/// A local "repo" with two skills, wired in as the download source.
struct Remote {
    _root: tempfile::TempDir,
    _reset: HookReset,
}

impl Remote {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for (dir, name) in [("alpha", "Alpha"), ("beta", "Beta")] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
            std::fs::write(
                root.path().join(dir).join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {name} description\n---\n{name} v1\n"),
            )
            .unwrap();
        }
        cc_switch_lib::web_api::skill_repo_fixture_test_hook(Some(root.path().to_path_buf()));
        Self {
            _root: root,
            _reset: HookReset,
        }
    }
}

async fn call(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

async fn add_repo(router: &axum::Router) {
    let repo = json!({"owner": "acme", "name": "skills", "branch": "main", "enabled": true});
    call(
        router,
        "POST",
        "/api/skills/repos",
        Some(json!({"repo": repo})),
    )
    .await;
}

// ---------- repo tokens, per-repo discovery, metadata refresh ----------

/// Linux only: there the token store is the owner-only file under the test HOME. On macOS
/// and Windows CI the same store would reach the real system keychain, which a test must
/// not write to.
#[cfg(target_os = "linux")]
#[tokio::test]
#[serial]
async fn repo_tokens_round_trip_without_echoing_the_token() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    const SECRET: &str = "glpat-secret-123";
    let mut bodies: Vec<Value> = Vec::new();

    let (status, put) = web_call(
        &router,
        "PUT",
        "/api/skills/repo-tokens",
        Some(json!({"host": "GitLab.Example.com", "token": SECRET})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{put}");
    assert_eq!(put["ok"], json!(true), "{put}");
    assert_eq!(put["data"], json!("gitlab.example.com"));
    bodies.push(put);

    let (_, list) = web_call(&router, "GET", "/api/skills/repo-tokens", None).await;
    assert_eq!(list["ok"], json!(true), "{list}");
    assert_eq!(list["data"]["hosts"], json!(["gitlab.example.com"]));
    bodies.push(list);

    let (_, del) = web_call(
        &router,
        "DELETE",
        "/api/skills/repo-tokens/gitlab.example.com",
        None,
    )
    .await;
    assert_eq!(del["ok"], json!(true), "{del}");
    assert_eq!(del["data"], json!("gitlab.example.com"));
    bodies.push(del);

    let (_, list) = web_call(&router, "GET", "/api/skills/repo-tokens", None).await;
    assert_eq!(list["data"]["hosts"], json!([]), "{list}");
    bodies.push(list);

    for body in &bodies {
        assert!(
            !body.to_string().contains(SECRET),
            "a response carries the token: {body}"
        );
    }
}

#[tokio::test]
#[serial]
async fn rejected_repo_token_is_not_echoed_in_the_error() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    // a space is outside the accepted token alphabet, so nothing is stored
    const BAD: &str = "glpat-secret bad";

    let (_, resp) = web_call(
        &router,
        "PUT",
        "/api/skills/repo-tokens",
        Some(json!({"host": "gitlab.example.com", "token": BAD})),
    )
    .await;
    assert_eq!(resp["ok"], json!(false), "{resp}");
    assert!(
        !resp.to_string().contains("glpat-secret"),
        "the error carries the token: {resp}"
    );
}

#[tokio::test]
#[serial]
async fn discover_of_an_unknown_repo_is_empty() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let data = call(
        &router,
        "GET",
        "/api/skills/repos/nobody/nothing/discover",
        None,
    )
    .await;
    assert_eq!(
        data,
        // `SkillDiscoveryResult::default()` serialised (the type is not exported by the lib)
        json!({"skills": [], "failures": []})
    );
}

#[tokio::test]
#[serial]
async fn discover_of_a_saved_repo_lists_only_its_skills() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _remote = Remote::new();
    let router = web_router();
    add_repo(&router).await;

    let data = call(
        &router,
        "GET",
        "/api/skills/repos/acme/skills/discover",
        None,
    )
    .await;
    let mut names: Vec<&str> = data["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["Alpha", "Beta"], "{data}");
    assert_eq!(data["failures"], json!([]));
}

#[tokio::test]
#[serial]
async fn refresh_metadata_without_local_skills_counts_zero() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let data = call(&router, "POST", "/api/skills/refresh-metadata", None).await;
    assert_eq!(data, json!(0));
}
