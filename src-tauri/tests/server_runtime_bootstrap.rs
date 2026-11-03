//! `examples/server.rs` hands its Tokio runtime to `tauri::async_runtime` before
//! anything else runs (WEB-11: the auto-sync workers spawn through it). This
//! binary is its own process, so the global runtime is still unset when
//! `set` is called; `set` panics if it is not (`runtime already initialized`).
//!
//! The example itself cannot be started from a test, so the wiring is pinned
//! twice: the runtime test repeats the example's sequence (`set`, then the
//! bootstrap through `spawn_blocking`), and the source test checks that the
//! example calls `set` before `server_log::init` and the bootstrap.

mod support;

use cc_switch_lib::bootstrap::{run_headless_bootstrap_with, CodexHistory, StepOutcome};
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_runs_after_the_server_runtime_is_set_as_the_tauri_runtime() {
    {
        // Only the fixture setup needs the lock; it must not be held across awaits.
        let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
        reset_test_fs();
        let home = ensure_test_home();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(
            home.join(".claude").join("settings.json"),
            r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-live","ANTHROPIC_BASE_URL":"https://example.invalid"}}"#,
        )
        .unwrap();
    }

    // Exactly what `examples/server.rs` does first.
    tauri::async_runtime::set(tokio::runtime::Handle::current());

    // `tauri::async_runtime` now runs on this very runtime, not on its own.
    let here = tokio::runtime::Handle::current().id();
    let there = tauri::async_runtime::spawn(async { tokio::runtime::Handle::current().id() })
        .await
        .expect("task spawned through tauri::async_runtime");
    assert_eq!(there, here);

    // A `tauri::async_runtime::block_on` reached from the blocking pool must
    // not panic on the handle-backed runtime either.
    let from_blocking =
        tokio::task::spawn_blocking(|| tauri::async_runtime::block_on(async { 7u8 }))
            .await
            .expect("block_on from the blocking pool");
    assert_eq!(from_blocking, 7);

    let boot = tokio::task::spawn_blocking(|| {
        run_headless_bootstrap_with(CodexHistory::Spawn(&|job| {
            tokio::task::spawn_blocking(job);
        }))
    })
    .await
    .expect("bootstrap task must not panic")
    .expect("bootstrap");

    assert_eq!(
        boot.report.step("codex_history"),
        Some(&StepOutcome::Deferred)
    );
    assert!(boot.report.failures().is_empty(), "{:?}", boot.report);
    let claude = boot
        .state
        .db
        .get_all_providers("claude")
        .expect("claude providers");
    assert!(!claude.is_empty(), "claude live config must be imported");
}

#[test]
fn server_example_sets_the_tauri_runtime_before_logging_and_bootstrap() {
    let source = include_str!("../examples/server.rs");
    let at = |needle: &str| {
        source
            .find(needle)
            .unwrap_or_else(|| panic!("examples/server.rs must call `{needle}`"))
    };
    let set = at("tauri::async_runtime::set(");
    assert!(
        set < at("cc_switch_lib::server_log::init("),
        "`tauri::async_runtime::set` must come before `server_log::init`"
    );
    assert!(
        set < at("run_headless_bootstrap_with("),
        "`tauri::async_runtime::set` must come before the bootstrap"
    );
    assert_eq!(
        source.matches("tauri::async_runtime::set(").count(),
        1,
        "set panics on a second call"
    );

    // Any earlier use of `tauri::async_runtime` (a spawn, a block_on, a handle)
    // initialises the global runtime and makes `set` panic at startup, so `set`
    // must be the first statement of `main`: only blank lines and comments
    // may sit between the opening brace and it.
    let main = at("async fn main(");
    let body = main + source[main..].find('{').expect("main has a body") + 1;
    assert!(body < set, "`set` must be called inside `main`");
    for line in source[body..set].lines() {
        let line = line.trim();
        assert!(
            line.is_empty() || line.starts_with("//"),
            "`tauri::async_runtime::set` must be the first statement of `main`, found `{line}` before it"
        );
    }
}
