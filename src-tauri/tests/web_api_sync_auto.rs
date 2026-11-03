//! Auto-sync of the headless web server (WEB-11, phase 34): a write to a config table of the
//! database reaches the WebDAV and S3 workers through the SQLite change hook, they upload to the
//! stub of `support/dav_stub.rs` and report on the event hub (`webdav-sync-status-updated`,
//! `s3-sync-status-updated`, payload `{source:"auto", status, error?}`), the outcome is saved to
//! the settings and read back through `GET /api/settings`.
//!
//! Its own binary, apart from `web_api_sync.rs` (which has no room left and never starts the
//! workers): the worker start is process-wide (`DB_CHANGE_TX` is a `OnceLock` that keeps the
//! database and the hub of the FIRST `start_worker`), so every test here shares ONE fixture and
//! the workers are started once, through the bootstrap's own `start_auto_sync_workers`. The sync
//! settings are global too, so the tests are `#[serial]` and each starts by switching auto-sync
//! off and waiting for the worker to go quiet. The worker runs on tauri's default runtime, which
//! outlives the single `#[tokio::test]` runtimes.
//!
//! "Nothing happens" is only believed when the same test then makes it happen: each negative
//! wait (longer than `MAX_AUTO_SYNC_WAIT_MS`, the worker's upper bound for a debounce) is
//! followed by a positive control against the same stub. The stub's loopback-prober `GET /` is
//! never recorded, and the assertions look at PUT only.
#![cfg(not(target_os = "windows"))]

#[path = "support/dav_stub.rs"]
mod dav_stub;
mod support;
#[path = "support/sync_fixtures.rs"]
mod sync_fixtures;

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use cc_switch_lib::events::Event;
use dav_stub::DavStub;
use serde_json::{json, Value};
use serial_test::serial;
use sync_fixtures::*;
use tokio::sync::broadcast::{error::RecvError, Receiver};

const WEBDAV_EVENT: &str = "webdav-sync-status-updated";
const S3_EVENT: &str = "s3-sync-status-updated";
/// A change reaches the stub within the debounce (1 s) plus the upload; the rest is margin for
/// a loaded machine. It is an upper bound, a positive wait returns at the event.
const POSITIVE_WAIT: Duration = Duration::from_secs(15);
/// `MAX_AUTO_SYNC_WAIT_MS` (10 s, `services/webdav_auto_sync.rs`) plus a margin: an upload that
/// is going to happen has happened by then.
const NEGATIVE_WAIT: Duration = Duration::from_millis(11_500);
/// Longer than the debounce (1 s): a pending run of an earlier step has fired by then.
const QUIET_FOR: Duration = Duration::from_millis(1_500);
const WEBDAV_REMOTE: &str = REMOTE_DIR;
const S3_REMOTE: &str = "/bucket/cc-switch-sync/v2/db-v6/default";

/// One fixture for the whole binary, workers started on it (see the module comment).
fn shared() -> &'static Fixture {
    static FX: OnceLock<Fixture> = OnceLock::new();
    FX.get_or_init(|| {
        let fx = fixture_with_providers();
        let step =
            cc_switch_lib::bootstrap::start_auto_sync_workers(&fx.state.db, &fx.state.events);
        assert_eq!(step, cc_switch_lib::bootstrap::StepOutcome::Done);
        fx
    })
}

/// The shared fixture with auto-sync off and the worker quiet, plus a hub subscription taken
/// before anything of the test writes (a receiver sees only what comes after it subscribed).
async fn begin() -> (&'static Fixture, Receiver<Event>) {
    let fx = shared();
    // Off first: a run still pending from an earlier test then finds nothing to do.
    cc_switch_lib::update_settings(cc_switch_lib::AppSettings::default()).expect("reset settings");
    let mut events = fx.state.events.subscribe();
    quiet(&mut events).await;
    // The database outlives the tests: back to the fixture's provider, unheard by the workers.
    write_provider_key_unhooked("k-old");
    (fx, events)
}

/// Returns once the hub saw no event for `QUIET_FOR`.
async fn quiet(events: &mut Receiver<Event>) {
    while let Ok(received) = tokio::time::timeout(QUIET_FOR, events.recv()).await {
        assert!(
            !matches!(received, Err(RecvError::Closed)),
            "event hub closed"
        );
    }
}

