#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use cc_switch_lib::{update_settings, AppSettings};
use serde_json::{json, Value};
use serial_test::serial;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call, web_router_with};

/// Router over a fresh state with `openclaw_config_dir` pointing at a temp
/// dir. Returns the OpenClaw dir; the workspace is `<dir>/workspace`.
fn fresh() -> (axum::Router, PathBuf) {
    reset_test_fs();
    let home = ensure_test_home();
    let dir = home.join(".openclaw-web-api");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("openclaw dir");
    update_settings(AppSettings {
        openclaw_config_dir: Some(dir.to_string_lossy().into_owned()),
        ..AppSettings::default()
    })
    .expect("openclaw dir override");
    (
        web_router_with(Arc::new(create_test_state().expect("state"))),
        dir,
    )
}

async fn data(router: &axum::Router, method: &str, uri: &str, body: Option<Value>) -> Value {
    let (status, resp) = web_call(router, method, uri, body).await;
    assert_eq!(status, StatusCode::OK, "{method} {uri}: {resp}");
    assert_eq!(resp["ok"], json!(true), "{method} {uri}: {resp}");
    resp["data"].clone()
}

/// Status only: a router 404 has an empty body, which `web_call` cannot parse.
async fn status_of(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> StatusCode {
    use axum::body::Body;
    use axum::http::{header, Request};
    use tower::ServiceExt;

    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            req = req.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    router
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .expect("infallible")
        .status()
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .expect("read_dir")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[tokio::test]
#[serial]
async fn workspace_write_then_read_is_atomic() {
    let (router, dir) = fresh();
    let uri = "/api/openclaw/workspace/AGENTS.md";
    assert_eq!(data(&router, "GET", uri, None).await, Value::Null);
    data(&router, "PUT", uri, Some(json!({"content": "hello\nмир"}))).await;
    assert_eq!(data(&router, "GET", uri, None).await, json!("hello\nмир"));
    data(&router, "PUT", uri, Some(json!({"content": "second"}))).await;
    assert_eq!(data(&router, "GET", uri, None).await, json!("second"));
    assert_eq!(
        names(&dir.join("workspace")),
        vec!["AGENTS.md".to_string()],
        "tmp leftovers"
    );
}

#[tokio::test]
#[serial]
async fn disallowed_workspace_names_are_400_and_create_nothing() {
    let (router, dir) = fresh();
    // `..%2Fsecret.md` is decoded by axum into `../secret.md` and reaches the
    // handler; `%2e%2e%2f` likewise.
    for name in [
        "AGENTS.txt",
        "notes.md",
        "agents.md",
        "..%2Fsecret.md",
        "%2e%2e%2fsecret.md",
        "%2e%2e%2f%2e%2e%2fsecret.md",
    ] {
        let uri = format!("/api/openclaw/workspace/{name}");
        for (method, body) in [("GET", None), ("PUT", Some(json!({"content": "x"})))] {
            let (status, resp) = web_call(&router, method, &uri, body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {resp}");
            assert_eq!(resp["ok"], json!(false), "{method} {uri}: {resp}");
        }
    }
    assert!(!dir.join("secret.md").exists());
    assert!(!dir.join("workspace").join("secret.md").exists());
    assert!(!dir.parent().unwrap().join("secret.md").exists());
    assert!(!dir.join("workspace").join("notes.md").exists());
    assert!(!dir.join("workspace").join("AGENTS.txt").exists());
}

#[tokio::test]
#[serial]
async fn literal_traversal_path_never_succeeds() {
    let (router, dir) = fresh();
    // Literal slashes do not match `:filename`; the router answers 404.
    for method in ["GET", "PUT", "DELETE"] {
        let body = (method == "PUT").then(|| json!({"content": "x"}));
        for uri in [
            "/api/openclaw/workspace/../../etc/passwd",
            "/api/openclaw/daily-memory/../../secret.md",
        ] {
            let status = status_of(&router, method, uri, body.clone()).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
        }
    }
    assert!(!dir.join("secret.md").exists());
    assert!(!dir.parent().unwrap().join("secret.md").exists());
}

#[tokio::test]
#[serial]
async fn daily_memory_bad_names_are_400() {
    let (router, dir) = fresh();
    for name in [
        "notes.md",
        "2026-01-01.txt",
        "26-01-01.md",
        "%2e%2e%2f2026-01-01.md",
        "..%2F2026-01-01.md",
    ] {
        let uri = format!("/api/openclaw/daily-memory/{name}");
        for (method, body) in [
            ("GET", None),
            ("PUT", Some(json!({"content": "x"}))),
            ("DELETE", None),
        ] {
            let (status, resp) = web_call(&router, method, &uri, body).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{method} {uri}: {resp}");
            assert_eq!(resp["ok"], json!(false), "{method} {uri}: {resp}");
        }
    }
    assert!(!dir.join("workspace").exists(), "nothing was created");
    assert!(!dir.join("2026-01-01.md").exists());
}

/// `2026-13-45.md` matches the filename regex (it checks shape, not the
/// calendar), so the command accepts it. Pinned as the actual behaviour.
#[tokio::test]
#[serial]
async fn impossible_calendar_date_passes_the_regex() {
    let (router, dir) = fresh();
    let uri = "/api/openclaw/daily-memory/2026-13-45.md";
    data(&router, "PUT", uri, Some(json!({"content": "odd"}))).await;
    assert_eq!(data(&router, "GET", uri, None).await, json!("odd"));
    assert!(dir
        .join("workspace")
        .join("memory")
        .join("2026-13-45.md")
        .is_file());
}

#[tokio::test]
#[serial]
async fn daily_memory_write_read_list_search_delete() {
    let (router, dir) = fresh();
    let a = "/api/openclaw/daily-memory/2026-01-02.md";
    let b = "/api/openclaw/daily-memory/2026-03-04.md";
    assert_eq!(data(&router, "GET", a, None).await, Value::Null);
    assert_eq!(
        data(&router, "GET", "/api/openclaw/daily-memory", None).await,
        json!([])
    );
    data(
        &router,
        "PUT",
        a,
        Some(json!({"content": "buy Needle thread"})),
    )
    .await;
    data(&router, "PUT", b, Some(json!({"content": "nothing here"}))).await;
    assert_eq!(
        data(&router, "GET", a, None).await,
        json!("buy Needle thread")
    );
    assert_eq!(
        names(&dir.join("workspace").join("memory")),
        vec!["2026-01-02.md".to_string(), "2026-03-04.md".to_string()],
        "tmp leftovers"
    );

    let list = data(&router, "GET", "/api/openclaw/daily-memory", None).await;
    let files: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["filename"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        vec!["2026-03-04.md", "2026-01-02.md"],
        "newest first"
    );
    assert_eq!(list[1]["date"], json!("2026-01-02"));
    assert_eq!(list[1]["sizeBytes"], json!(17));

    let found = data(
        &router,
        "GET",
        "/api/openclaw/daily-memory/search?q=needle",
        None,
    )
    .await;
    assert_eq!(found.as_array().unwrap().len(), 1, "{found}");
    assert_eq!(found[0]["filename"], json!("2026-01-02.md"));
    assert_eq!(found[0]["matchCount"], json!(1));
    assert!(found[0]["snippet"].as_str().unwrap().contains("Needle"));
    // matches by date too, empty query matches nothing
    let by_date = data(
        &router,
        "GET",
        "/api/openclaw/daily-memory/search?q=2026-03",
        None,
    )
    .await;
    assert_eq!(by_date[0]["filename"], json!("2026-03-04.md"));
    assert_eq!(
        data(&router, "GET", "/api/openclaw/daily-memory/search", None).await,
        json!([])
    );

    data(&router, "DELETE", a, None).await;
    assert_eq!(data(&router, "GET", a, None).await, Value::Null);
    // idempotent
    data(&router, "DELETE", a, None).await;
    assert_eq!(data(&router, "GET", b, None).await, json!("nothing here"));
}

#[tokio::test]
#[serial]
async fn workspace_paths_match_override() {
    let (router, dir) = fresh();
    let paths = data(&router, "GET", "/api/openclaw/workspace-paths", None).await;
    assert_eq!(
        paths,
        json!({
            "workspace": dir.join("workspace").to_string_lossy(),
            "memory": dir.join("workspace").join("memory").to_string_lossy(),
        })
    );
}

/// Risk: an allowlisted name that is a symlink pointing outside the workspace.
#[cfg(unix)]
#[tokio::test]
#[serial]
async fn symlinked_workspace_file_does_not_write_outside() {
    use std::os::unix::fs::symlink;
    let (router, dir) = fresh();
    let outside = dir.parent().unwrap().join("outside-target.md");
    fs::write(&outside, "ORIGINAL").unwrap();
    let ws = dir.join("workspace");
    fs::create_dir_all(&ws).unwrap();
    symlink(&outside, ws.join("MEMORY.md")).unwrap();

    data(
        &router,
        "PUT",
        "/api/openclaw/workspace/MEMORY.md",
        Some(json!({"content": "PWNED"})),
    )
    .await;
    assert_eq!(
        fs::read_to_string(&outside).unwrap(),
        "ORIGINAL",
        "write followed the symlink out of the workspace"
    );
    assert_eq!(fs::read_to_string(ws.join("MEMORY.md")).unwrap(), "PWNED");
    assert!(!fs::symlink_metadata(ws.join("MEMORY.md"))
        .unwrap()
        .file_type()
        .is_symlink());
}

/// Pinned: reads DO follow a symlink out of the workspace (the command only
/// checks the name). Writes replace the link instead (see above).
#[cfg(unix)]
#[tokio::test]
#[serial]
async fn symlinked_workspace_file_read_follows_link() {
    use std::os::unix::fs::symlink;
    let (router, dir) = fresh();
    let outside = dir.parent().unwrap().join("outside-readable.md");
    fs::write(&outside, "OUTSIDE").unwrap();
    let ws = dir.join("workspace");
    fs::create_dir_all(&ws).unwrap();
    symlink(&outside, ws.join("SOUL.md")).unwrap();
    assert_eq!(
        data(&router, "GET", "/api/openclaw/workspace/SOUL.md", None).await,
        json!("OUTSIDE")
    );
}
