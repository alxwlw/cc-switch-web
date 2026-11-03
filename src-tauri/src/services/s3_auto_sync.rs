use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::events::EventHub;
use serde_json::json;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use crate::error::AppError;
use crate::services::s3_sync;
use crate::services::sync_protocol::should_trigger_auto_sync_for_table;
use crate::settings::{self, S3SyncSettings};

const AUTO_SYNC_DEBOUNCE_MS: u64 = 1000;
pub(crate) const MAX_AUTO_SYNC_WAIT_MS: u64 = 10_000;

static DB_CHANGE_TX: OnceLock<Sender<String>> = OnceLock::new();
static AUTO_SYNC_SUPPRESS_DEPTH: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct AutoSyncSuppressionGuard;

impl AutoSyncSuppressionGuard {
    pub fn new() -> Self {
        AUTO_SYNC_SUPPRESS_DEPTH.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for AutoSyncSuppressionGuard {
    fn drop(&mut self) {
        let _ =
            AUTO_SYNC_SUPPRESS_DEPTH.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(value.saturating_sub(1))
            });
    }
}

pub(crate) fn is_auto_sync_suppressed() -> bool {
    AUTO_SYNC_SUPPRESS_DEPTH.load(Ordering::SeqCst) > 0
}

pub fn should_trigger_for_table(table: &str) -> bool {
    should_trigger_auto_sync_for_table(table)
}

pub(crate) fn enqueue_change_signal(tx: &Sender<String>, table: &str) -> bool {
    match tx.try_send(table.to_string()) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => false,
    }
}

pub(crate) fn auto_sync_wait_duration(started_at: Instant, now: Instant) -> Option<Duration> {
    let max_wait = Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS);
    let debounce = Duration::from_millis(AUTO_SYNC_DEBOUNCE_MS);
    let elapsed = now.saturating_duration_since(started_at);
    if elapsed >= max_wait {
        return None;
    }
    Some(debounce.min(max_wait - elapsed))
}

fn should_run_auto_sync(settings: Option<&S3SyncSettings>) -> bool {
    let Some(sync) = settings else {
        return false;
    };
    sync.enabled && sync.auto_sync
}

fn persist_auto_sync_error(settings: &mut S3SyncSettings, error: &AppError) {
    settings.status.last_error = Some(error.to_string());
    settings.status.last_error_source = Some("auto".to_string());
    let _ = settings::update_s3_sync_status(settings.status.clone());
}

fn emit_auto_sync_status_updated(events: &EventHub, status: &str, error: Option<&str>) {
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

    events.emit("s3-sync-status-updated", payload);
}

/// The settings are read inside the sync lock, not before waiting for it: a
/// worker queued behind another sync must see what that sync (or the user)
/// saved meanwhile. Reading first let it upload after auto-sync was switched
/// off, and write back a status snapshot that erased a parallel success.
async fn run_auto_sync_upload(
    db: &crate::database::Database,
    events: &EventHub,
) -> Result<(), AppError> {
    let result = s3_sync::run_with_sync_lock(async {
        let Some(mut sync_settings) =
            settings::get_s3_sync_settings().filter(|s| should_run_auto_sync(Some(s)))
        else {
            return Ok(false);
        };
        let uploaded = s3_sync::upload(db, &mut sync_settings).await;
        if let Err(err) = &uploaded {
            persist_auto_sync_error(&mut sync_settings, err);
        }
        uploaded.map(|_| true)
    })
    .await;
    match result {
        Ok(true) => {
            emit_auto_sync_status_updated(events, "success", None);
            Ok(())
        }
        Ok(false) => Ok(()),
        Err(err) => {
            emit_auto_sync_status_updated(events, "error", Some(&err.to_string()));
            Err(err)
        }
    }
}

pub fn notify_db_changed(table: &str) {
    if is_auto_sync_suppressed() {
        return;
    }
    if !should_trigger_for_table(table) {
        return;
    }
    let Some(tx) = DB_CHANGE_TX.get() else {
        return;
    };
    let _ = enqueue_change_signal(tx, table);
}

