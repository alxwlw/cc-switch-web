//! WEB-28 T2: `POST /api/update/install`, `GET /api/update/status` and the `selfUpdate` flag of
//! `/api/capabilities`, through the axum harness with an injected `UpdateSpawner` (no
//! `systemd-run` ever runs).
//!
//! The conditions are read from the process environment when a request arrives, and the agent's
//! shell has `INVOCATION_ID` set: every test is `#[serial]` and starts from `Fixture::new`, which
//! saves the variables it touches, sets the "self-update is on" ones in a temporary directory and
//! puts everything back on drop.
#![cfg(unix)]

#[allow(dead_code)]
mod support;

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use cc_switch_lib::web_api::{
    init_error_router, router, router_with_update, SpawnError, UpdateLaunch, UpdateRunner,
    UpdateSpawner,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use serial_test::serial;
use support::{create_test_state, ensure_test_home, reset_test_fs, web_call};
use tower::ServiceExt;

const TOUCHED: [&str; 11] = [
    "CC_SWITCH_WEB_SELF_UPDATE",
    "CC_SWITCH_WEB_SOURCE_DIR",
    "CC_SWITCH_WEB_UPDATE_BRANCH",
    "CC_SWITCH_WEB_BUILD_PATH",
    "INVOCATION_ID",
    "XDG_STATE_HOME",
    "HOME",
    "HOST",
    "PORT",
    "WEB_DIST",
    "CC_SWITCH_WEB_SUPERVISED",
];
const ALREADY: &str = "обновление уже идёт";

struct Fixture {
    saved: Vec<(&'static str, Option<OsString>)>,
    source: tempfile::TempDir,
    state: tempfile::TempDir,
}

impl Fixture {
    /// Self-update fully on: flag, systemd, a checkout with the script, a state directory.
    fn new() -> Self {
        reset_test_fs();
        ensure_test_home();
        let saved = TOUCHED
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect();
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(source.path().join("scripts")).unwrap();
        let script = source.path().join("scripts/self-update.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let state = tempfile::tempdir().unwrap();
        for name in TOUCHED.iter().filter(|n| **n != "HOME") {
            std::env::remove_var(name);
        }
        std::env::set_var("CC_SWITCH_WEB_SELF_UPDATE", "1");
        std::env::set_var("INVOCATION_ID", "0f3c9a5e1d2b4c6f8a7b9c0d1e2f3a4b");
        std::env::set_var("CC_SWITCH_WEB_SOURCE_DIR", source.path());
        std::env::set_var("XDG_STATE_HOME", state.path());
        Self {
            saved,
            source,
            state,
        }
    }

    fn status_file(&self) -> PathBuf {
        self.state.path().join("cc-switch-web/update-status.json")
    }

    fn write_status(&self, body: &str) {
        let path = self.status_file();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn router(&self, spawner: &Arc<Fake>) -> axum::Router {
        let state = Arc::new(create_test_state().expect("create test state"));
        let spawner: Arc<dyn UpdateSpawner> = spawner.clone();
        router_with_update(state, None, Some(Arc::new(UpdateRunner::new(spawner))))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for (name, value) in &self.saved {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
    }
}

/// Plays `systemd-run`: records what it was asked, and what the status file looked like at
/// that moment.
#[derive(Default)]
struct Fake {
    launches: Mutex<Vec<UpdateLaunch>>,
    status_present_at_spawn: Mutex<Vec<bool>>,
    /// An update unit is alive: set by a successful spawn, or by the test.
    running: AtomicBool,
    fail_spawn: Mutex<Option<SpawnError>>,
    fail_probe: AtomicBool,
    in_flight: AtomicUsize,
    max_in_flight: AtomicUsize,
    /// Yields inside `spawn`, to give a second request every chance to get in.
    dawdle: AtomicBool,
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn calls(&self) -> usize {
        self.launches.lock().unwrap().len()
    }
}

impl UpdateSpawner for Fake {
    fn is_running(&self) -> Result<bool, String> {
        if self.fail_probe.load(Ordering::SeqCst) {
            return Err("bus unreachable".into());
        }
        Ok(self.running.load(Ordering::SeqCst))
    }

    fn spawn(&self, launch: &UpdateLaunch) -> Result<(), SpawnError> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_in_flight.fetch_max(now, Ordering::SeqCst);
        if self.dawdle.load(Ordering::SeqCst) {
            for _ in 0..2_000 {
                std::thread::yield_now();
            }
        }
        self.status_present_at_spawn
            .lock()
            .unwrap()
            .push(launch.status_file.exists());
        self.launches.lock().unwrap().push(launch.clone());
        let outcome = match self.fail_spawn.lock().unwrap().clone() {
            Some(e) => Err(e),
            None => {
                self.running.store(true, Ordering::SeqCst);
                Ok(())
            }
        };
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        outcome
    }
}

async fn install(router: &axum::Router) -> (StatusCode, Value) {
    web_call(router, "POST", "/api/update/install", Some(json!({}))).await
}

async fn status(router: &axum::Router) -> (StatusCode, Value) {
    web_call(router, "GET", "/api/update/status", None).await
}

async fn self_update_flag(router: &axum::Router) -> Value {
    let (code, body) = web_call(router, "GET", "/api/capabilities", None).await;
    assert_eq!(code, StatusCode::OK);
    body["selfUpdate"].clone()
}

#[tokio::test]
#[serial]
async fn every_missing_condition_is_a_409_with_its_reason_and_the_flag_follows() {
    let fake = Fake::new();

    type Break = fn(&Fixture);
    let cases: [(&str, Break, &str); 8] = [
        (
            "flag unset",
            |_| std::env::remove_var("CC_SWITCH_WEB_SELF_UPDATE"),
            "CC_SWITCH_WEB_SELF_UPDATE=1",
        ),
        (
            "flag is `true`",
            |_| std::env::set_var("CC_SWITCH_WEB_SELF_UPDATE", "true"),
            "CC_SWITCH_WEB_SELF_UPDATE=1",
        ),
        (
            "no systemd",
            |_| std::env::remove_var("INVOCATION_ID"),
            "systemd user unit",
        ),
        (
            "supervised flag is not systemd",
            |_| {
                std::env::remove_var("INVOCATION_ID");
                std::env::set_var("CC_SWITCH_WEB_SUPERVISED", "1");
            },
            "systemd user unit",
        ),
        (
            "no source dir",
            |_| std::env::remove_var("CC_SWITCH_WEB_SOURCE_DIR"),
            "CC_SWITCH_WEB_SOURCE_DIR is not set",
        ),
        (
            "relative source dir",
            |_| std::env::set_var("CC_SWITCH_WEB_SOURCE_DIR", "checkout"),
            "must be an absolute path",
        ),
        (
            "source dir that does not exist",
            |_| std::env::set_var("CC_SWITCH_WEB_SOURCE_DIR", "/nonexistent/cc-switch-web"),
            "is not a directory",
        ),
        (
            "checkout without the script",
            |fx| {
                std::fs::remove_file(fx.source.path().join("scripts/self-update.sh")).unwrap();
            },
            "no executable scripts/self-update.sh",
        ),
    ];
    for (name, break_it, reason) in cases {
        let fx_case = Fixture::new();
        let router = fx_case.router(&fake);
        break_it(&fx_case);

        let (code, body) = install(&router).await;

        assert_eq!(code, StatusCode::CONFLICT, "{name}: {body}");
        assert_eq!(body["ok"], json!(false), "{name}: {body}");
        let error = body["error"].as_str().unwrap();
        assert!(error.contains(reason), "{name}: {error}");
        assert!(
            !error.contains(fx_case.source.path().to_str().unwrap()),
            "{name}: the checkout path leaked: {error}"
        );
        assert_eq!(
            self_update_flag(&router).await,
            json!(false),
            "{name}: the flag must say what the route says"
        );
        drop(fx_case);
    }
    assert_eq!(fake.calls(), 0, "no refused request may reach the launcher");
}

#[tokio::test]
#[serial]
async fn an_enabled_install_is_202_and_the_launcher_gets_the_script_the_branch_and_the_env() {
    let fx = Fixture::new();
    std::env::set_var("CC_SWITCH_WEB_UPDATE_BRANCH", "release");
    std::env::set_var("HOST", "100.64.0.7");
    std::env::set_var("PORT", "19001");
    std::env::set_var("WEB_DIST", "/srv/dist-web");
    std::env::set_var("CC_SWITCH_WEB_BUILD_PATH", "/opt/node/bin:/opt/cargo/bin");
    let fake = Fake::new();
    let router = fx.router(&fake);
    assert_eq!(self_update_flag(&router).await, json!(true));

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::ACCEPTED, "{body}");
    assert_eq!(body["ok"], json!(true));
    assert_eq!(body["data"]["unit"], json!("cc-switch-web-update"));
    assert_eq!(body["data"]["branch"], json!("release"));
    let launches = fake.launches.lock().unwrap().clone();
    assert_eq!(launches.len(), 1);
    let launch = &launches[0];
    assert_eq!(launch.unit, "cc-switch-web-update");
    assert_eq!(
        launch.script,
        fx.source.path().join("scripts/self-update.sh")
    );
    assert_eq!(launch.source_dir, fx.source.path());
    assert_eq!(launch.branch, "release");
    assert_eq!(launch.status_file, fx.status_file());
    let env: Vec<(&str, String)> = launch
        .env
        .iter()
        .map(|(k, v)| (*k, v.to_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        env,
        [
            ("HOST", "100.64.0.7".to_string()),
            ("PORT", "19001".to_string()),
            ("WEB_DIST", "/srv/dist-web".to_string()),
            (
                "XDG_STATE_HOME",
                fx.state.path().to_str().unwrap().to_string()
            ),
            (
                "CC_SWITCH_WEB_BUILD_PATH",
                "/opt/node/bin:/opt/cargo/bin".to_string()
            ),
        ]
    );
}

#[tokio::test]
#[serial]
async fn the_default_branch_is_main_and_nothing_comes_from_the_body() {
    let fx = Fixture::new();
    let fake = Fake::new();
    let router = fx.router(&fake);

    let (code, body) = web_call(
        &router,
        "POST",
        "/api/update/install",
        Some(json!({
            "branch": "evil",
            "sourceDir": "/tmp",
            "script": "/bin/sh",
            "env": {"PORT": "1"},
        })),
    )
    .await;

    assert_eq!(code, StatusCode::ACCEPTED, "{body}");
    let launch = fake.launches.lock().unwrap()[0].clone();
    assert_eq!(launch.branch, "main");
    assert_eq!(launch.source_dir, fx.source.path());
    assert_eq!(
        launch.script,
        fx.source.path().join("scripts/self-update.sh")
    );
    assert!(launch.env.iter().all(|(k, _)| *k != "PORT"));
}

#[tokio::test]
#[serial]
async fn the_conditions_are_read_again_on_every_request() {
    let fx = Fixture::new();
    let fake = Fake::new();
    let router = fx.router(&fake);

    std::env::remove_var("CC_SWITCH_WEB_SELF_UPDATE");
    let (off, _) = install(&router).await;
    assert_eq!(off, StatusCode::CONFLICT);
    assert_eq!(self_update_flag(&router).await, json!(false));

    std::env::set_var("CC_SWITCH_WEB_SELF_UPDATE", "1");
    let (on, body) = install(&router).await;
    assert_eq!(on, StatusCode::ACCEPTED, "{body}");
    assert_eq!(self_update_flag(&router).await, json!(true));

    std::env::set_var("CC_SWITCH_WEB_UPDATE_BRANCH", "next");
    fake.running.store(false, Ordering::SeqCst);
    let (again, _) = install(&router).await;
    assert_eq!(again, StatusCode::ACCEPTED);
    assert_eq!(fake.launches.lock().unwrap()[1].branch, "next");
}

#[tokio::test]
#[serial]
async fn install_needs_a_json_content_type_before_anything_else() {
    let fx = Fixture::new();
    let fake = Fake::new();
    let router = fx.router(&fake);

    for content_type in [
        None,
        Some("text/plain"),
        Some("application/x-www-form-urlencoded"),
    ] {
        let mut req = Request::builder().method("POST").uri("/api/update/install");
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let resp = router
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .expect("router is infallible");
        assert_eq!(
            resp.status(),
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "content type {content_type:?}"
        );
    }
    // Disabled as well: the content type is still what answers.
    std::env::remove_var("CC_SWITCH_WEB_SELF_UPDATE");
    let resp = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/update/install")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("router is infallible");
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(fake.calls(), 0);
}

#[tokio::test]
#[serial]
async fn a_running_update_is_409_and_its_status_is_left_alone() {
    let fx = Fixture::new();
    let running_now = r#"{"stage":"building","ok":true,"message":"building 1c24a43","fromSha":"aaa1111","toSha":"1c24a43","startedAt":"2026-11-04T10:00:00Z","finishedAt":null}"#;
    fx.write_status(running_now);
    let fake = Fake::new();
    fake.running.store(true, Ordering::SeqCst);
    let router = fx.router(&fake);

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], json!(ALREADY));
    assert_eq!(fake.calls(), 0);
    let (_, shown) = status(&router).await;
    assert_eq!(shown["data"]["stage"], json!("building"), "{shown}");
    assert_eq!(
        std::fs::read_to_string(fx.status_file()).unwrap(),
        running_now
    );
}

#[tokio::test]
#[serial]
async fn a_unit_name_that_is_taken_at_launch_is_409_not_500() {
    let fx = Fixture::new();
    let fake = Fake::new();
    *fake.fail_spawn.lock().unwrap() = Some(SpawnError::AlreadyRunning);
    let router = fx.router(&fake);

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"], json!(ALREADY));
    assert_eq!(fake.calls(), 1);
}

#[tokio::test]
#[serial]
async fn two_posts_at_once_start_one_update() {
    let fx = Fixture::new();
    let fake = Fake::new();
    fake.dawdle.store(true, Ordering::SeqCst);
    let router = fx.router(&fake);

    let (a, b, c) = tokio::join!(install(&router), install(&router), install(&router));

    let mut codes = [a.0, b.0, c.0];
    codes.sort();
    assert_eq!(
        codes,
        [
            StatusCode::ACCEPTED,
            StatusCode::CONFLICT,
            StatusCode::CONFLICT
        ],
        "{a:?} {b:?} {c:?}"
    );
    assert_eq!(fake.calls(), 1, "exactly one launch");
    assert_eq!(fake.max_in_flight.load(Ordering::SeqCst), 1);
    for loser in [a, b, c]
        .into_iter()
        .filter(|r| r.0 == StatusCode::CONFLICT)
    {
        assert_eq!(loser.1["error"], json!(ALREADY));
    }
}

#[tokio::test]
#[serial]
async fn the_previous_status_is_cleared_before_the_unit_starts() {
    let fx = Fixture::new();
    fx.write_status(r#"{"stage":"done","ok":true,"message":"updated","fromSha":"a","toSha":"b","startedAt":"x","finishedAt":"y"}"#);
    let fake = Fake::new();
    let router = fx.router(&fake);
    assert_eq!(status(&router).await.1["data"]["stage"], json!("done"));

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::ACCEPTED, "{body}");
    assert_eq!(
        *fake.status_present_at_spawn.lock().unwrap(),
        [false],
        "the old status must be gone when the unit starts, or it could write into the old one"
    );
    let (code, shown) = status(&router).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(shown, json!({"ok": true, "data": null}));
}

#[tokio::test]
#[serial]
async fn a_launcher_that_fails_leaves_no_stage_behind() {
    let fx = Fixture::new();
    fx.write_status(r#"{"stage":"rolled-back","ok":false,"message":"old run","fromSha":"a","toSha":"b","startedAt":"x","finishedAt":"y"}"#);
    let fake = Fake::new();
    *fake.fail_spawn.lock().unwrap() = Some(SpawnError::Failed("systemd-run: no bus".into()));
    let router = fx.router(&fake);

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert!(body["error"].as_str().unwrap().contains("no bus"), "{body}");
    let (_, shown) = status(&router).await;
    assert_eq!(
        shown["data"],
        Value::Null,
        "the old stage must not pose as this run"
    );
    assert!(!fx.status_file().exists());
}

#[tokio::test]
#[serial]
async fn a_probe_that_cannot_tell_whether_an_update_runs_is_a_500_and_no_launch() {
    let fx = Fixture::new();
    let fake = Fake::new();
    fake.fail_probe.store(true, Ordering::SeqCst);
    let router = fx.router(&fake);

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["error"].as_str().unwrap().contains("bus unreachable"),
        "{body}"
    );
    assert_eq!(fake.calls(), 0);
}

#[tokio::test]
#[serial]
async fn a_status_file_that_cannot_be_cleared_stops_the_launch() {
    let fx = Fixture::new();
    // A directory where the file should be: remove_file fails with something but NotFound.
    std::fs::create_dir_all(fx.status_file()).unwrap();
    let fake = Fake::new();
    let router = fx.router(&fake);

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("cannot clear the previous update status"),
        "{body}"
    );
    assert_eq!(fake.calls(), 0);
}

#[tokio::test]
#[serial]
async fn a_router_without_a_launcher_is_409_even_with_everything_set() {
    let fx = Fixture::new();
    let _ = &fx;
    let state = Arc::new(create_test_state().expect("create test state"));
    let router = router(state);

    let (code, body) = install(&router).await;

    assert_eq!(code, StatusCode::CONFLICT, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("without an update launcher"),
        "{body}"
    );
    assert_eq!(self_update_flag(&router).await, json!(false));
}

#[tokio::test]
#[serial]
async fn the_init_error_router_never_offers_self_update() {
    let _fx = Fixture::new();
    let router = init_error_router();

    assert_eq!(self_update_flag(&router).await, json!(false));
}

#[tokio::test]
#[serial]
async fn status_is_null_before_any_update_and_then_the_file_field_by_field() {
    let fx = Fixture::new();
    let router = fx.router(&Fake::new());

    let (code, body) = status(&router).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body, json!({"ok": true, "data": null}));

    fx.write_status(r#"{"stage":"restarting","ok":true,"message":"restarting cc-switch-web","fromSha":"aaa1111","toSha":"bbb2222","startedAt":"2026-11-04T10:00:00Z","finishedAt":null,"extra":"ghp_secret"}"#);
    let (code, body) = status(&router).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        body,
        json!({"ok": true, "data": {
            "stage": "restarting",
            "ok": true,
            "message": "restarting cc-switch-web",
            "fromSha": "aaa1111",
            "toSha": "bbb2222",
            "startedAt": "2026-11-04T10:00:00Z",
            "finishedAt": null,
        }})
    );
}

#[tokio::test]
#[serial]
async fn status_is_read_from_home_local_state_when_xdg_state_home_is_empty() {
    let fx = Fixture::new();
    let home = tempfile::tempdir().unwrap();
    std::env::set_var("HOME", home.path());
    std::env::set_var("XDG_STATE_HOME", "");
    let file = home
        .path()
        .join(".local/state/cc-switch-web/update-status.json");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        r#"{"stage":"done","ok":true,"message":"updated","fromSha":"a","toSha":"b","startedAt":"x","finishedAt":"y"}"#,
    )
    .unwrap();
    let router = fx.router(&Fake::new());

    let (code, body) = status(&router).await;

    assert_eq!(code, StatusCode::OK, "{body}");
    assert_eq!(body["data"]["stage"], json!("done"));
}

#[tokio::test]
#[serial]
async fn a_broken_status_file_is_a_500_that_does_not_quote_the_file() {
    let fx = Fixture::new();
    fx.write_status("{\"stage\":\"build\npassword=hunter2");
    let router = fx.router(&Fake::new());

    let (code, body) = status(&router).await;

    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(
        body["error"],
        json!("the update status file is not valid JSON")
    );
    assert!(!body.to_string().contains("hunter2"), "{body}");
}
