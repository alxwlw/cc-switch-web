//! `/api/skills/*` beyond the installed list: repos, discovery, install, updates, backups,
//! import, storage migration and skills.sh search. GitHub is never reached: repo downloads
//! are served from a local directory through `skill_repo_fixture_test_hook` and skills.sh
//! search from a local axum listener through `skills_sh_search_url_test_hook`. The
//! install/update/uninstall write guard lives inside the service, so the web path inherits
//! the desktop behaviour for concurrent installs.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

/// Clears the process-global seams even when a test panics.
struct HookReset;

impl Drop for HookReset {
    fn drop(&mut self) {
        cc_switch_lib::web_api::skill_repo_fixture_test_hook(None);
        cc_switch_lib::web_api::skills_sh_search_url_test_hook(None);
    }
}

fn write_skill(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} description\n---\n{body}\n"),
    )
    .unwrap();
}

/// A local "repo" with two skills, wired in as the download source.
struct Remote {
    root: tempfile::TempDir,
    _reset: HookReset,
}

impl Remote {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        write_skill(&root.path().join("alpha"), "Alpha", "alpha v1");
        write_skill(&root.path().join("beta"), "Beta", "beta v1");
        cc_switch_lib::web_api::skill_repo_fixture_test_hook(Some(root.path().to_path_buf()));
        Self {
            root,
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

/// The discovered skill of the fixture repo with this directory.
async fn discovered(router: &axum::Router, directory: &str) -> Value {
    let report = call(router, "GET", "/api/skills/discover", None).await;
    report["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["repoOwner"] == "acme" && s["directory"] == directory)
        .unwrap_or_else(|| panic!("{directory} not discovered: {report}"))
        .clone()
}

async fn install(router: &axum::Router, skill: &Value) -> Value {
    call(
        router,
        "POST",
        "/api/skills/install",
        Some(json!({"skill": skill, "currentApp": "claude"})),
    )
    .await
}

async fn installed_ids(router: &axum::Router) -> Vec<String> {
    call(router, "GET", "/api/skills/installed", None)
        .await
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

fn ssot() -> PathBuf {
    ensure_test_home().join(".cc-switch").join("skills")
}

#[tokio::test]
#[serial]
async fn repos_add_list_invalid_and_delete() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    add_repo(&router).await;
    let repos = call(&router, "GET", "/api/skills/repos", None).await;
    let acme = repos
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["owner"] == "acme")
        .unwrap_or_else(|| panic!("repo not listed: {repos}"));
    assert_eq!(acme["name"], json!("skills"));
    assert_eq!(acme["branch"], json!("main"));

    let bad = json!({"repo": {"owner": "bad/owner", "name": "skills", "branch": "main", "enabled": true}});
    let (status, resp) = web_call(&router, "POST", "/api/skills/repos", Some(bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(
        resp["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{resp}"
    );
    let repos = call(&router, "GET", "/api/skills/repos", None).await;
    assert!(
        !repos
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["owner"] == "bad/owner"),
        "invalid repo was stored: {repos}"
    );

    assert_eq!(
        call(&router, "DELETE", "/api/skills/repos/acme/skills", None).await,
        json!(true)
    );
    let repos = call(&router, "GET", "/api/skills/repos", None).await;
    assert!(
        !repos
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["owner"] == "acme"),
        "repo still listed: {repos}"
    );
}

#[tokio::test]
#[serial]
async fn discover_then_install_puts_files_in_the_app_dir() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _remote = Remote::new();
    let router = web_router();
    add_repo(&router).await;

    let report = call(&router, "GET", "/api/skills/discover", None).await;
    let mut dirs: Vec<String> = report["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["repoOwner"] == "acme")
        .map(|s| s["directory"].as_str().unwrap().to_string())
        .collect();
    dirs.sort();
    assert_eq!(dirs, ["alpha", "beta"], "{report}");
    let alpha = discovered(&router, "alpha").await;
    assert_eq!(alpha["repoName"], json!("skills"));
    assert_eq!(alpha["key"], json!("acme/skills:alpha"));
    assert_eq!(alpha["name"], json!("Alpha"));

    let installed = install(&router, &alpha).await;
    assert_eq!(installed["id"], json!("acme/skills:alpha"));
    assert_eq!(installed["apps"]["claude"], json!(true));

    assert_eq!(installed_ids(&router).await, ["acme/skills:alpha"]);
    let home = ensure_test_home();
    assert!(std::fs::read_to_string(ssot().join("alpha/SKILL.md"))
        .unwrap()
        .contains("alpha v1"));
    let live = home.join(".claude/skills/alpha/SKILL.md");
    assert!(
        std::fs::read_to_string(&live).unwrap().contains("alpha v1"),
        "{} missing",
        live.display()
    );
}

#[tokio::test]
#[serial]
async fn install_with_an_unknown_app_is_a_400() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _remote = Remote::new();
    let router = web_router();
    add_repo(&router).await;
    let alpha = discovered(&router, "alpha").await;

    let (status, resp) = web_call(
        &router,
        "POST",
        "/api/skills/install",
        Some(json!({"skill": alpha, "currentApp": "nope"})),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(installed_ids(&router).await.is_empty());
}

#[tokio::test]
#[serial]
async fn updates_report_a_changed_skill_and_update_pulls_it() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let remote = Remote::new();
    let router = web_router();
    add_repo(&router).await;
    let alpha = discovered(&router, "alpha").await;
    install(&router, &alpha).await;

    let before = call(&router, "GET", "/api/skills/updates", None).await;
    assert!(
        before["updates"].as_array().unwrap().is_empty(),
        "nothing changed yet: {before}"
    );

    write_skill(&remote.root.path().join("alpha"), "Alpha", "alpha v2");
    let after = call(&router, "GET", "/api/skills/updates", None).await;
    let ids: Vec<&str> = after["updates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["acme/skills:alpha"], "{after}");

    // the id contains '/' and ':' so it travels in the body
    let updated = call(
        &router,
        "POST",
        "/api/skills/update",
        Some(json!({"id": "acme/skills:alpha"})),
    )
    .await;
    assert_eq!(updated["id"], json!("acme/skills:alpha"));
    assert!(std::fs::read_to_string(ssot().join("alpha/SKILL.md"))
        .unwrap()
        .contains("alpha v2"));
    let again = call(&router, "GET", "/api/skills/updates", None).await;
    assert!(again["updates"].as_array().unwrap().is_empty(), "{again}");
}

#[tokio::test]
#[serial]
async fn update_of_an_unknown_skill_fails() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _remote = Remote::new();
    let router = web_router();

    let (status, resp) = web_call(
        &router,
        "POST",
        "/api/skills/update",
        Some(json!({"id": "ghost/none:nothing"})),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
}

#[tokio::test]
#[serial]
async fn uninstall_backup_restore_and_delete_backup() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _remote = Remote::new();
    let router = web_router();
    add_repo(&router).await;
    let alpha = discovered(&router, "alpha").await;
    install(&router, &alpha).await;

    call(
        &router,
        "POST",
        "/api/skills/uninstall",
        Some(json!({"id": "acme/skills:alpha"})),
    )
    .await;
    assert!(installed_ids(&router).await.is_empty());
    let backups = call(&router, "GET", "/api/skills/backups", None).await;
    let backups = backups.as_array().unwrap();
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(backups[0]["skill"]["id"], json!("acme/skills:alpha"));
    let backup_id = backups[0]["backupId"].as_str().unwrap().to_string();

    let restored = call(
        &router,
        "POST",
        &format!("/api/skills/backups/{backup_id}/restore"),
        Some(json!({"currentApp": "claude"})),
    )
    .await;
    assert_eq!(restored["id"], json!("acme/skills:alpha"));
    assert_eq!(restored["apps"]["claude"], json!(true), "{restored}");
    assert_eq!(restored["apps"]["codex"], json!(false), "{restored}");
    assert_eq!(installed_ids(&router).await, ["acme/skills:alpha"]);
    assert!(ssot().join("alpha/SKILL.md").exists());

    assert_eq!(
        call(
            &router,
            "DELETE",
            &format!("/api/skills/backups/{backup_id}"),
            None
        )
        .await,
        json!(true)
    );
    let backups = call(&router, "GET", "/api/skills/backups", None).await;
    assert!(backups.as_array().unwrap().is_empty(), "{backups}");
}

#[tokio::test]
#[serial]
async fn restore_with_an_unknown_backup_fails() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, resp) = web_call(
        &router,
        "POST",
        "/api/skills/backups/no-such-backup/restore",
        Some(json!({"currentApp": "claude"})),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
}

#[tokio::test]
#[serial]
async fn unmanaged_skill_in_an_app_dir_can_be_imported() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    write_skill(&home.join(".claude/skills/local-one"), "Local One", "local");
    let router = web_router();

