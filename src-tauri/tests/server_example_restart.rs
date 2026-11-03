//! WEB-26: `POST /api/restart` against the real `examples/server` process.
//!
//! The route tests (`web_api_restart.rs`) inject a handle; here the process itself is started
//! with its own HOME, a free port and an environment built from nothing (`env_clear` plus an
//! explicit list), so neither the developer's `INVOCATION_ID` nor a running service reaches
//! it. Pinned: under a supervisor the 202 comes back and the process exits with 75, also while
//! an `/api/events` stream is open (the drain timer branch of `main`); SIGTERM still exits with
//! 0; without a supervisor the answer is 409 and the process keeps serving.
//!
//! Not covered here: a second restart request during the shutdown. The listener is closed by
//! then and the connection is refused, so the double-request case is pinned on the handle
//! (`web_api_restart.rs`).

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

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

/// One request over HTTP/1.0 (no chunking, the server closes after the response): status line
/// and body. `body` is sent as JSON with a Content-Length.
fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
) -> std::io::Result<(String, String)> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    match body {
        Some(body) => write!(
            stream,
            "{method} {path} HTTP/1.0\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )?,
        None => write!(stream, "{method} {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n")?,
    }
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw.as_str(), ""));
    Ok((
        head.lines().next().unwrap_or_default().to_string(),
        body.to_string(),
    ))
}

struct Server {
    child: ChildGuard,
    addr: SocketAddr,
    stderr_file: PathBuf,
    // Keeps HOME and the log directory alive for the test.
    _root: tempfile::TempDir,
}

impl Server {
    /// `supervised`: whether `CC_SWITCH_WEB_SUPERVISED=1` is in the child's environment.
    fn start(supervised: bool) -> Self {
        let exe = server_example();
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        let xdg = root.path().join("xdg");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&xdg).unwrap();
        let port = free_port();
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let stderr_file = root.path().join("stderr.txt");
        let mut command = Command::new(exe);
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &xdg)
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string())
            .env("WEB_DIST", root.path().join("no-dist"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&stderr_file).unwrap());
        if supervised {
            command.env("CC_SWITCH_WEB_SUPERVISED", "1");
        }
        let child = ChildGuard(command.spawn().expect("spawn examples/server"));
        let mut server = Self {
            child,
            addr,
            stderr_file,
            _root: root,
        };
        server.wait_until_serving();
        server
    }

    fn diagnostics(&self) -> String {
        std::fs::read_to_string(&self.stderr_file).unwrap_or_default()
    }

    fn wait_until_serving(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(status) = self.child.0.try_wait().expect("try_wait") {
                panic!(
                    "server exited early ({status}); stderr:\n{}",
                    self.diagnostics()
                );
            }
            if matches!(http(self.addr, "GET", "/api/health", None), Ok((s, _)) if s.contains("200"))
            {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "server did not answer in time; stderr:\n{}",
                self.diagnostics()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn instance_id(&self) -> String {
        let (status, body) = http(self.addr, "GET", "/api/health", None).expect("GET /api/health");
        assert!(status.contains("200"), "{status}: {body}");
        let body: Value = serde_json::from_str(&body).expect("health json");
        assert_eq!(body["status"], "ok", "{body}");
        body["instanceId"]
            .as_str()
            .expect("instanceId is a string")
            .to_string()
    }

    fn post_restart(&self) -> (String, Value) {
        let (status, body) =
            http(self.addr, "POST", "/api/restart", Some("{}")).expect("POST /api/restart");
        let json = serde_json::from_str(&body).unwrap_or(Value::Null);
        (status, json)
    }

    /// Polls for the exit of the process (a sleep between `try_wait`s, not a fixed wait).
    fn wait_for_exit(&mut self, within: Duration) -> (ExitStatus, Duration) {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.0.try_wait().expect("try_wait") {
                return (status, started.elapsed());
            }
            assert!(
                started.elapsed() < within,
                "server still running {} s later; stderr:\n{}",
                within.as_secs(),
                self.diagnostics()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
fn restart_under_a_supervisor_answers_202_with_the_instance_id_and_exits_75() {
    let mut server = Server::start(true);
    let instance = server.instance_id();
    assert_eq!(server.instance_id(), instance, "stable inside the process");

    let (status, body) = server.post_restart();

    assert!(status.contains("202"), "{status}: {body}");
    assert_eq!(body["ok"], true, "{body}");
    assert_eq!(body["data"]["instanceId"], instance.as_str(), "{body}");
    let (exit, _) = server.wait_for_exit(Duration::from_secs(40));
    assert_eq!(
        exit.code(),
        Some(75),
        "{exit}; stderr:\n{}",
        server.diagnostics()
    );
    let log = server.diagnostics();
    assert!(log.contains("Restart requested, shutting down"), "{log}");
    assert!(
        !log.contains("Open connections did not drain"),
        "no stream was open, the serve future must end by itself: {log}"
    );
}

#[test]
fn restart_with_an_open_event_stream_still_exits_75_after_the_drain_timeout() {
    let mut server = Server::start(true);
    let instance = server.instance_id();

    // `/api/events` never ends by itself: the serve future cannot finish, so the exit comes
    // from the drain timer branch of `select!`.
    let mut stream = TcpStream::connect_timeout(&server.addr, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET /api/events HTTP/1.1\r\nHost: {}\r\nAccept: text/event-stream\r\n\r\n",
        server.addr
    )
    .unwrap();
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).expect("SSE response head");
    assert_eq!(&head, b"HTTP/1.1 200", "events stream must be open");

    let (status, body) = server.post_restart();

    assert!(status.contains("202"), "{status}: {body}");
    assert_eq!(body["data"]["instanceId"], instance.as_str(), "{body}");
    let (exit, elapsed) = server.wait_for_exit(Duration::from_secs(40));
    assert_eq!(
        exit.code(),
        Some(75),
        "{exit}; stderr:\n{}",
        server.diagnostics()
    );
    let log = server.diagnostics();
    assert!(
        log.contains("Open connections did not drain"),
        "the drain timer branch must have ended the wait: {log}"
    );
    assert!(
        elapsed >= Duration::from_secs(3),
        "left after {elapsed:?}, before the drain timeout could have fired"
    );
    drop(stream);
}

#[test]
fn sigterm_without_a_restart_request_still_exits_0() {
    let mut server = Server::start(true);

    // SAFETY: plain `kill(2)` on the pid of our own child.
    let rc = unsafe { libc::kill(server.child.0.id() as i32, libc::SIGTERM) };
    assert_eq!(rc, 0, "kill(SIGTERM)");

    let (exit, _) = server.wait_for_exit(Duration::from_secs(40));
    assert_eq!(
        exit.code(),
        Some(0),
        "{exit}; stderr:\n{}",
        server.diagnostics()
    );
    let log = server.diagnostics();
    assert!(log.contains("SIGTERM received, shutting down"), "{log}");
    assert!(!log.contains("Restart requested"), "{log}");
}

#[test]
fn without_a_supervisor_restart_is_409_and_the_server_keeps_serving() {
    let mut server = Server::start(false);

    let (status, body) = server.post_restart();

    assert!(status.contains("409"), "{status}: {body}");
    assert_eq!(body["ok"], false, "{body}");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|e| e.contains("перезапустите сервер вручную")),
        "{body}"
    );
    // Still up: the refused request did not start a shutdown.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        server.child.0.try_wait().expect("try_wait").is_none(),
        "server exited after a refused restart; stderr:\n{}",
        server.diagnostics()
    );
    server.instance_id();
}