pub fn start_worker(db: Arc<crate::database::Database>, events: EventHub) {
    if DB_CHANGE_TX.get().is_some() {
        return;
    }

    // Buffer size 1 is enough: we only need "dirty" signals, not every event.
    let (tx, rx) = channel::<String>(1);
    if DB_CHANGE_TX.set(tx).is_err() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        run_worker_loop(db, rx, events).await;
    });
}

async fn run_worker_loop(
    db: Arc<crate::database::Database>,
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
            "[S3][AutoSync] Triggered by table={first_table}, merged_changes={merged_count}"
        );

        if let Err(err) = run_auto_sync_upload(&db, &events).await {
            log::warn!("[S3][AutoSync] Upload failed: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        auto_sync_wait_duration, enqueue_change_signal, is_auto_sync_suppressed,
        should_run_auto_sync, should_trigger_for_table, AutoSyncSuppressionGuard,
        MAX_AUTO_SYNC_WAIT_MS,
    };
    use crate::settings::S3SyncSettings;
    use serial_test::serial;
    use std::time::{Duration, Instant};
    use tokio::sync::mpsc::channel;

    #[test]
    fn should_trigger_sync_for_config_tables_only() {
        assert!(should_trigger_for_table("providers"));
        assert!(should_trigger_for_table("profiles"));
        assert!(should_trigger_for_table("settings"));
        assert!(!should_trigger_for_table("proxy_request_logs"));
        assert!(!should_trigger_for_table("provider_health"));
    }

    #[test]
    #[serial]
    fn suppression_guard_enables_and_restores_state() {
        assert!(!is_auto_sync_suppressed());
        {
            let _guard = AutoSyncSuppressionGuard::new();
            assert!(is_auto_sync_suppressed());
        }
        assert!(!is_auto_sync_suppressed());
    }

    #[test]
    fn max_wait_caps_flush_latency_for_continuous_events() {
        let started = Instant::now();
        let later = started + Duration::from_millis(MAX_AUTO_SYNC_WAIT_MS + 1);
        assert!(auto_sync_wait_duration(started, later).is_none());
    }

    #[tokio::test]
    async fn enqueue_change_signal_drops_when_channel_is_full() {
        let (tx, _rx) = channel::<String>(1);
        assert!(enqueue_change_signal(&tx, "providers"));
        assert!(!enqueue_change_signal(&tx, "providers"));
    }

    #[test]
    fn should_run_auto_sync_requires_enabled_and_auto_sync_flag() {
        assert!(!should_run_auto_sync(None));

        let disabled = S3SyncSettings {
            enabled: false,
            auto_sync: true,
            ..S3SyncSettings::default()
        };
        assert!(!should_run_auto_sync(Some(&disabled)));

        let auto_sync_off = S3SyncSettings {
            enabled: true,
            auto_sync: false,
            ..S3SyncSettings::default()
        };
        assert!(!should_run_auto_sync(Some(&auto_sync_off)));

        let enabled = S3SyncSettings {
            enabled: true,
            auto_sync: true,
            ..S3SyncSettings::default()
        };
        assert!(should_run_auto_sync(Some(&enabled)));
    }

    #[test]
    fn service_layer_does_not_depend_on_commands_layer() {
        let source = include_str!("s3_auto_sync.rs");
        let needle = ["crate", "commands", ""].join("::");
        assert!(
            !source.contains(&needle),
            "services layer should not depend on commands layer"
        );
    }

    #[test]
    fn worker_does_not_depend_on_the_tauri_app_handle() {
        // Built from parts so this test's own text is not what it finds.
        let needle = ["Ap", "pHandle"].join("");
        let source = include_str!("s3_auto_sync.rs");
        assert!(
            !source.contains(&needle),
            "the worker talks to the event hub, not to `{needle}`"
        );
    }
}

#[cfg(test)]
mod status_event_tests {
    use super::*;

