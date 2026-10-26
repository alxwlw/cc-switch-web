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
    pub fn bridge_to_tauri(&self, app: tauri::AppHandle) {
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
pub fn emit_via_app<S: Serialize + Clone>(app: &tauri::AppHandle, name: &str, payload: S) {
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
    use serde_json::json;
    use std::sync::Mutex;
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};

    #[tokio::test]
    async fn emit_reaches_every_subscriber_in_order() {
        let hub = EventHub::new();
        let mut a = hub.subscribe();
        let mut b = hub.subscribe();
        hub.emit("first", json!({ "appType": "claude" }));
        hub.emit("second", ());
        for rx in [&mut a, &mut b] {
            assert_eq!(
                rx.recv().await.unwrap(),
                Event {
                    name: "first".into(),
                    payload: json!({ "appType": "claude" })
                }
            );
            assert_eq!(
                rx.recv().await.unwrap(),
                Event {
                    name: "second".into(),
                    payload: Value::Null
                }
            );
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
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn lagged_receiver_does_not_block_emit_and_recovers() {
        let hub = EventHub::new();
        let mut slow = hub.subscribe();
        let total = CHANNEL_CAPACITY + 10;
        for i in 0..total {
            hub.emit("tick", i);
        }
        // the sender was never blocked; the slow receiver learns how many it missed
        assert_eq!(slow.recv().await, Err(RecvError::Lagged(10)));
        // ...and keeps receiving the retained tail
        assert_eq!(slow.recv().await.unwrap().payload, json!(10));
        // a fresh emit still works
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
        // no await: the sink ran inside emit()
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
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }
}