/// The first `source:"auto"` event of each name in `names`, or a panic after `POSITIVE_WAIT`.
async fn auto_events(events: &mut Receiver<Event>, names: &[&str]) -> BTreeMap<String, Event> {
    let deadline = tokio::time::Instant::now() + POSITIVE_WAIT;
    let mut got = BTreeMap::new();
    while got.len() < names.len() {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event))
                if event.payload["source"] == "auto"
                    && names.contains(&event.name.as_str())
                    && !got.contains_key(&event.name) =>
            {
                got.insert(event.name.clone(), event);
            }
            Ok(Ok(_)) | Ok(Err(RecvError::Lagged(_))) => {}
            Ok(Err(e)) => panic!("event hub closed: {e}"),
            Err(_) => panic!("no auto event within {POSITIVE_WAIT:?} for {names:?}, got {got:?}"),
        }
    }
    got
}

/// Nothing named `names` with `source:"auto"` comes on the hub for `NEGATIVE_WAIT`.
async fn no_auto_events(events: &mut Receiver<Event>, names: &[&str]) {
    let deadline = tokio::time::Instant::now() + NEGATIVE_WAIT;
    loop {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) => assert!(
                !(event.payload["source"] == "auto" && names.contains(&event.name.as_str())),
                "unexpected {event:?}"
            ),
            Ok(Err(RecvError::Lagged(_))) => {}
            Ok(Err(e)) => panic!("event hub closed: {e}"),
            Err(_) => return,
        }
    }
}

fn webdav_body(stub: &DavStub, enabled: bool, auto_sync: bool) -> Value {
    let mut settings = webdav_settings(&stub.base_url());
    settings["enabled"] = json!(enabled);
    settings["autoSync"] = json!(auto_sync);
    settings
}

fn s3_body(stub: &DavStub, enabled: bool, auto_sync: bool) -> Value {
    let mut settings = s3_settings_for(stub);
    settings["enabled"] = json!(enabled);
    settings["autoSync"] = json!(auto_sync);
    settings
}

/// Saves both backends' settings through the save routes.
async fn configure(fx: &Fixture, webdav: Value, s3: Value) {
    for (backend, settings) in [("webdav", webdav), ("s3", s3)] {
        data(
            &fx.router,
            "PUT",
            &format!("/api/sync/{backend}/settings"),
            Some(json!({ "settings": settings, "passwordTouched": true })),
        )
        .await;
    }
}

/// A write to the `providers` table through the hooked connection of the server.
fn write_provider_key(fx: &Fixture, key: &str) {
    let mut provider = fx
        .state
        .db
        .get_provider_by_id("old-provider", "claude")
        .unwrap()
        .unwrap();
    provider.settings_config = json!({ "env": { "ANTHROPIC_API_KEY": key } });
    fx.state.db.save_provider("claude", &provider).unwrap();
}

/// The same write on a second connection to the database file: the change hook belongs to the
/// connection of the server, so this one tells no worker.
fn write_provider_key_unhooked(key: &str) {
    let path = support::ensure_test_home()
        .join(".cc-switch")
        .join("cc-switch.db");
    let conn = rusqlite::Connection::open(path).expect("second connection");
    let changed = conn
        .execute(
            "UPDATE providers SET settings_config = ?1 WHERE id = 'old-provider' AND app_type = 'claude'",
            [json!({ "env": { "ANTHROPIC_API_KEY": key } }).to_string()],
        )
        .expect("update");
    assert_eq!(changed, 1);
}

/// `webdavSync.status` / `s3Sync.status` of `GET /api/settings`.
async fn status_of(fx: &Fixture, section: &str) -> Value {
    let settings = data(&fx.router, "GET", "/api/settings", None).await;
    settings[section]["status"].clone()
}

fn puts(stub: &DavStub, prefix: &str) -> Vec<String> {
    stub.requests("PUT")
        .into_iter()
        .map(|r| r.path)
        .filter(|p| p.starts_with(prefix))
        .collect()
}