    let unmanaged = call(&router, "GET", "/api/skills/unmanaged", None).await;
    let found = unmanaged
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["directory"] == "local-one")
        .unwrap_or_else(|| panic!("not listed: {unmanaged}"));
    assert_eq!(found["name"], json!("Local One"));

    let imported = call(
        &router,
        "POST",
        "/api/skills/import-from-apps",
        Some(json!({"imports": [{"directory": "local-one", "apps": {"claude": true}}]})),
    )
    .await;
    assert_eq!(imported.as_array().unwrap().len(), 1, "{imported}");
    assert_eq!(imported[0]["directory"], json!("local-one"));
    assert_eq!(imported[0]["apps"]["claude"], json!(true));
    assert_eq!(installed_ids(&router).await, ["local:local-one"]);
    assert!(ssot().join("local-one/SKILL.md").exists());
}

#[tokio::test]
#[serial]
async fn migrate_storage_moves_the_skills_and_dir_stays_the_cc_switch_dir() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let agents = home.join(".agents");
    let _ = std::fs::remove_dir_all(&agents);
    let _remote = Remote::new();
    let router = web_router();
    add_repo(&router).await;
    let alpha = discovered(&router, "alpha").await;
    install(&router, &alpha).await;

    let dir = call(&router, "GET", "/api/skills/dir", None).await;
    assert_eq!(dir, json!(ssot().to_string_lossy()));

    let moved = call(
        &router,
        "POST",
        "/api/skills/migrate-storage",
        Some(json!({"target": "unified"})),
    )
    .await;
    assert_eq!(moved["migratedCount"], json!(1), "{moved}");
    assert_eq!(moved["errors"], json!([]), "{moved}");
    assert!(agents.join("skills/alpha/SKILL.md").exists());
    assert!(!ssot().join("alpha").exists(), "old copy left behind");
    // like the desktop helper, /dir is always the CC Switch dir, not the storage location
    let dir = call(&router, "GET", "/api/skills/dir", None).await;
    assert_eq!(dir, json!(ssot().to_string_lossy()));

    let back = call(
        &router,
        "POST",
        "/api/skills/migrate-storage",
        Some(json!({"target": "cc_switch"})),
    )
    .await;
    assert_eq!(back["migratedCount"], json!(1), "{back}");
    assert!(ssot().join("alpha/SKILL.md").exists());
    assert!(!agents.join("skills/alpha").exists());
    let dir = call(&router, "GET", "/api/skills/dir", None).await;
    assert_eq!(dir, json!(ssot().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&agents);
}

