//! WEB-26: `GET/PUT /api/app-config-dir` through the axum harness.
//!
//! Own test binary: the override cache is process-global and the tests set
//! `CC_SWITCH_CONFIG_DIR`, so every test is `#[serial]` and starts from `Fixture::new`,
//! which removes the env variable, the override file and the cached override.

#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};

use axum::http::StatusCode;
use cc_switch_lib::app_store::{
    headless_override_file_path, load_headless_override, read_headless_override_file,
    write_headless_override_file,
};
use cc_switch_lib::get_app_config_dir;
use serde_json::{json, Value};
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs, web_call, web_call_raw, web_router};

const URI: &str = "/api/app-config-dir";
const ENV_LOCKED_TEXT: &str = "каталог задан CC_SWITCH_CONFIG_DIR в unit сервера";

/// Clean slate for one test; puts `CC_SWITCH_CONFIG_DIR` back on drop so a failing
/// test cannot leak it into the next one.
struct Fixture {
    home: PathBuf,
    saved_env: Option<std::ffi::OsString>,
}

impl Fixture {
    fn new() -> Self {
        reset_test_fs();
        let home = ensure_test_home().to_path_buf();
        let saved_env = std::env::var_os("CC_SWITCH_CONFIG_DIR");
        std::env::remove_var("CC_SWITCH_CONFIG_DIR");
        let _ = std::fs::remove_file(headless_override_file_path());
        load_headless_override();
        Self { home, saved_env }
    }

    fn default_dir(&self) -> PathBuf {
        self.home.join(".cc-switch")
    }