    #[test]
    fn auto_sync_status_goes_to_the_hub_with_and_without_error() {
        let hub = EventHub::new();
        let mut rx = hub.subscribe();
        emit_auto_sync_status_updated(&hub, "success", None);
        emit_auto_sync_status_updated(&hub, "error", Some("boom"));
        let first = rx.try_recv().unwrap();
        assert_eq!(first.name, "s3-sync-status-updated");
        assert_eq!(
            first.payload,
            serde_json::json!({ "source": "auto", "status": "success" })
        );
        let second = rx.try_recv().unwrap();
        assert_eq!(second.name, "s3-sync-status-updated");
        assert_eq!(
            second.payload,
            serde_json::json!({ "source": "auto", "status": "error", "error": "boom" })
        );
    }
}

#[cfg(test)]
mod lock_order_tests {
    use super::*;
    use crate::database::Database;
    use crate::services::sync_protocol::sync_mutex;
    use crate::settings::{
        get_s3_sync_settings, set_s3_sync_settings, update_s3_sync_status, WebDavSyncStatus,
    };
    use serial_test::serial;
    use tokio::sync::broadcast::error::TryRecvError;

    /// Points the settings file at a temp home and puts `initial` into the
    /// store; clears the store and restores the env on drop.
    struct IsolatedSettings {
        previous_test_home: Option<std::ffi::OsString>,
        _dir: tempfile::TempDir,
    }

    impl IsolatedSettings {
        fn new(initial: S3SyncSettings) -> Self {
            let dir = tempfile::tempdir().expect("temp home");
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            set_s3_sync_settings(Some(initial)).expect("seed settings");
            Self {
                previous_test_home,
                _dir: dir,
            }
        }
    }

    impl Drop for IsolatedSettings {
        fn drop(&mut self) {
            let _ = set_s3_sync_settings(None);
            match &self.previous_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    /// Enabled with auto-sync on and one credential field set (an all-blank
    /// block is dropped by the store), with the bucket missing: an upload that reaches
    /// `upload()` fails at `validate()` without touching the network, so the
    /// error path is observable and deterministic.
    fn auto_sync_settings() -> S3SyncSettings {
        S3SyncSettings {
            enabled: true,
            auto_sync: true,
            region: "us-east-1".to_string(),
            ..S3SyncSettings::default()
        }
    }

    /// Starts `run_auto_sync_upload` while the global sync lock is held by the
    /// caller and lets it run until it parks on that lock (current-thread
    /// runtime: one yield is enough).
    async fn spawn_upload_behind_the_lock(
        db: Arc<Database>,
        hub: EventHub,
    ) -> tokio::task::JoinHandle<Result<(), AppError>> {
        let worker = tokio::spawn(async move { run_auto_sync_upload(&db, &hub).await });
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
        let mut off = get_s3_sync_settings().expect("seeded");
        off.auto_sync = false;
        set_s3_sync_settings(Some(off)).expect("switch auto-sync off");
        drop(lock);

        worker
            .await
            .expect("worker task")
            .expect("a switched-off auto-sync is not an error");
        assert!(
            matches!(events.try_recv(), Err(TryRecvError::Empty)),
            "no status event when nothing was uploaded"
        );
        let status = get_s3_sync_settings().expect("settings").status;
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
        update_s3_sync_status(WebDavSyncStatus {
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
        let status = get_s3_sync_settings().expect("settings").status;
        assert_eq!(status.last_sync_at, Some(1_234));
        assert_eq!(
            status.last_remote_etag.as_deref(),
            Some("etag-from-the-parallel-sync")
        );
        assert_eq!(status.last_error, Some(err.to_string()));
        assert_eq!(status.last_error_source.as_deref(), Some("auto"));
        let event = events.try_recv().expect("error event");
        assert_eq!(event.name, "s3-sync-status-updated");
        assert_eq!(event.payload["status"], "error");
        assert_eq!(event.payload["source"], "auto");
    }
}
