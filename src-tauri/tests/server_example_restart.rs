//! WEB-26: `POST /api/restart` against the real `examples/server` process.
//!
//! The route tests (`web_api_restart.rs`) inject a handle; here the process itself is started
//! with its own HOME, a free port and an environment built from nothing (`env_clear` plus an
//! explicit list), so neither the developer's `INVOCATION_ID` nor a running service reaches
//! it. Pinned: under a supervisor the 202 comes back and the process exits with 75, also while
//! an `/api/events` stream is open (the drain timer branch of `main`); SIGTERM still exits with
//! 0; without a supervisor the answer is 409 and the process keeps serving.
//!
//! A second restart request is reachable on the real process: the route raises the signal
//! 100 ms after the 202, and axum stops accepting connections only when the graceful-shutdown
//! future (the signal, then the exit cleanup) is done. `a_restart_request_hammered_...` keeps
//! posting until the connection goes away and checks that every answer is 202, that the exit
//! cleanup ran once, that nothing panicked and that the exit code is still 75.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

// Both server-process test binaries pull the helpers in and each uses a part of them: the
// allow sits on this side.
#[allow(dead_code)]
#[path = "support/server_process.rs"]
mod server_process;

use server_process::{free_port, http, server_example, ChildGuard};

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

/// Writes a `POST /api/restart` on a fresh connection and leaves the answer unread.
fn send_restart(addr: SocketAddr) -> std::io::Result<TcpStream> {
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    write!(
        stream,
        "POST /api/restart HTTP/1.0\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}"
    )?;
    Ok(stream)
}

/// The status line of an answer read to its end; empty when the connection closed without one.
fn read_status(mut stream: TcpStream) -> std::io::Result<String> {
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    Ok(raw.lines().next().unwrap_or_default().to_string())
}

#[test]
fn a_restart_request_repeated_through_the_whole_shutdown_gets_202_each_time_and_one_cleanup() {
    let mut server = Server::start(true);

    // Two clicks at once: both requests are on the wire before the first answer is read, so
    // both reach the route inside the 100 ms between the 202 and the signal.
    let a = send_restart(server.addr).expect("first request");
    let b = send_restart(server.addr).expect("second request");
    let mut statuses = vec![read_status(a).expect("first answer")];
    statuses.push(read_status(b).expect("second answer"));
    // Then keep clicking until the listener goes away: the signal, the exit cleanup (axum
    // still accepts here) and the end of the process are all crossed by requests.
    let deadline = Instant::now() + Duration::from_secs(60);
    while server.child.0.try_wait().expect("try_wait").is_none() {
        assert!(
            Instant::now() < deadline,
            "still answering a minute later; stderr:\n{}",
            server.diagnostics()
        );
        match send_restart(server.addr).and_then(read_status) {
            // Refused or reset: the listener is gone. Empty: accepted, then closed by the
            // graceful shutdown before the request was read. Neither is an answer.
            Err(_) => break,
            Ok(status) if status.is_empty() => break,
            Ok(status) => statuses.push(status),
        }
        std::thread::sleep(Duration::from_millis(3));
    }

    let (exit, _) = server.wait_for_exit(Duration::from_secs(40));
    let log = server.diagnostics();
    assert!(
        statuses.iter().all(|s| s.contains("202")),
        "{statuses:?}; stderr:\n{log}"
    );
    assert!(statuses.len() >= 2, "{statuses:?}");
    assert_eq!(exit.code(), Some(75), "{exit}; stderr:\n{log}");
    // The line is logged at info and the child has no RUST_LOG (env_clear): the same level as
    // "Restart requested", which the other tests read from stderr. 0 here would mean the cleanup
    // did not run or did not log; 2 that a repeated request started a second shutdown.
    assert_eq!(
        log.matches("退出清理完成").count(),
        1,
        "exit cleanup must run once; stderr:\n{log}"
    );
    assert!(!log.contains("panicked"), "{log}");
    assert!(!log.contains("Shutdown cleanup did not finish"), "{log}");
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
