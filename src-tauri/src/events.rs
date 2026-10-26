//! Event hub: one in-process bus for backend -> frontend events.
//!
//! Desktop: the hub forwards every event synchronously to `AppHandle::emit`
//! (see [`EventHub::bridge_to_tauri`]), so event names, payloads and ordering
//! are exactly what the direct `app.emit` calls produced before.
//! Headless web: there is no AppHandle; subscribers ([`EventHub::subscribe`])
//! are the SSE stream (`GET /api/events`).
//!
//! Desktop-only events (`tray-navigate`, `update-download-progress`,
//! `tray_panel` window-targeted `emit_to`) intentionally stay on AppHandle.

use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;

/// Receivers that fall this far behind get `RecvError::Lagged` (the sender
/// never blocks and the server never fails because of a slow client).
const CHANNEL_CAPACITY: usize = 256;

/// One event on the bus: the Tauri event name plus its JSON payload
/// (`Value::Null` for events emitted with `()`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    pub name: String,
    pub payload: Value,
}

type Sink = Arc<dyn Fn(&Event) + Send + Sync>;

struct Inner {
    tx: broadcast::Sender<Event>,
    sink: RwLock<Option<Sink>>,
}

/// Cheap to clone; all clones share one bus.
#[derive(Clone)]
pub struct EventHub {
    inner: Arc<Inner>,
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

impl EventHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            inner: Arc::new(Inner {
                tx,
                sink: RwLock::new(None),
            }),
        }
    }

    /// Publish an event. Never blocks, never fails: no subscribers is fine,
    /// a serialisation error is logged and the event dropped.
    pub fn emit<S: Serialize>(&self, name: &str, payload: S) {
        match serde_json::to_value(payload) {
            Ok(payload) => self.emit_event(Event {
                name: name.to_string(),
                payload,
            }),
            Err(e) => log::warn!("emit {name}: payload serialisation failed: {e}"),
        }
    }

    fn emit_event(&self, event: Event) {
        let sink = self.inner.sink.read().ok().and_then(|s| s.clone());
        if let Some(sink) = sink {
            sink(&event);
        }
        // Err = no receivers; nothing to do.
        let _ = self.inner.tx.send(event);
    }

    /// Subscribe to events emitted from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.tx.subscribe()
    }

    /// Install the synchronous forwarder (replaces a previous one).
    pub fn set_sink(&self, sink: impl Fn(&Event) + Send + Sync + 'static) {
        if let Ok(mut slot) = self.inner.sink.write() {
            *slot = Some(Arc::new(sink));
        }
    }

    /// Desktop bridge: every event also goes to `app.emit(name, payload)`.
    pub fn bridge_to_tauri<R: tauri::Runtime>(&self, app: tauri::AppHandle<R>) {
        use tauri::Emitter;
        self.set_sink(move |event| {
            if let Err(e) = app.emit(&event.name, event.payload.clone()) {
                log::warn!("emit {} failed: {e}", event.name);
            }
        });
    }
}

/// Emit from a code path that only has an AppHandle (desktop): through the
/// managed `AppState` hub when present (so web subscribers see it too),
/// straight to the AppHandle otherwise (startup, before state is managed).
pub fn emit_via_app<R: tauri::Runtime, S: Serialize + Clone>(
    app: &tauri::AppHandle<R>,
    name: &str,
    payload: S,
) {
    use tauri::{Emitter, Manager};
    match app.try_state::<crate::store::AppState>() {
        Some(state) => state.events.emit(name, payload),
        None => {
            if let Err(e) = app.emit(name, payload) {
                log::warn!("emit {name} failed: {e}");
            }
        }
    }
}

