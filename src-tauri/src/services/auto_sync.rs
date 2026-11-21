//! Background auto sync shared by the WebDAV and S3 backends: local DB
//! changes are debounced, then the snapshot is uploaded under the global
//! sync lock. Each backend supplies its settings and upload call through
//! [`AutoSyncBackend`].

use std::future::Future;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::database::Database;
use crate::error::AppError;
use crate::events::EventHub;
use crate::services::sync_protocol::{run_with_sync_lock, should_trigger_auto_sync_for_table};
use crate::settings::WebDavSyncStatus;

const AUTO_SYNC_DEBOUNCE_MS: u64 = 1000;
const MAX_AUTO_SYNC_WAIT_MS: u64 = 10_000;

pub(crate) trait AutoSyncBackend: Send + Sync + 'static {
    type Settings: Send + 'static;
    /// Log prefix, e.g. `WebDAV`.
    const NAME: &'static str;
    /// Event the frontend listens to for auto sync results.
    const STATUS_EVENT: &'static str;

    fn state() -> &'static AutoSyncState;
    fn load() -> Option<Self::Settings>;
    fn wants_auto_sync(settings: &Self::Settings) -> bool;
    fn status_mut(settings: &mut Self::Settings) -> &mut WebDavSyncStatus;
    fn store_status(status: WebDavSyncStatus) -> Result<(), AppError>;
    fn upload<'a>(
        db: &'a Database,
        settings: &'a mut Self::Settings,
    ) -> impl Future<Output = Result<Value, AppError>> + Send + 'a;
}

/// Per-backend worker state.
pub(crate) struct AutoSyncState {
    tx: OnceLock<Sender<String>>,
    suppress_depth: AtomicUsize,
}

impl AutoSyncState {
    pub(crate) const fn new() -> Self {
        Self {
            tx: OnceLock::new(),
            suppress_depth: AtomicUsize::new(0),
        }
    }
}

/// While alive, DB changes don't queue an auto upload for backend `B`.
pub(crate) struct SuppressionGuard<B: AutoSyncBackend>(PhantomData<B>);

impl<B: AutoSyncBackend> SuppressionGuard<B> {
    pub fn new() -> Self {
        B::state().suppress_depth.fetch_add(1, Ordering::SeqCst);
        Self(PhantomData)
    }
}

impl<B: AutoSyncBackend> Drop for SuppressionGuard<B> {
    fn drop(&mut self) {
        let _ =
            B::state()
                .suppress_depth
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    Some(value.saturating_sub(1))
                });
    }
}

pub(crate) fn is_suppressed<B: AutoSyncBackend>() -> bool {
    B::state().suppress_depth.load(Ordering::SeqCst) > 0
}

fn enqueue_change_signal(tx: &Sender<String>, table: &str) -> bool {
    match tx.try_send(table.to_string()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => false,
    }
}

fn auto_sync_wait_duration(started_at: Instant, now: Instant) -> Option<Duration> {
    let max_wait = Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS);
    let debounce = Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS);
    let elapsed = now.saturating_duration_since(started_at);
    if elapsed >= max_wait {
        return None;
    }
    Some(debounce.min(max_wait - elapsed))
}

fn load_if_auto_sync<B: AutoSyncBackend>() -> Option<B::Settings> {
    B::load().filter(B::wants_auto_sync)
}

fn persist_auto_sync_error<B: AutoSyncBackend>(settings: &mut B::Settings, error: &AppError) {
    let status = B::status_mut(settings);
    status.last_error = Some(error.to_string());
    status.last_error_source = Some("auto".to_string());
    let _ = B::store_status(status.clone());
}

fn emit_auto_sync_status_updated<B: AutoSyncBackend>(
    events: &EventHub,
    status: &str,
    error: Option<&str>,
) {
    let payload = match error {
        Some(message) => json!({
            "source": "auto",
            "status": status,
            "error": message,
        }),
        None => json!({
            "source": "auto",
            "status": status,
        }),
    };

    events.emit(B::STATUS_EVENT, payload);
}

