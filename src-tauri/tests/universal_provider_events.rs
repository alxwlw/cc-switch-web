use serde_json::json;

use cc_switch_lib::{
    delete_universal_provider_emit, sync_universal_provider_emit, upsert_universal_provider_emit,
    UniversalProvider,
};

#[path = "support.rs"]
mod support;
use support::{create_test_state, ensure_test_home, reset_test_fs, test_mutex};

fn drain(
    rx: &mut tokio::sync::broadcast::Receiver<cc_switch_lib::events::Event>,
) -> Vec<(String, serde_json::Value)> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        out.push((ev.name, ev.payload));
    }
    out
}

#[test]
fn universal_provider_helpers_emit_on_success_only() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    ensure_test_home();
    let state = create_test_state().expect("state");
    let mut rx = state.events.subscribe();

    let provider = UniversalProvider::new(
        "up-1".into(),
        "Relay".into(),
        "custom".into(),
        "https://api.example".into(),
        "key".into(),
    );
    assert!(upsert_universal_provider_emit(&state, provider).unwrap());
    assert_eq!(
        drain(&mut rx),
        vec![(
            "universal-provider-synced".to_string(),
            json!({"action": "upsert", "id": "up-1"})
        )]
    );

    assert!(sync_universal_provider_emit(&state, "up-1").unwrap());
    assert_eq!(
        drain(&mut rx),
        vec![(
            "universal-provider-synced".to_string(),
            json!({"action": "sync", "id": "up-1"})
        )]
    );

    // failure: sync of a missing id emits nothing
    assert!(sync_universal_provider_emit(&state, "missing").is_err());
    assert!(drain(&mut rx).is_empty());

    assert!(delete_universal_provider_emit(&state, "up-1").unwrap());
    assert_eq!(
        drain(&mut rx),
        vec![(
            "universal-provider-synced".to_string(),
            json!({"action": "delete", "id": "up-1"})
        )]
    );
}
