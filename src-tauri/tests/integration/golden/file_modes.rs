//! ① 切换后客户端目录里有哪些文件、各自的权限位。
//!
//! 重构会换掉写 live 的代码。含凭据的文件现在走 `atomic_write_private`（Unix 下 0600），
//! 换写法时一旦退回普通写入，Key 就对同机其他用户可读；多写或漏写文件（备份、状态文件、
//! 模型目录）也会在这里显形。

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::json;

use cc_switch_lib::{AppType, ProviderService};

use crate::golden::util::{
    assert_golden, home, official, provider, seed_providers, write_home_file,
};
use crate::support::{create_test_state, reset_test_fs, test_mutex};

const CLIENT_ROOTS: &[&str] = &[".claude", ".claude.json", ".codex", ".gemini", ".grok"];

/// `base` with the bits of the process umask cleared: the mode a file gets when the app creates it
/// without asking for one (`0o666` for a regular file). The umask is read by setting it, then restored at once.
fn umask_mode(base: u32) -> u32 {
    // SAFETY: `umask` only swaps the process-wide mask and cannot fail; the old value is put back below.
    let mask = unsafe {
        let mask = libc::umask(0o022);
        libc::umask(mask);
        mask
    };
    base & !(mask as u32)
}

/// The snapshots record `644` for files the app writes without an explicit mode, which is what umask 022
/// gives. Under another umask the same file has another mode, so a mode equal to the default one for this
/// umask is written as `644`. Explicit modes (`600`) are kept literally.
fn snapshot_mode(mode: u32) -> u32 {
    if mode != 0o600 && mode == umask_mode(0o666) {
        0o644
    } else {
        mode
    }
}

fn inventory() -> String {
    let home = home();
    let mut lines = Vec::new();
    for root in CLIENT_ROOTS {
        collect(&home, &home.join(root), &mut lines);
    }
    lines.sort();
    lines.join("\n") + "\n"
}

fn collect(home: &Path, path: &Path, lines: &mut Vec<String>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        for entry in std::fs::read_dir(path).expect("read dir") {
            collect(home, &entry.expect("dir entry").path(), lines);
        }
        return;
    }
    let rel = path
        .strip_prefix(home)
        .expect("under home")
        .to_string_lossy();
    lines.push(format!(
        "{rel} {:o}",
        snapshot_mode(meta.permissions().mode() & 0o777)
    ));
}

fn seed_all_providers(state: &cc_switch_lib::AppState) {
    seed_providers(
        state,
        &AppType::Claude,
        &[
            provider(
                "relay",
                json!({ "env": {
                    "ANTHROPIC_BASE_URL": "https://relay.example",
                    "ANTHROPIC_AUTH_TOKEN": "sk-relay"
                } }),
                None,
            ),
            official("claude-official", json!({ "env": {} })),
        ],
        "claude-official",
    );
    seed_providers(
        state,
        &AppType::Codex,
        &[
            provider(
                "relay",
                json!({
                    "auth": { "OPENAI_API_KEY": "sk-relay" },
                    "config": "model_provider = \"custom\"\nmodel = \"relay-model\"\n\n[model_providers.custom]\nname = \"custom\"\nbase_url = \"https://relay.example/v1\"\nwire_api = \"responses\"\n",
                    "modelCatalog": { "models": [{ "model": "relay-model" }] }
                }),
                None,
            ),
            official(
                "codex-official",
                json!({
                    "auth": { "OPENAI_API_KEY": "sk-official" },
                    "config": ""
                }),
            ),
        ],
        "codex-official",
    );
    seed_providers(
        state,
        &AppType::Gemini,
        &[
            provider(
                "relay",
                json!({
                    "env": {
                        "GEMINI_API_KEY": "g-relay",
                        "GOOGLE_GEMINI_BASE_URL": "https://relay.example"
                    },
                    "config": {}
                }),
                None,
            ),
            official("gemini-official", json!({ "env": {}, "config": {} })),
        ],
        "gemini-official",
    );
    seed_providers(
        state,
        &AppType::GrokBuild,
        &[provider(
            "relay",
            json!({ "config": "[models]\ndefault = \"grok-4.5\"\n\n[model.\"grok-4.5\"]\nmodel = \"grok-4.5\"\nbase_url = \"https://relay.example/v1\"\nname = \"Relay\"\napi_key = \"xai-relay\"\napi_backend = \"responses\"\ncontext_window = 500000\n" }),
            None,
        )],
        "relay",
    );
}

/// 官方 Codex（带 Key，auth.json 由它整份写入），再把四个应用都切到第三方。
fn switch_everything(state: &cc_switch_lib::AppState) -> (String, String) {
    ProviderService::switch(state, AppType::Codex, "codex-official").expect("codex official");
    let after_official = inventory();
    for app in [
        AppType::Claude,
        AppType::Codex,
        AppType::Gemini,
        AppType::GrokBuild,
    ] {
        ProviderService::switch(state, app, "relay").expect("switch to relay");
    }
    (after_official, inventory())
}

/// 客户端目录已存在（用户装过这些工具）、文件由 CC Switch 新建时的权限位。
#[test]
fn switch_creates_expected_files_and_modes() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    for dir in [".claude", ".codex", ".gemini", ".grok"] {
        std::fs::create_dir_all(home().join(dir)).expect("create client dir");
    }
    let state = create_test_state().expect("create test state");
    seed_all_providers(&state);

    let (after_official, after_third_party) = switch_everything(&state);

    assert_golden("modes/fresh-after-codex-official.txt", &after_official);
    assert_golden("modes/fresh-after-third-party.txt", &after_third_party);
}

/// 已有文件是 0600 时，切换后仍是 0600（替换写入不能把权限放宽）。
#[test]
fn switch_keeps_private_modes_of_existing_files() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    reset_test_fs();
    for (path, content) in [
        (".claude/settings.json", "{}"),
        (".codex/config.toml", ""),
        (".codex/auth.json", "{}"),
        (".gemini/settings.json", "{}"),
        (".gemini/.env", ""),
        (".grok/config.toml", ""),
    ] {
        let path = write_home_file(path, content);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("chmod 600");
    }
    let state = create_test_state().expect("create test state");
    seed_all_providers(&state);

    let (after_official, after_third_party) = switch_everything(&state);

    for inventory in [after_official, after_third_party] {
        for line in inventory.lines() {
            if line.ends_with("cc-switch-model-catalog.json 644") {
                continue; // CC Switch 自己新建的模型目录，不是预置文件
            }
            assert!(
                line.ends_with(" 600"),
                "a private file was loosened: {line}\n{inventory}"
            );
        }
    }
}

/// The default mode follows the umask, so the snapshots must not depend on it.
#[test]
fn umask_mode_clears_the_umask_bits() {
    let _guard = test_mutex().lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: restores the mask it replaced; the mutex keeps other tests in this binary from observing it.
    let old = unsafe { libc::umask(0o027) };
    let (file, dir, kept) = (umask_mode(0o666), umask_mode(0o777), umask_mode(0o600));
    unsafe { libc::umask(old) };
    assert_eq!(format!("{file:o}"), "640");
    assert_eq!(format!("{dir:o}"), "750");
    assert_eq!(format!("{kept:o}"), "600");
    assert_eq!(
        snapshot_mode(0o640),
        0o640,
        "only the default mode is renamed"
    );
}