    fn dir(&self, name: &str) -> PathBuf {
        let dir = self.home.join("elsewhere").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Writes the override file the way the server would find it at startup.
    fn write_file(&self, path: &Path) {
        write_headless_override_file(Some(path)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(headless_override_file_path());
        load_headless_override();
        match &self.saved_env {
            Some(v) => std::env::set_var("CC_SWITCH_CONFIG_DIR", v),
            None => std::env::remove_var("CC_SWITCH_CONFIG_DIR"),
        }
    }
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

async fn get_status(router: &axum::Router) -> Value {
    let (status, body) = web_call(router, "GET", URI, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ok"], json!(true));
    body["data"].clone()
}

async fn put(router: &axum::Router, path: Value) -> (StatusCode, Value) {
    web_call(router, "PUT", URI, Some(json!({ "path": path }))).await
}

fn error_text(body: &Value) -> &str {
    assert_eq!(body["ok"], json!(false), "{body}");
    body["error"].as_str().expect("error text")
}

#[tokio::test]
#[serial]
async fn get_reports_the_default_when_nothing_is_set() {
    let fx = Fixture::new();
    let router = web_router();

    let data = get_status(&router).await;

    assert_eq!(data["effective"], json!(s(&fx.default_dir())));
    assert_eq!(data["source"], json!("default"));
    assert_eq!(data["override"], Value::Null);
    assert_eq!(data["envLocked"], json!(false));
    assert_eq!(data["default"], json!(s(&fx.default_dir())));
}

#[tokio::test]
#[serial]
async fn get_with_env_is_locked_and_the_env_value_is_trimmed_like_the_resolver_does() {
    let fx = Fixture::new();
    let router = web_router();
    let from_file = fx.dir("from-file");
    fx.write_file(&from_file);
    let env_dir = fx.dir("from-env");
    std::env::set_var("CC_SWITCH_CONFIG_DIR", format!("  {}  ", s(&env_dir)));

    let data = get_status(&router).await;

    assert_eq!(data["envLocked"], json!(true));
    assert_eq!(data["source"], json!("env"));
    assert_eq!(data["effective"], json!(s(&env_dir)));
    assert_eq!(get_app_config_dir(), env_dir);
    // The file is still reported: it is what applies once the env is gone.
    assert_eq!(data["override"], json!(s(&from_file)));
}

#[tokio::test]
#[serial]
async fn get_ignores_an_empty_or_blank_env_exactly_as_the_resolver_does() {
    let fx = Fixture::new();
    let router = web_router();
    let target = fx.dir("override");
    fx.write_file(&target);
    load_headless_override();

    for blank in ["", "   ", "\t"] {
        std::env::set_var("CC_SWITCH_CONFIG_DIR", blank);
        let data = get_status(&router).await;
        assert_eq!(data["envLocked"], json!(false), "env {blank:?}");
        assert_eq!(data["source"], json!("override"), "env {blank:?}");
        assert_eq!(data["effective"], json!(s(&target)), "env {blank:?}");
        assert_eq!(get_app_config_dir(), target, "env {blank:?}");
    }

    // A blank env over an empty cache is the default, and a PUT is allowed.
    std::fs::remove_file(headless_override_file_path()).unwrap();
    load_headless_override();
    std::env::set_var("CC_SWITCH_CONFIG_DIR", "  ");
    let data = get_status(&router).await;
    assert_eq!(data["source"], json!("default"));
    assert_eq!(data["envLocked"], json!(false));
    let (status, body) = put(&router, json!(s(&fx.dir("after-blank-env")))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
#[serial]
async fn get_reports_a_loaded_override_as_source_override() {
    let fx = Fixture::new();
    let router = web_router();
    let target = fx.dir("override");
    fx.write_file(&target);
    assert_eq!(load_headless_override(), Some(target.clone()));

    let data = get_status(&router).await;

    assert_eq!(data["source"], json!("override"));
    assert_eq!(data["effective"], json!(s(&target)));
    assert_eq!(data["override"], json!(s(&target)));
    assert_eq!(data["envLocked"], json!(false));
    assert_eq!(data["default"], json!(s(&fx.default_dir())));
}

#[tokio::test]
#[serial]
async fn put_stores_the_override_for_the_next_start_and_leaves_this_process_alone() {
    let fx = Fixture::new();
    let router = web_router();
    let target = fx.dir("new-home");

    let (status, body) = put(&router, json!(s(&target))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["override"], json!(s(&target)));
    assert_eq!(body["data"]["source"], json!("default"));
    assert_eq!(body["data"]["effective"], json!(s(&fx.default_dir())));
    assert_eq!(read_headless_override_file(), Some(target.clone()));
    assert_eq!(get_app_config_dir(), fx.default_dir());
    let data = get_status(&router).await;
    assert_eq!(data["override"], json!(s(&target)));
    assert_eq!(data["effective"], json!(s(&fx.default_dir())));

    // "After restart": the startup load picks the stored value up.
    assert_eq!(load_headless_override(), Some(target.clone()));
    let data = get_status(&router).await;
    assert_eq!(data["source"], json!("override"));
    assert_eq!(data["effective"], json!(s(&target)));
}

#[tokio::test]
#[serial]
async fn put_creates_a_missing_directory_and_leaves_nothing_in_it() {
    let fx = Fixture::new();
    let router = web_router();
    let target = fx.home.join("elsewhere").join("not-yet").join("deep");
    assert!(!target.exists());

    let (status, body) = put(&router, json!(s(&target))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(target.is_dir());
    // The write probe and the DB check leave no file behind.
    assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0);
    assert_eq!(read_headless_override_file(), Some(target));
}

#[tokio::test]
#[serial]
async fn put_expands_tilde_and_stores_the_absolute_normalised_path() {
    let fx = Fixture::new();
    let router = web_router();

    let (status, body) = put(&router, json!("~/tilde-data/./sub/")).await;

    let expected = fx.home.join("tilde-data").join("sub");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["override"], json!(s(&expected)));
    assert_eq!(read_headless_override_file(), Some(expected.clone()));
    let stored = std::fs::read_to_string(headless_override_file_path()).unwrap();
    assert!(stored.contains(s(&expected)), "{stored}");
    assert!(!stored.contains('~'), "{stored}");

    let (status, body) = put(&router, json!("~")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read_headless_override_file(), Some(fx.home.clone()));
}

#[tokio::test]
#[serial]
async fn put_null_or_blank_removes_the_override() {
    let fx = Fixture::new();
    let router = web_router();
    for clear in [Value::Null, json!(""), json!("   ")] {
        fx.write_file(&fx.dir("to-remove"));
        assert!(read_headless_override_file().is_some());

        let (status, body) = put(&router, clear.clone()).await;

        assert_eq!(status, StatusCode::OK, "{clear}: {body}");
        assert_eq!(body["data"]["override"], Value::Null, "{clear}");
        assert_eq!(read_headless_override_file(), None, "{clear}");
    }
    // Nothing to remove is not an error either.
    let (status, _) = put(&router, Value::Null).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
#[serial]
async fn put_is_refused_with_409_while_the_env_pins_the_directory() {
    let fx = Fixture::new();
    let router = web_router();
    let existing = fx.dir("existing");
    fx.write_file(&existing);
    std::env::set_var("CC_SWITCH_CONFIG_DIR", s(&fx.dir("from-env")));

    for path in [json!(s(&fx.dir("wanted"))), Value::Null, json!("relative")] {
        let (status, body) = put(&router, path.clone()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{path}: {body}");
        assert_eq!(error_text(&body), ENV_LOCKED_TEXT);
    }
    assert_eq!(read_headless_override_file(), Some(existing));
}

#[tokio::test]
#[serial]
async fn put_refuses_paths_that_are_not_absolute_or_not_a_directory() {
    let fx = Fixture::new();
    let router = web_router();
    let a_file = fx.home.join("elsewhere").join("a-file");
    std::fs::create_dir_all(a_file.parent().unwrap()).unwrap();
    std::fs::write(&a_file, "x").unwrap();

    for (path, needle) in [
        ("relative/dir", "не абсолютный"),
        ("./dir", "не абсолютный"),
        ("~someone/dir", "не поддерживается"),
        (s(&a_file), "файл"),
    ] {
        let (status, body) = put(&router, json!(path)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}: {body}");
        assert!(error_text(&body).contains(needle), "{path}: {body}");
        assert_eq!(read_headless_override_file(), None, "{path}");
    }
    assert!(a_file.is_file());
}

#[tokio::test]
#[serial]
async fn put_without_the_path_key_is_rejected_and_keeps_the_override() {
    let fx = Fixture::new();
    let router = web_router();
    let existing = fx.dir("existing");
    fx.write_file(&existing);

    for body in [json!({}), json!({ "dir": "/tmp/x" })] {
        let (status, _, _) = web_call_raw(&router, "PUT", URI, Some(body.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    assert_eq!(read_headless_override_file(), Some(existing));
}

#[cfg(unix)]
#[tokio::test]
#[serial]
async fn put_refuses_a_directory_without_write_access() {
    use std::os::unix::fs::PermissionsExt;

    let fx = Fixture::new();
    let router = web_router();
    let target = fx.dir("read-only");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o555)).unwrap();

    let (status, body) = put(&router, json!(s(&target))).await;

    // Back to writable before any assertion can unwind, so the test home can be cleaned.
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(error_text(&body).contains("нельзя записывать"), "{body}");
    assert!(error_text(&body).contains(s(&target)), "{body}");
    assert_eq!(read_headless_override_file(), None);
}

fn sqlite_with_user_version(dir: &Path, version: i64) {
    let conn = rusqlite::Connection::open(dir.join("cc-switch.db")).unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {version};"))
        .unwrap();
}

#[tokio::test]
#[serial]
async fn put_refuses_a_database_newer_than_this_build_supports() {
    let fx = Fixture::new();
    let router = web_router();
    let target = fx.dir("future");
    sqlite_with_user_version(&target, 9999);

    let (status, body) = put(&router, json!(s(&target))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(error_text(&body).contains("9999"), "{body}");
    assert_eq!(read_headless_override_file(), None);
}

#[tokio::test]
#[serial]
async fn put_accepts_a_database_at_or_below_the_supported_version() {
    let fx = Fixture::new();
    let router = web_router();
    let target = fx.dir("older");
    sqlite_with_user_version(&target, 1);

    let (status, body) = put(&router, json!(s(&target))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(read_headless_override_file(), Some(target));
}

#[tokio::test]
#[serial]
async fn put_refuses_a_cc_switch_db_that_is_not_sqlite() {
    let fx = Fixture::new();
    let router = web_router();
    let target = fx.dir("garbage");
    std::fs::write(
        target.join("cc-switch.db"),
        "this is not a database, ".repeat(100),
    )
    .unwrap();

    let (status, body) = put(&router, json!(s(&target))).await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(error_text(&body).contains("SQLite"), "{body}");
    assert_eq!(read_headless_override_file(), None);
}