fn text_of(stub: &DavStub, path: &str) -> String {
    String::from_utf8(
        stub.file(path)
            .unwrap_or_else(|| panic!("{path} is not on the stub")),
    )
    .expect("utf-8")
}

#[tokio::test]
#[serial]
async fn a_provider_write_uploads_to_both_remotes_and_reports_success() {
    let (fx, mut events) = begin().await;
    let stub = DavStub::start().await;
    configure(
        fx,
        webdav_body(&stub, true, true),
        s3_body(&stub, true, true),
    )
    .await;

    write_provider_key(fx, "k-auto-success");
    let got = auto_events(&mut events, &[WEBDAV_EVENT, S3_EVENT]).await;
    for name in [WEBDAV_EVENT, S3_EVENT] {
        assert_eq!(
            got[name].payload,
            json!({ "source": "auto", "status": "success" }),
            "{name}"
        );
    }

    // The snapshot of the changed database is what reached each remote.
    for (remote, root) in [(WEBDAV_REMOTE, "/dav"), (S3_REMOTE, "/bucket")] {
        assert_eq!(
            puts(&stub, root),
            ["db.sql", "skills.zip", "manifest.json"].map(|f| format!("{remote}/{f}")),
            "{root}"
        );
        assert!(
            text_of(&stub, &format!("{remote}/db.sql")).contains("k-auto-success"),
            "{root}: db.sql carries the new key"
        );
    }
    // And the success is on record, with no error.
    for section in ["webdavSync", "s3Sync"] {
        let status = status_of(fx, section).await;
        assert!(
            status["lastSyncAt"].as_i64().is_some(),
            "{section}: {status}"
        );
        assert!(status.get("lastError").is_none(), "{section}: {status}");
    }
}

#[tokio::test]
#[serial]
async fn a_failing_remote_reports_error_and_the_settings_show_it_until_the_next_success() {
    let (fx, mut events) = begin().await;
    let stub = DavStub::start().await;
    configure(
        fx,
        webdav_body(&stub, true, true),
        s3_body(&stub, true, true),
    )
    .await;
    stub.fail_puts_with(Some(500));

    write_provider_key(fx, "k-auto-fail");
    let got = auto_events(&mut events, &[WEBDAV_EVENT, S3_EVENT]).await;
    for (name, section, root) in [
        (WEBDAV_EVENT, "webdavSync", "/dav"),
        (S3_EVENT, "s3Sync", "/bucket"),
    ] {
        let payload = &got[name].payload;
        assert_eq!(payload["source"], json!("auto"), "{name}");
        assert_eq!(payload["status"], json!("error"), "{name}");
        let error = payload["error"].as_str().expect("error text");
        assert!(error.contains("500"), "{name}: {error}");
        assert!(
            !puts(&stub, root).is_empty(),
            "{name}: the upload was tried"
        );

        // The record the settings page reads says the same, and who wrote it.
        let status = status_of(fx, section).await;
        assert_eq!(status["lastError"], json!(error), "{section}: {status}");
        assert_eq!(
            status["lastErrorSource"],
            json!("auto"),
            "{section}: {status}"
        );
        assert!(status.get("lastSyncAt").is_none(), "{section}: {status}");
    }

    // The remote recovers: the next change uploads, and the error is gone.
    stub.fail_puts_with(None);
    write_provider_key(fx, "k-auto-recovered");
    let got = auto_events(&mut events, &[WEBDAV_EVENT, S3_EVENT]).await;
    for name in [WEBDAV_EVENT, S3_EVENT] {
        assert_eq!(got[name].payload["status"], json!("success"), "{name}");
    }
    for section in ["webdavSync", "s3Sync"] {
        let status = status_of(fx, section).await;
        assert!(status.get("lastError").is_none(), "{section}: {status}");
        assert!(
            status["lastSyncAt"].as_i64().is_some(),
            "{section}: {status}"
        );
    }
}