#[tokio::test]
#[serial]
async fn migrate_storage_rejects_an_unknown_target() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    // the extractor's rejection is plain text, so read the raw response
    let (status, _, _) = support::web_call_raw(
        &router,
        "POST",
        "/api/skills/migrate-storage",
        Some(json!({"target": "elsewhere"})),
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

/// Serves `status` + `body` on 127.0.0.1 and records each request's path-and-query.
async fn serve_skills_sh(
    status: StatusCode,
    body: Value,
) -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
    use axum::{extract::OriginalUri, routing::get, Router};
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let app = Router::new().route(
        "/api/search",
        get(move |OriginalUri(uri): OriginalUri| {
            let log = log.clone();
            let body = body.clone();
            async move {
                log.lock().unwrap().push(uri.to_string());
                (status, axum::Json(body))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/search", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (url, seen, handle)
}

#[tokio::test]
#[serial]
async fn search_forwards_query_limit_offset_and_maps_the_result() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let canned = json!({
        "query": "pdf tools",
        "searchType": "fuzzy",
        "skills": [{
            "id": "acme/skills/pdf",
            "skillId": "pdf",
            "name": "PDF",
            "installs": 42,
            "source": "acme/skills"
        }],
        "count": 7,
        "duration_ms": 3
    });
    let (url, seen, server) = serve_skills_sh(StatusCode::OK, canned).await;
    let _reset = HookReset;
    cc_switch_lib::web_api::skills_sh_search_url_test_hook(Some(url));
    let router = web_router();

    let result = call(
        &router,
        "GET",
        "/api/skills/search?query=pdf%20tools&limit=5&offset=10",
        None,
    )
    .await;

    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].contains("q=pdf+tools") || requests[0].contains("q=pdf%20tools"));
    assert!(requests[0].contains("limit=5"), "{}", requests[0]);
    assert!(requests[0].contains("offset=10"), "{}", requests[0]);
    assert_eq!(result["totalCount"], json!(7), "{result}");
    assert_eq!(result["skills"][0]["key"], json!("acme/skills/pdf"));
    assert_eq!(result["skills"][0]["directory"], json!("pdf"));
    assert_eq!(result["skills"][0]["repoOwner"], json!("acme"));
    assert_eq!(result["skills"][0]["installs"], json!(42));
    server.abort();
}

#[tokio::test]
#[serial]
async fn search_upstream_failure_is_ok_false() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let (url, _seen, server) =
        serve_skills_sh(StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "down"})).await;
    let _reset = HookReset;
    cc_switch_lib::web_api::skills_sh_search_url_test_hook(Some(url));
    let router = web_router();

    let (status, resp) = web_call(
        &router,
        "GET",
        "/api/skills/search?query=pdf&limit=5&offset=0",
        None,
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(resp["error"].as_str().unwrap().contains("500"), "{resp}");
    server.abort();
}