/// The settings are read inside the sync lock, not before waiting for it: a
/// worker queued behind another sync must see what that sync (or the user)
/// saved meanwhile. Reading first let it upload after auto-sync was switched
/// off, and write back a status snapshot that erased a parallel success.
async fn run_auto_sync_upload<B: AutoSyncBackend>(
    db: &Database,
    events: &EventHub,
) -> Result<(), AppError> {
    if load_if_auto_sync::<B>().is_none() {
        return Ok(());
    }
    let result = run_with_sync_lock(async {
        let Some(mut sync_settings) = load_if_auto_sync::<B>() else {
            return Ok(false);
        };
        let uploaded = B::upload(db, &mut sync_settings).await;
        if let Err(err) = &uploaded {
            persist_auto_sync_error::<B>(&mut sync_settings, err);
        }
        uploaded.map(|_| true)
    })
    .await;
    match result {
        Ok(true) => {
            emit_auto_sync_status_updated::<B>(events, "success", None);
            Ok(())
        }
        Ok(false) => Ok(()),
        Err(err) => {
            emit_auto_sync_status_updated::<B>(events, "error", Some(&err.to_string()));
            Err(err)
        }
    }
}

pub fn notify_db_changed<B: AutoSyncBackend>(table: &str) {
    if is_suppressed::<B>() {
        return;
    }
    if !should_trigger_auto_sync_for_table(table) {
        return;
    }
    let Some(tx) = B::state().tx.get() else {
        return;
    };
    let _ = enqueue_change_signal(tx, table);
}

pub fn start_worker<B: AutoSyncBackend>(db: Arc<Database>, events: EventHub) {
    if B::state().tx.get().is_some() {
        return;
    }

    // Buffer size 1 is enough: we only need "dirty" signals, not every event.
    let (tx, rx) = channel::<String>(1);
    if B::state().tx.set(tx).is_err() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        run_worker_loop::<B>(db, rx, events).await;
    });
}