/// Payload of `provider-switched` / `proxy-flags-changed` after a provider switch
/// (the shape `App.tsx` listens for).
pub fn provider_switched_payload(
    state: &crate::store::AppState,
    app_type: &crate::app_config::AppType,
    provider_id: &str,
) -> Value {
    serde_json::json!({
        "appType": app_type.as_str(),
        "proxyEnabled": crate::mode::current::is_proxy(app_type),
        "autoFailoverEnabled": state.db.get_proxy_flags_sync(app_type.as_str()).1,
        "providerId": provider_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::Duration;
    use tauri::{Listener, Manager};
    use tokio::sync::broadcast::error::TryRecvError;

    fn ev(name: &str, payload: Value) -> Event {
        Event {
            name: name.into(),
            payload,
        }
    }

    #[test]
    fn emit_reaches_every_subscriber_in_order() {
        let hub = EventHub::new();
        let mut a = hub.subscribe();
        let mut b = hub.subscribe();
        hub.emit("first", json!({ "appType": "claude" }));
        hub.emit("second", ());
        for rx in [&mut a, &mut b] {
            assert_eq!(
                rx.try_recv(),
                Ok(ev("first", json!({ "appType": "claude" })))
            );
            assert_eq!(rx.try_recv(), Ok(ev("second", Value::Null)));
            assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
        }
    }

    #[test]
    fn emit_without_subscribers_is_a_noop() {
        EventHub::new().emit("nobody", json!(1));
    }

    #[test]
    fn subscriber_only_sees_events_after_subscribing() {
        let hub = EventHub::new();
        hub.emit("early", ());
        let mut rx = hub.subscribe();
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }

    #[test]
    fn lagged_receiver_does_not_block_emit_and_recovers() {
        let hub = EventHub::new();
        let mut slow = hub.subscribe();
        for i in 0..CHANNEL_CAPACITY + 10 {
            hub.emit("tick", i);
        }
        // the sender was never blocked; the slow receiver learns how many it missed
        assert_eq!(slow.try_recv(), Err(TryRecvError::Lagged(10)));
        // ...and keeps receiving the retained tail
        assert_eq!(slow.try_recv().unwrap().payload, json!(10));
        hub.emit("after", ());
        let mut last = None;
        while let Ok(e) = slow.try_recv() {
            last = Some(e.name);
        }
        assert_eq!(last.as_deref(), Some("after"));
    }

    #[test]
    fn sink_gets_each_event_synchronously_and_in_order() {
        let hub = EventHub::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        hub.set_sink(move |e| {
            seen2
                .lock()
                .unwrap()
                .push((e.name.clone(), e.payload.clone()))
        });
        hub.emit("a", json!({ "x": 1 }));
        hub.emit("b", ());
        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                ("a".to_string(), json!({ "x": 1 })),
                ("b".to_string(), Value::Null)
            ]
        );
    }

    #[test]
    fn unserialisable_payload_is_dropped_not_panicking() {
        struct Bad;
        impl Serialize for Bad {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom("nope"))
            }
        }
        let hub = EventHub::new();
        let mut rx = hub.subscribe();
        hub.emit("bad", Bad);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));
    }

    /// Frontend-side view of a mock app: every `name` event payload (raw JSON).
    fn listen_on<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        name: &str,
    ) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        app.listen(name, move |e| {
            let _ = tx.send(e.payload().to_string());
        });
        rx
    }

    #[test]
    fn bridge_forwards_name_and_payload_to_the_app_handle() {
        let app = tauri::test::mock_app();
        let hub = EventHub::new();
        hub.bridge_to_tauri(app.handle().clone());
        let seen = listen_on(app.handle(), "provider-switched");
        let unit = listen_on(app.handle(), "deeplink-pending");

        hub.emit(
            "provider-switched",
            json!({ "appType": "claude", "providerId": "p" }),
        );
        hub.emit("deeplink-pending", ());

        let got = seen.recv_timeout(Duration::from_secs(2)).unwrap();
        let got: Value = serde_json::from_str(&got).unwrap();
        assert_eq!(got, json!({ "appType": "claude", "providerId": "p" }));
        assert_eq!(unit.recv_timeout(Duration::from_secs(2)).unwrap(), "null");
    }

    #[test]
    fn emit_via_app_uses_the_managed_state_hub_and_reaches_desktop_through_the_bridge() {
        let app = tauri::test::mock_app();
        let state = crate::store::AppState::new(Arc::new(Database::memory().unwrap()));
        state.events.bridge_to_tauri(app.handle().clone());
        let mut rx = state.events.subscribe();
        app.manage(state);
        let seen = listen_on(app.handle(), "profile-applied");

        emit_via_app(app.handle(), "profile-applied", json!({ "profileId": "x" }));

        assert_eq!(
            rx.try_recv(),
            Ok(ev("profile-applied", json!({ "profileId": "x" })))
        );
        assert_eq!(
            seen.recv_timeout(Duration::from_secs(2)).unwrap(),
            r#"{"profileId":"x"}"#
        );
        // exactly once on the app handle (hub bridge), not twice
        assert!(seen.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn emit_via_app_without_managed_state_emits_on_the_app_handle_directly() {
        let app = tauri::test::mock_app();
        let seen = listen_on(app.handle(), "deeplink-pending");
        emit_via_app(app.handle(), "deeplink-pending", ());
        assert_eq!(seen.recv_timeout(Duration::from_secs(2)).unwrap(), "null");
    }
}
