//! `/api/tools/*`: CLI install / update / uninstall and installation probing on the server
//! host. Lifecycle scripts are replaced through `tool_lifecycle_command_test_hook` so no real
//! installer runs; the 15-minute deadline itself is covered by the lib unit tests.
#![cfg(not(target_os = "windows"))]

mod support;

use axum::http::StatusCode;
use serde_json::{json, Value};
use serial_test::serial;
use std::path::{Path, PathBuf};
use std::time::Duration;
use support::{ensure_test_home, reset_test_fs, test_mutex, web_call, web_router};

const LIFECYCLE: &str = "/api/tools/lifecycle";
const PREVIEW: &str = "/api/tools/uninstall-preview";
const PROBE: &str = "/api/tools/probe";
const INSTALLATIONS: &str = "/api/tools/installations";

/// Clears the process-global script override even when a test panics.
struct HookReset;

impl Drop for HookReset {
    fn drop(&mut self) {
        cc_switch_lib::tool_lifecycle_command_test_hook(None);
    }
}

fn set_script(script: &str) -> HookReset {
    cc_switch_lib::tool_lifecycle_command_test_hook(Some(script.to_string()));
    HookReset
}

/// A fake executable `claude` under `$HOME/.local/bin`, a directory the installation
/// scan always searches; returns its path.
fn fake_claude(version: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = ensure_test_home().join(".local/bin");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("claude");
    let _ = std::fs::remove_file(&path);
    std::fs::write(
        &path,
        format!("#!/bin/sh\necho \"{version} (Claude Code)\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn install_body(tool: &str) -> Value {
    json!({"tools": [tool], "action": "install"})
}

async fn wait_for(path: &Path) {
    for _ in 0..600 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} never appeared", path.display());
}

#[tokio::test]
#[serial]
async fn install_runs_the_overridden_script_and_returns_null() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let ran = ensure_test_home().join("lifecycle-ran");
    let _ = std::fs::remove_file(&ran);
    let _reset = set_script(&format!("echo ok && touch '{}'", ran.display()));
    let router = web_router();

    let (status, body) = web_call(&router, "POST", LIFECYCLE, Some(install_body("claude"))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"ok": true, "data": null}));
    assert!(ran.exists(), "the overridden script did not run");
}

#[tokio::test]
#[serial]
async fn failing_script_is_a_500_carrying_its_stderr() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let _reset = set_script("echo boom >&2; exit 2");
    let router = web_router();

    let (status, body) = web_call(&router, "POST", LIFECYCLE, Some(install_body("claude"))).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["ok"], json!(false));
    assert_eq!(body["error"], json!("boom"));
}

#[tokio::test]
#[serial]
async fn second_install_of_the_same_tool_gets_tool_action_in_progress() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let home = ensure_test_home();
    let marker = home.join("lifecycle-started");
    let release = home.join("lifecycle-release");
    let _ = std::fs::remove_file(&marker);
    let _ = std::fs::remove_file(&release);
    // Holds the lifecycle lock until released; bounded so a failed test cannot hang.
    let _reset = set_script(&format!(
        "touch '{}'; i=0; while [ ! -f '{}' ] && [ $i -lt 300 ]; do sleep 0.1; i=$((i+1)); done",
        marker.display(),
        release.display()
    ));
    let router = web_router();

    let first = {
        let router = router.clone();
        tokio::spawn(async move {
            web_call(&router, "POST", LIFECYCLE, Some(install_body("claude"))).await
        })
    };
    wait_for(&marker).await;

    let (status, body) = web_call(&router, "POST", LIFECYCLE, Some(install_body("claude"))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"], json!("TOOL_ACTION_IN_PROGRESS"));

    std::fs::write(&release, "").unwrap();
    let (status, body) = first.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, json!({"ok": true, "data": null}));
}

#[tokio::test]
#[serial]
async fn uninstall_without_a_plan_is_refused() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        LIFECYCLE,
        Some(json!({"tools": ["claude"], "action": "uninstall"})),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(
        body["error"],
        json!("Preview the uninstall plan before confirming.")
    );
}