async fn run_worker_loop<B: AutoSyncBackend>(
    db: Arc<Database>,
    mut rx: Receiver<String>,
    events: EventHub,
) {
    while let Some(first_table) = rx.recv().await {
        let started_at = Instant::now();
        let mut merged_count = 1usize;

        while let Some(wait_for) = auto_sync_wait_duration(started_at, Instant::now()) {
            let timeout = tokio::time::timeout(wait_for, rx.recv()).await;

            match timeout {
                Ok(Some(_)) => merged_count += 1,
                Ok(None) => return,
                Err(_) => break,
            }
        }

        log::debug!(
            "[{}][AutoSync] Triggered by table={first_table}, merged_changes={merged_count}",
            B::NAME
        );

        if let Err(err) = run_auto_sync_upload::<B>(&db, &events).await {
            log::warn!("[{}][AutoSync] Upload failed: {err}", B::NAME);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        auto_sync_wait_duration, run_worker_loop, AutoSyncBackend, AUTO_SYNC_DEBOUNCE_MS,
        MAX_AUTO_SYNC_WAIT_MS,
    };
    use crate::database::Database;
    use crate::error::AppError;
    use crate::events::EventHub;
    use crate::services::s3_auto_sync::S3AutoSync;
    use crate::services::sync_protocol::sync_mutex;
    use crate::services::webdav_auto_sync::WebDavAutoSync;
    use crate::settings::{self, AppSettings, S3SyncSettings, WebDavSyncSettings};
    use crate::test_support::EnvVars;
    use serial_test::serial;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc::channel;

    #[test]
    fn max_wait_caps_flush_latency_for_continuous_events() {
        let started = Instant::now();
        let later = started + Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS + 1);
        assert!(auto_sync_wait_duration(started, later).is_none());
    }

    #[test]
    fn worker_does_not_depend_on_the_tauri_app_handle() {
        // Built from parts so this test's own text is not what it finds.
        let needle = ["Ap", "pHandle"].join("");
        for source in [
            include_str!("auto_sync.rs"),
            include_str!("webdav_auto_sync.rs"),
            include_str!("s3_auto_sync.rs"),
        ] {
            assert!(
                !source.contains(&needle),
                "the worker talks to the event hub, not to `{needle}`"
            );
        }
    }

    #[test]
    fn auto_sync_status_goes_to_the_hub_with_and_without_error() {
        for (hub_event, emit) in [
            (
                "webdav-sync-status-updated",
                super::emit_auto_sync_status_updated::<WebDavAutoSync>
                    as fn(&EventHub, &str, Option<&str>),
            ),
            (
                S3AutoSync::STATUS_EVENT,
                super::emit_auto_sync_status_updated::<S3AutoSync>,
            ),
        ] {
            let hub = EventHub::new();
            let mut rx = hub.subscribe();
            emit(&hub, "success", None);
            emit(&hub, "error", Some("boom"));
            let first = rx.try_recv().unwrap();
            assert_eq!(first.name, hub_event);
            assert_eq!(
                first.payload,
                serde_json::json!({ "source": "auto", "status": "success" })
            );
            let second = rx.try_recv().unwrap();
            assert_eq!(second.name, hub_event);
            assert_eq!(
                second.payload,
                serde_json::json!({ "source": "auto", "status": "error", "error": "boom" })
            );
        }
    }

    #[test]
    fn service_layer_does_not_depend_on_commands_layer() {
        let needle = ["crate", "commands", ""].join("::");
        for source in [
            include_str!("auto_sync.rs"),
            include_str!("webdav_auto_sync.rs"),
            include_str!("s3_auto_sync.rs"),
        ] {
            assert!(
                !source.contains(&needle),
                "services layer should not depend on commands layer"
            );
        }
    }

    /// A remote that answers 200 to every request and counts them.
    fn counting_remote() -> (String, Arc<AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local listener");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_in_thread = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                hits_in_thread.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
                let mut body_len = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        body_len = value.trim().parse().unwrap_or(0);
                    }
                }
                let _ = reader.read_exact(&mut vec![0; body_len]);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            }
        });
        (url, hits)
    }

    fn webdav_settings(remote: &str, enabled: bool, auto_sync: bool) -> WebDavSyncSettings {
        WebDavSyncSettings {
            enabled,
            auto_sync,
            base_url: remote.to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        }
    }

    fn s3_settings(remote: &str, enabled: bool, auto_sync: bool) -> S3SyncSettings {
        S3SyncSettings {
            enabled,
            auto_sync,
            region: "us-east-1".to_string(),
            bucket: "bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            endpoint: remote.to_string(),
            ..S3SyncSettings::default()
        }
    }

    /// What the user does while the auto upload waits for the sync lock.
    enum UserChange {
        Nothing,
        /// Saves the same config with these `(enabled, auto_sync)` flags.
        Save(bool, bool),
        /// Clears the saved config.
        Clear,
    }

    /// Points settings at a fresh home for one test and puts the old one back.
    struct IsolatedHome {
        _env: EnvVars,
        _dir: tempfile::TempDir,
    }

    impl IsolatedHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create test home");
            let env = EnvVars::new().set("CC_SWITCH_TEST_HOME", dir.path());
            settings::update_settings(AppSettings::default()).expect("reset settings");
            Self {
                _env: env,
                _dir: dir,
            }
        }
    }

    impl Drop for IsolatedHome {
        fn drop(&mut self) {
            let _ = settings::update_settings(AppSettings::default());
        }
    }

    /// A DB change queues an auto upload while another sync operation holds
    /// the lock. While it waits, the user makes `change`. Returns how many
    /// requests reached the remote.
    async fn requests_after_queued_upload<B: AutoSyncBackend>(
        store: fn(Option<B::Settings>) -> Result<(), AppError>,
        settings_for: fn(&str, bool, bool) -> B::Settings,
        change: UserChange,
    ) -> usize {
        let _home = IsolatedHome::new();
        let (remote, hits) = counting_remote();
        store(Some(settings_for(&remote, true, true))).expect("seed settings");

        let db = Arc::new(Database::memory().expect("memory db"));
        let hub = EventHub::new();
        let (tx, rx) = channel::<String>(1);

        let other_sync = sync_mutex().lock().await;
        let worker = tokio::spawn(run_worker_loop::<B>(db, rx, hub));
        tx.send("providers".to_string())
            .await
            .expect("queue change");
        // Past the debounce, so the upload is queued on the sync lock.
        tokio::time::sleep(Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS + 300)).await;

        match change {
            UserChange::Nothing => {}
            UserChange::Save(enabled, auto_sync) => {
                store(Some(settings_for(&remote, enabled, auto_sync))).expect("save settings")
            }
            UserChange::Clear => store(None).expect("clear settings"),
        }
        drop(other_sync);
        drop(tx);
        tokio::time::timeout(Duration::from_secs(10), worker)
            .await
            .expect("worker should finish")
            .expect("worker should not panic");
        hits.load(Ordering::SeqCst)
    }

    macro_rules! queued_upload_tests {
        ($name:ident, $backend:ty, $store:path, $settings:path) => {
            mod $name {
                use super::*;

                async fn requests(change: UserChange) -> usize {
                    requests_after_queued_upload::<$backend>($store, $settings, change).await
                }

                #[tokio::test]
                #[serial]
                async fn queued_upload_runs_when_settings_are_unchanged() {
                    let requests = requests(UserChange::Nothing).await;
                    assert!(requests > 0, "the queued upload should reach the remote");
                }

                #[tokio::test]
                #[serial]
                async fn queued_upload_is_dropped_after_auto_sync_is_turned_off() {
                    let requests = requests(UserChange::Save(true, false)).await;
                    assert_eq!(
                        requests, 0,
                        "nothing may be uploaded after auto sync is off"
                    );
                }

                #[tokio::test]
                #[serial]
                async fn queued_upload_is_dropped_after_sync_is_disabled() {
                    let requests = requests(UserChange::Save(false, true)).await;
                    assert_eq!(
                        requests, 0,
                        "nothing may be uploaded after sync is disabled"
                    );
                }

                #[tokio::test]
                #[serial]
                async fn queued_upload_is_dropped_after_config_is_cleared() {
                    let requests = requests(UserChange::Clear).await;
                    assert_eq!(
                        requests, 0,
                        "nothing may be uploaded after the config is cleared"
                    );
                }
            }
        };
    }

    queued_upload_tests!(
        webdav,
        WebDavAutoSync,
        settings::set_webdav_sync_settings,
        webdav_settings
    );
    queued_upload_tests!(s3, S3AutoSync, settings::set_s3_sync_settings, s3_settings);
}

