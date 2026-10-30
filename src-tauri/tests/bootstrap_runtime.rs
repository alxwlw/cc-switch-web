//! The headless bootstrap called the way `examples/server.rs` calls it: from
//! inside a Tokio runtime, through `spawn_blocking`, with the Codex history
//! migrations handed to the blocking pool (WEB-32). Pins the `Spawn` wiring
//! (step reported `Deferred`) and a clean run over populated live configs; it
//! does not prove protection against a nested `block_on` (no startup step
//! reaches one today, and the same run directly on the async thread passes).

mod support;

use cc_switch_lib::bootstrap::{run_headless_bootstrap_with, CodexHistory, StepOutcome};
use support::{ensure_test_home, reset_test_fs, test_mutex};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bootstrap_spawns_codex_history_and_imports_populated_live_configs() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(
        home.join(".claude").join("settings.json"),
        r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-live","ANTHROPIC_BASE_URL":"https://example.invalid"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
        home.join(".codex").join("auth.json"),
        r#"{"OPENAI_API_KEY":"sk-codex"}"#,
    )
    .unwrap();
    std::fs::write(
        home.join(".codex").join("config.toml"),
        "model_provider = \"custom\"\nmodel = \"gpt-5\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://example.invalid/v1\"\nwire_api = \"responses\"\n",
    )
    .unwrap();

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