#[tokio::test]
#[serial]
async fn unknown_tools_are_refused_by_lifecycle_and_probe() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, body) = web_call(&router, "POST", LIFECYCLE, Some(install_body("nope"))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"], json!("No supported tools selected"));

    for uri in [PROBE, INSTALLATIONS] {
        let (status, body) = web_call(&router, "POST", uri, Some(json!({"tools": ["nope"]}))).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{uri}: {body}");
        assert_eq!(body["error"], json!("No supported tools selected"), "{uri}");
    }

    let (status, body) = web_call(&router, "POST", PREVIEW, Some(json!({"tool": "nope"}))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"], json!("Unsupported tool action target: nope"));
}

#[tokio::test]
#[serial]
async fn probe_and_installations_list_the_fake_claude_with_its_version() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let fake = fake_claude("7.8.9");
    let router = web_router();

    for uri in [PROBE, INSTALLATIONS] {
        let (status, body) = web_call(
            &router,
            "POST",
            uri,
            Some(json!({"tools": ["claude", "nope"]})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        let reports = body["data"].as_array().expect("report list");
        assert_eq!(reports.len(), 1, "unknown tool must be filtered: {body}");
        assert_eq!(reports[0]["tool"], json!("claude"));
        let installs = reports[0]["installs"].as_array().unwrap();
        let ours = installs
            .iter()
            .find(|i| i["path"] == json!(fake.to_string_lossy()))
            .unwrap_or_else(|| panic!("{uri}: fake claude not listed: {body}"));
        assert_eq!(ours["version"], json!("7.8.9"));
        assert_eq!(ours["runnable"], json!(true));
    }
}

#[tokio::test]
#[serial]
async fn uninstall_preview_offers_a_plan_for_the_fake_claude() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let fake = fake_claude("7.8.9");
    let router = web_router();

    let (status, body) = web_call(&router, "POST", PREVIEW, Some(json!({"tool": "claude"}))).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let candidates = body["data"].as_array().expect("candidate list");
    let ours = candidates
        .iter()
        .find(|c| c["path"] == json!(fake.to_string_lossy()))
        .unwrap_or_else(|| panic!("fake claude not offered: {body}"));
    assert_eq!(ours["version"], json!("7.8.9"));
    // Exactly one of the two is set: a runnable plan, or the reason it cannot be one.
    assert!(
        ours["plan"].is_object() != ours["unavailable_reason"].is_string(),
        "{ours}"
    );
}

fn uninstall_body(plan: Value) -> Value {
    json!({"tools": ["claude"], "action": "uninstall", "uninstallPlan": plan})
}

fn plan_for(tool: &str, path: &str, command: &str) -> Value {
    json!({
        "tool": tool, "path": path, "real_path": path, "command": command,
        "native": false, "wsl_distro": null,
    })
}

#[tokio::test]
#[serial]
async fn uninstall_plan_for_another_tool_is_refused() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();

    let (status, body) = web_call(
        &router,
        "POST",
        LIFECYCLE,
        Some(uninstall_body(plan_for("codex", "/x", "true"))),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(
        body["error"],
        json!("The uninstall plan does not match the selected tool.")
    );
}

#[tokio::test]
#[serial]
async fn uninstall_plan_for_a_missing_installation_is_refused() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    let router = web_router();
    let missing = ensure_test_home().join("no-such-dir/claude");

    let (status, body) = web_call(
        &router,
        "POST",
        LIFECYCLE,
        Some(uninstall_body(plan_for(
            "claude",
            &missing.to_string_lossy(),
            "true",
        ))),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Installation changed since the uninstall preview"),
        "{body}"
    );
}

#[tokio::test]
#[serial]
async fn tampered_uninstall_command_is_rederived_and_never_executed() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    // Shaped like the native installer (ELF behind a symlink into share/claude/versions)
    // so a plan is offered. The ELF is a copy of /bin/true, only probed with --version.
    let home = ensure_test_home();
    let versions = home.join(".local/share/claude/versions");
    std::fs::create_dir_all(&versions).unwrap();
    let real = versions.join("7.8.9");
    let _ = std::fs::remove_file(&real);
    std::fs::copy("/bin/true", &real).unwrap();
    let stub = home.join(".local/bin/claude");
    std::fs::create_dir_all(stub.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&stub);
    std::os::unix::fs::symlink(&real, &stub).unwrap();
    let fake = stub;
    let marker = home.join("tampered-ran");
    let _ = std::fs::remove_file(&marker);
    let router = web_router();

    let (_, preview) = web_call(&router, "POST", PREVIEW, Some(json!({"tool": "claude"}))).await;
    let mut plan = preview["data"]
        .as_array()
        .and_then(|c| {
            c.iter()
                .find(|c| c["path"] == json!(fake.to_string_lossy()))
        })
        .and_then(|c| c.get("plan").cloned())
        .filter(Value::is_object)
        .unwrap_or_else(|| panic!("no plan offered for the fake claude: {preview}"));
    plan["command"] = json!(format!("touch '{}'", marker.display()));

    let (status, body) = web_call(&router, "POST", LIFECYCLE, Some(uninstall_body(plan))).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Installation changed since the uninstall preview"),
        "{body}"
    );
    assert!(!marker.exists(), "the client-supplied command ran");
}
