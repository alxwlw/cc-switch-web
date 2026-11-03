//! WEB-26: the real `examples/server` process with an `app_paths.json` override.
//!
//! The logger opens `<config dir>/logs/cc-switch-web.log` at the top of `main`,
//! before the bootstrap. If the override is loaded only by the bootstrap, the
//! log lands in `~/.cc-switch` while the database and `GET /api/logs/tail` use
//! the override directory. This test starts the example as a child process with
//! a temp HOME and checks that the log file, the database and the tail route all
//! sit under the override, and that the default directory stays untouched.
//!
//! The child gets its own `HOME` / `XDG_CONFIG_HOME` and none of
//! `CC_SWITCH_CONFIG_DIR`, `CC_SWITCH_TEST_HOME`, `RUST_LOG`: nothing from the
//! developer's environment reaches it. A guard kills and reaps it on every exit.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Kills and reaps the child however the test ends (assert, panic, early return).
struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `target/<profile>/examples/server`, built on demand: `cargo test --test <name>`
/// does not build examples.
fn server_example() -> PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("target/<profile>/deps/<test>");
    let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build
        .args(["build", "--example", "server", "--manifest-path"])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    if profile_dir.file_name().is_some_and(|n| n == "release") {
        build.arg("--release");
    }
    let status = build.status().expect("run cargo build --example server");
    assert!(status.success(), "cargo build --example server failed");
    let path = profile_dir.join("examples").join("server");
    assert!(path.exists(), "{} was not built", path.display());
    path
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind port 0");
    listener.local_addr().expect("local addr").port()
}

/// `GET path` over HTTP/1.0 (no chunking): status line and the body.
fn http_get(addr: SocketAddr, path: &str) -> std::io::Result<(String, String)> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n")?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw.as_str(), ""));
    let status = head.lines().next().unwrap_or_default().to_string();
    Ok((status, body.to_string()))
}

fn wait_until_serving(addr: SocketAddr, child: &mut Child, stderr_file: &Path) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            panic!(
                "server exited early ({status}); stderr:\n{}",
                std::fs::read_to_string(stderr_file).unwrap_or_default()
            );
        }
        if matches!(http_get(addr, "/api/logs/tail"), Ok((_, body)) if !body.is_empty()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "server did not answer in time; stderr:\n{}",
            std::fs::read_to_string(stderr_file).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn server_with_an_override_file_keeps_log_database_and_log_tail_under_it() {
    let exe = server_example();

    let root = tempfile::tempdir().expect("tempdir");
    let home = root.path().join("home");
    let xdg = root.path().join("xdg");
    let target = root.path().join("ov");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    let override_file = xdg.join("cc-switch-web").join("app_paths.json");
    std::fs::create_dir_all(override_file.parent().unwrap()).unwrap();
    std::fs::write(
        &override_file,
        serde_json::json!({ "app_config_dir_override": target.to_str().unwrap() }).to_string(),
    )
    .unwrap();

    let port = free_port();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let stderr_file = root.path().join("stderr.txt");
    let mut child = ChildGuard(
        Command::new(exe)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &xdg)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .env("WEB_DIST", root.path().join("no-dist"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&stderr_file).unwrap())
            .spawn()
            .expect("spawn examples/server"),
    );
    wait_until_serving(addr, &mut child.0, &stderr_file);

    let log_file = target.join("logs").join("cc-switch-web.log");
    let diagnostics = || std::fs::read_to_string(&stderr_file).unwrap_or_default();
    assert!(
        log_file.exists(),
        "log file must be under the override; stderr:\n{}",
        diagnostics()
    );
    let log = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        log.contains(&format!("Logging to {}", log_file.display())),
        "log file content: {log}"
    );
    assert!(
        target.join("cc-switch.db").exists(),
        "database must be under the override"
    );
    // Device-level files (`settings.json`, the live-state and catalog-history
    // files) stay in `~/.cc-switch` by design; the log and the database do not.
    let default_dir = home.join(".cc-switch");
    assert!(
        !default_dir.join("logs").exists() && !default_dir.join("cc-switch.db").exists(),
        "no log or database may be created in the default dir"
    );

    let (status, body) = http_get(addr, "/api/logs/tail").expect("GET /api/logs/tail");
    assert!(status.contains("200"), "{status}: {body}");
    let body: serde_json::Value = serde_json::from_str(&body).expect("json body");
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["data"]["path"], log_file.to_str().unwrap(), "{body}");
    assert!(
        body["data"]["lines"]
            .as_array()
            .expect("lines array")
            .iter()
            .any(|l| l.as_str().is_some_and(|l| l.contains("Logging to"))),
        "tail must return the lines of the file under the override: {body}"
    );
}
