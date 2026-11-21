//! Helpers for the tests that start the real `examples/server` process
//! (`server_example_data_dir.rs`, `server_example_restart.rs`): the child guard, the example
//! binary, a free port and a one-shot HTTP client. Not a `tests/support.rs` submodule: those
//! tests do not need its HOME fixtures, and this file is Unix-only in practice.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

/// Kills and reaps the child however the test ends (assert, panic, early return).
pub struct ChildGuard(pub Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `target/<profile>/examples/server`, built on demand: `cargo test --test <name>`
/// does not build examples.
pub fn server_example() -> PathBuf {
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

pub fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind port 0");
    listener.local_addr().expect("local addr").port()
}

/// One request over HTTP/1.0 (no chunking, the server closes after the response): status line
/// and body. `body` is sent as JSON with a Content-Length. A connection that closes without
/// a response gives an empty status line.
pub fn http(
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