#[cfg(test)]
mod lock_order_tests {
    use super::{run_auto_sync_upload, AutoSyncBackend};
    use crate::database::Database;
    use crate::error::AppError;
    use crate::events::EventHub;
    use crate::services::sync_protocol::sync_mutex;
    use crate::services::webdav_auto_sync::WebDavAutoSync;
    use crate::settings::{
        get_webdav_sync_settings, set_webdav_sync_settings, update_webdav_sync_status,
        WebDavSyncSettings, WebDavSyncStatus,
    };
    use serial_test::serial;
    use std::sync::Arc;
    use tokio::sync::broadcast::error::TryRecvError;

    /// Points the settings file at a temp home and puts `initial` into the
    /// store; clears the store and restores the env on drop.
    struct IsolatedSettings {
        previous_test_home: Option<std::ffi::OsString>,
        _dir: tempfile::TempDir,
    }

    impl IsolatedSettings {
        fn new(initial: WebDavSyncSettings) -> Self {
            let dir = tempfile::tempdir().expect("temp home");
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            set_webdav_sync_settings(Some(initial)).expect("seed settings");
            Self {
                previous_test_home,
                _dir: dir,
            }
        }
    }

    impl Drop for IsolatedSettings {
        fn drop(&mut self) {
            let _ = set_webdav_sync_settings(None);
            match &self.previous_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    /// Enabled with auto-sync on and one credential field set (an all-blank
    /// block is dropped by the store), with the username missing: an upload
    /// that reaches `upload()` fails at `validate()` without touching the
    /// network, so the error path is observable and deterministic.
    fn auto_sync_settings() -> WebDavSyncSettings {
        WebDavSyncSettings {
            enabled: true,
            auto_sync: true,
            base_url: "http://127.0.0.1:1".to_string(),
            ..WebDavSyncSettings::default()
        }
    }

    /// Starts `run_auto_sync_upload` while the global sync lock is held by the
    /// caller and lets it run until it parks on that lock (current-thread
    /// runtime: one yield is enough).
    async fn spawn_upload_behind_the_lock(
        db: Arc<Database>,
        hub: EventHub,
    ) -> tokio::task::JoinHandle<Result<(), AppError>> {
        let worker =
            tokio::spawn(async move { run_auto_sync_upload::<WebDavAutoSync>(&db, &hub).await });
        tokio::task::yield_now().await;
        assert!(!worker.is_finished(), "the worker must wait for the lock");
        worker
    }

    #[tokio::test]
    #[serial]
    async fn upload_queued_behind_the_lock_sees_auto_sync_switched_off_meanwhile() {
        let _settings = IsolatedSettings::new(auto_sync_settings());
        let db = Arc::new(Database::memory().expect("memory db"));
        let hub = EventHub::new();
        let mut events = hub.subscribe();

        let lock = sync_mutex().lock().await;
        let worker = spawn_upload_behind_the_lock(db, hub.clone()).await;
        let mut off = get_webdav_sync_settings().expect("seeded");
        off.auto_sync = false;
        set_webdav_sync_settings(Some(off)).expect("switch auto-sync off");
        drop(lock);

        worker
            .await
            .expect("worker task")
            .expect("a switched-off auto-sync is not an error");
        assert!(
            matches!(events.try_recv(), Err(TryRecvError::Empty)),
            "no status event when nothing was uploaded"
        );
        let status = get_webdav_sync_settings().expect("settings").status;
        assert_eq!(status.last_error, None);
        assert_eq!(status.last_error_source, None);
    }

    #[tokio::test]
    #[serial]
    async fn failed_upload_keeps_the_status_a_parallel_sync_saved_while_it_waited() {
        let _settings = IsolatedSettings::new(auto_sync_settings());
        let db = Arc::new(Database::memory().expect("memory db"));
        let hub = EventHub::new();
        let mut events = hub.subscribe();

        let lock = sync_mutex().lock().await;
        let worker = spawn_upload_behind_the_lock(db, hub.clone()).await;
        // A manual sync that held the lock before us finishes and records success.
        update_webdav_sync_status(WebDavSyncStatus {
            last_sync_at: Some(1_234),
            last_remote_etag: Some("etag-from-the-parallel-sync".to_string()),
            ..WebDavSyncStatus::default()
        })
        .expect("record the parallel success");
        drop(lock);

        let err = worker
            .await
            .expect("worker task")
            .expect_err("upload fails validation");
        let status = get_webdav_sync_settings().expect("settings").status;
        assert_eq!(status.last_sync_at, Some(1_234));
        assert_eq!(
            status.last_remote_etag.as_deref(),
            Some("etag-from-the-parallel-sync")
        );
        assert_eq!(status.last_error, Some(err.to_string()));
        assert_eq!(status.last_error_source.as_deref(), Some("auto"));
        let event = events.try_recv().expect("error event");
        assert_eq!(event.name, WebDavAutoSync::STATUS_EVENT);
        assert_eq!(event.payload["status"], "error");
        assert_eq!(event.payload["source"], "auto");
    }
}