// ---- ZIP upload (`POST /api/skills/install-zip`) ----

use std::io::Write;

/// A zip with `(path, content)` entries.
fn zip_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for (path, content) in entries {
        w.start_file(*path, opts).unwrap();
        w.write_all(content.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: {name} description\n---\n{name} body\n")
}

/// Raw-body POST (octet-stream); `web_call` is JSON-only.
async fn upload(router: &axum::Router, query: &str, body: Vec<u8>) -> (StatusCode, Value) {
    use axum::body::Body;
    use axum::http::{header, Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/skills/install-zip?{query}"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(body))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

#[tokio::test]
#[serial]
async fn install_zip_installs_the_skill() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    let zip = zip_bytes(&[("pack/SKILL.md", &skill_md("Packed"))]);

    let (status, resp) = upload(&router, "currentApp=claude&filename=pack.zip", zip).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(resp["ok"], json!(true), "{resp}");
    let installed = resp["data"]["installed"].as_array().unwrap();
    assert_eq!(installed.len(), 1, "{resp}");
    assert_eq!(installed[0]["directory"], json!("pack"));
    assert_eq!(installed_ids(&router).await.len(), 1);
    assert!(ssot().join("pack/SKILL.md").exists());
}

#[tokio::test]
#[serial]
async fn install_zip_names_a_root_skill_after_the_uploaded_file() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    // no `name:` in the frontmatter: the install name comes from the file stem
    let zip = zip_bytes(&[("SKILL.md", "---\ndescription: nameless\n---\nbody\n")]);

    let (status, resp) = upload(&router, "currentApp=claude&filename=my-uploaded.skill", zip).await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert_eq!(
        resp["data"]["installed"][0]["directory"],
        json!("my-uploaded"),
        "{resp}"
    );
    assert!(ssot().join("my-uploaded/SKILL.md").exists());
}

#[tokio::test]
#[serial]
async fn install_zip_rejects_non_zip_bytes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, resp) = upload(
        &router,
        "currentApp=claude&filename=a.zip",
        b"not a zip".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(
        resp["error"]
            .as_str()
            .unwrap()
            .contains("Failed to read ZIP file"),
        "{resp}"
    );
    assert!(installed_ids(&router).await.is_empty());
}