#[tokio::test]
#[serial]
async fn nothing_uploads_while_a_backend_is_off_but_the_worker_is_alive() {
    let (fx, mut events) = begin().await;
    let stub = DavStub::start().await;
    // WebDAV: enabled but not auto. S3: auto but not enabled.
    configure(
        fx,
        webdav_body(&stub, true, false),
        s3_body(&stub, false, true),
    )
    .await;

    write_provider_key(fx, "k-auto-off");
    no_auto_events(&mut events, &[WEBDAV_EVENT, S3_EVENT]).await;
    assert!(
        stub.requests("PUT").is_empty(),
        "{:?}",
        stub.requests("PUT")
    );

    // Control: both on, same stub, and the next change uploads on both.
    configure(
        fx,
        webdav_body(&stub, true, true),
        s3_body(&stub, true, true),
    )
    .await;
    write_provider_key(fx, "k-auto-on");
    auto_events(&mut events, &[WEBDAV_EVENT, S3_EVENT]).await;
    assert!(!puts(&stub, "/dav").is_empty());
    assert!(!puts(&stub, "/bucket").is_empty());
}

/// A manual download of `backend` with only that backend on auto-sync (the suppression guard of
/// one backend does not cover the other worker):
/// - the restore really replaces the database, drifted behind the workers' back;
/// - a write through the hooked connection while the download runs is not uploaded after it
///   (the guard; the download overwrites that write anyway);
/// - nothing is uploaded afterwards, however long we wait;
/// - the next write through the same connection is uploaded: the hook survived the restore.
async fn manual_download_uploads_nothing_and_keeps_the_hook(backend: &str) {
    let (fx, mut events) = begin().await;
    // Every transfer is held for 300 ms, so the download runs for about a second.
    let stub = DavStub::start_with_delay(Duration::from_millis(300)).await;
    let (webdav_on, s3_on, event, root, remote) = match backend {
        "webdav" => (true, false, WEBDAV_EVENT, "/dav", WEBDAV_REMOTE),
        _ => (false, true, S3_EVENT, "/bucket", S3_REMOTE),
    };
    configure(
        fx,
        webdav_body(&stub, webdav_on, webdav_on),
        s3_body(&stub, s3_on, s3_on),
    )
    .await;

    // Seed the remote with the current state, by hand (an upload does not write the database).
    assert_eq!(db_key(fx, "old-provider").as_deref(), Some("k-old"));
    data(
        &fx.router,
        "POST",
        &format!("/api/sync/{backend}/upload"),
        None,
    )
    .await;
    // The local state drifts away from the remote without the worker hearing of it.
    write_provider_key_unhooked("k-drift");
    assert_eq!(db_key(fx, "old-provider").as_deref(), Some("k-drift"));
    stub.forget_requests();

    let download_uri = format!("/api/sync/{backend}/download");
    let download = data(&fx.router, "POST", &download_uri, None);
    let write_while_downloading = async {
        // The first GET is on the stub: the download holds the guard and has seconds to go.
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while stub.requests("GET").is_empty() {
            assert!(
                tokio::time::Instant::now() < deadline,
                "the download never started"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        write_provider_key(fx, "k-during-download");
    };
    let (download, ()) = tokio::join!(download, write_while_downloading);
    assert_eq!(download["status"], json!("downloaded"), "{download}");
    assert_eq!(db_key(fx, "old-provider").as_deref(), Some("k-old"));

    // Whatever the download wrote or rewrote, the worker took none of it for a change.
    no_auto_events(&mut events, &[event]).await;
    assert!(
        stub.requests("PUT").is_empty(),
        "{:?}",
        stub.requests("PUT")
    );

    // Control: the restore replaced the pages of the very connection that carries the hook,
    // and the next write through it still reaches the worker.
    write_provider_key(fx, "k-after-download");
    let got = auto_events(&mut events, &[event]).await;
    assert_eq!(got[event].payload["status"], json!("success"), "{backend}");
    assert!(!puts(&stub, root).is_empty(), "{backend}");
    assert!(text_of(&stub, &format!("{remote}/db.sql")).contains("k-after-download"));
}

#[tokio::test]
#[serial]
async fn a_manual_webdav_download_uploads_nothing_and_keeps_the_hook() {
    manual_download_uploads_nothing_and_keeps_the_hook("webdav").await;
}

#[tokio::test]
#[serial]
async fn a_manual_s3_download_uploads_nothing_and_keeps_the_hook() {
    manual_download_uploads_nothing_and_keeps_the_hook("s3").await;
}