#[tokio::test]
#[serial]
async fn install_zip_rejects_other_extensions_and_missing_names() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    let zip = zip_bytes(&[("pack/SKILL.md", &skill_md("Packed"))]);

    for query in [
        "currentApp=claude&filename=a.txt",
        "currentApp=claude&filename=",
        "currentApp=claude&filename=..",
        "currentApp=nope&filename=a.zip",
    ] {
        let (status, resp) = upload(&router, query, zip.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {resp}");
        assert_eq!(resp["ok"], json!(false), "{query}");
    }
    assert!(installed_ids(&router).await.is_empty());
}

#[tokio::test]
#[serial]
async fn install_zip_skips_path_traversal_entries() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let router = web_router();
    let zip = zip_bytes(&[
        ("../evil/SKILL.md", &skill_md("Evil")),
        ("good/SKILL.md", &skill_md("Good")),
    ]);

    // a directory-bearing filename cannot escape the upload temp dir either
    let trap = std::env::temp_dir().join("skills-zip-trap.zip");
    let _ = std::fs::remove_file(&trap);
    let (status, resp) = upload(
        &router,
        "currentApp=claude&filename=..%2Fskills-zip-trap.zip",
        zip,
    )
    .await;
    let escaped = trap.exists();
    let _ = std::fs::remove_file(&trap);
    assert!(!escaped, "upload written outside its temp dir");
    assert_eq!(status, StatusCode::OK, "{resp}");
    let dirs: Vec<&str> = resp["data"]["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["directory"].as_str().unwrap())
        .collect();
    assert_eq!(dirs, ["good"], "{resp}");
    assert!(ssot().join("good/SKILL.md").exists());
    assert!(!ssot().join("evil").exists());
    assert!(!ssot().parent().unwrap().join("evil").exists());
    assert!(!home.join("evil").exists());
}

#[tokio::test]
#[serial]
async fn install_zip_over_the_body_limit_is_413() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let big = vec![0u8; 50 * 1024 * 1024 + 1];
    let (status, _) = upload(&router, "currentApp=claude&filename=big.zip", big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
#[serial]
async fn install_zip_entry_count_bomb_is_refused() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    // MAX_ARCHIVE_ENTRIES (30_000) + 1 empty entries; the 512 MiB byte budget is covered
    // by the unit tests in services/skill.rs
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..30_001 {
        w.start_file(format!("e{i}.txt"), opts).unwrap();
    }
    let zip = w.finish().unwrap().into_inner();

    let (status, resp) = upload(&router, "currentApp=claude&filename=bomb.zip", zip).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{resp}");
    assert_eq!(resp["ok"], json!(false));
    assert!(
        resp["error"]
            .as_str()
            .unwrap()
            .contains("ARCHIVE_TOO_MANY_ENTRIES"),
        "{resp}"
    );
    assert!(installed_ids(&router).await.is_empty());
}
