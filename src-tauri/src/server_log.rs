//! Log file of the headless web server (WEB-24).
//!
//! `env_logger` writes to stderr (journald) and to `<config>/logs/cc-switch-web.log`
//! through [`TeeWriter`]. On start an oversized file is rotated to `.1`
//! (one archive copy). Lives in the lib, not in `examples/server.rs`, so the
//! tests and the log-tail routes can reach it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const LOG_FILE_NAME: &str = "cc-switch-web.log";
pub const MAX_LOG_BYTES: u64 = 20 * 1024 * 1024;

/// Used when `RUST_LOG` is unset: everything is let through to `log`'s own
/// `max_level` (set from the saved `LogConfig`), except chatty dependencies
/// that would flood the file at `debug`. Mirrors nothing in the desktop (its
/// plugin silences no targets); the server file is the only place this spam
/// would be kept.
pub const DEFAULT_FILTER: &str = "trace,hyper=info,hyper_util=info,h2=info,reqwest=info,\
rustls=info,tower=info,tower_http=info,tokio_util=info,rusqlite=info,mio=info";

/// The `RUST_LOG` spec when it is set to something other than blank. An empty
/// or whitespace-only value (`Environment=RUST_LOG=` in a unit) counts as unset,
/// for the filter, the `info` cap and the bootstrap alike.
pub fn explicit_rust_log() -> Option<String> {
    non_blank_spec(std::env::var_os("RUST_LOG"))
}

fn non_blank_spec(raw: Option<std::ffi::OsString>) -> Option<String> {
    let raw = raw?;
    let spec = raw.to_string_lossy();
    let spec = spec.trim();
    (!spec.is_empty()).then(|| spec.to_string())
}

pub fn log_dir() -> PathBuf {
    crate::config::get_app_config_dir().join("logs")
}

pub fn log_file_path() -> PathBuf {
    log_dir().join(LOG_FILE_NAME)
}

fn archive_path(path: &Path) -> PathBuf {
    let mut p = path.as_os_str().to_os_string();
    p.push(".1");
    PathBuf::from(p)
}

/// Rename `path` to `path.1` (replacing an older archive) when it is larger
/// than `max_bytes`. A missing file is not an error.
pub fn rotate_if_needed(path: &Path, max_bytes: u64) -> io::Result<()> {
    match fs::metadata(path) {
        Ok(m) if m.len() > max_bytes => {
            let archive = archive_path(path);
            if archive.exists() {
                fs::remove_file(&archive)?;
            }
            fs::rename(path, archive)
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Writes every buffer to both sinks. A failing file sink never loses the
/// stderr copy: the first error is returned only after both were tried.
pub struct TeeWriter<A: Write, B: Write> {
    first: A,
    second: B,
}

impl<A: Write, B: Write> TeeWriter<A, B> {
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }
}

impl<A: Write, B: Write> Write for TeeWriter<A, B> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let a = self.first.write_all(buf);
        let b = self.second.write_all(buf);
        a.and(b).map(|()| buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let a = self.first.flush();
        let b = self.second.flush();
        a.and(b)
    }
}

/// Open (creating the directory, rotating first) the append-only log file.
pub fn open_log_file(path: &Path, max_bytes: u64) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    rotate_if_needed(path, max_bytes)?;
    OpenOptions::new().create(true).append(true).open(path)
}

/// Install the global logger: stderr + log file. The filter comes from
/// `RUST_LOG`, else [`DEFAULT_FILTER`]; the effective level is capped to `info`
/// right after init so startup does not run at `trace` before the bootstrap
/// applies the saved `LogConfig` via `log::set_max_level`. If the file cannot
/// be opened the server logs to stderr only.
pub fn init() {
    let mut builder = env_logger::Builder::new();
    let explicit = explicit_rust_log();
    match &explicit {
        Some(spec) => builder.parse_filters(spec),
        None => builder.parse_filters(DEFAULT_FILTER),
    };
    let path = log_file_path();
    let file_err = match open_log_file(&path, MAX_LOG_BYTES) {
        Ok(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(TeeWriter::new(
                io::stderr(),
                file,
            ))));
            None
        }
        Err(e) => Some(e),
    };
    builder.init();
    // env_logger::init sets max_level to its filter (trace). RUST_LOG, when set
    // (non-blank), keeps its own cap; bootstrap skips the saved level then.
    if explicit.is_none() {
        log::set_max_level(log::LevelFilter::Info);
    }
    match file_err {
        None => log::info!("Logging to {}", path.display()),
        Some(e) => log::warn!("Log file {} unavailable, stderr only: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("boom"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("boom"))
        }
    }

    #[test]
    fn blank_rust_log_counts_as_unset() {
        use std::ffi::OsString;
        let os = |s: &str| Some(OsString::from(s));
        assert_eq!(non_blank_spec(None), None);
        assert_eq!(non_blank_spec(os("")), None);
        assert_eq!(non_blank_spec(os("  \t ")), None);
        assert_eq!(
            non_blank_spec(os(" debug,hyper=warn ")),
            Some("debug,hyper=warn".into())
        );
    }

    #[test]
    fn tee_writes_to_both_sinks() {
        let (a, b) = (Shared::default(), Shared::default());
        let mut tee = TeeWriter::new(a.clone(), b.clone());
        tee.write_all(b"line one\n").unwrap();
        tee.write_all(b"line two\n").unwrap();
        tee.flush().unwrap();
        assert_eq!(&*a.0.lock().unwrap(), b"line one\nline two\n");
        assert_eq!(&*b.0.lock().unwrap(), b"line one\nline two\n");
    }

    #[test]
    fn tee_keeps_the_second_sink_when_the_first_fails() {
        let b = Shared::default();
        let mut tee = TeeWriter::new(Broken, b.clone());
        assert!(tee.write_all(b"x\n").is_err());
        assert_eq!(&*b.0.lock().unwrap(), b"x\n");
    }

    #[test]
    fn rotation_threshold_is_strictly_greater() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE_NAME);
        fs::write(&path, vec![b'a'; 10]).unwrap();
        rotate_if_needed(&path, 10).unwrap();
        assert!(path.exists());
        assert!(!archive_path(&path).exists());

        fs::write(&path, vec![b'b'; 11]).unwrap();
        rotate_if_needed(&path, 10).unwrap();
        assert!(!path.exists());
        assert_eq!(fs::read(archive_path(&path)).unwrap(), vec![b'b'; 11]);
    }

    #[test]
    fn rotation_replaces_the_old_archive_and_tolerates_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LOG_FILE_NAME);
        rotate_if_needed(&path, 1).unwrap();
        fs::write(archive_path(&path), b"old").unwrap();
        fs::write(&path, b"newer-and-big").unwrap();
        rotate_if_needed(&path, 1).unwrap();
        assert_eq!(fs::read(archive_path(&path)).unwrap(), b"newer-and-big");
    }

    #[test]
    fn open_log_file_creates_dir_rotates_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logs").join(LOG_FILE_NAME);
        let mut f = open_log_file(&path, 5).unwrap();
        f.write_all(b"123456").unwrap();
        drop(f);
        let mut f = open_log_file(&path, 5).unwrap();
        f.write_all(b"fresh").unwrap();
        drop(f);
        assert_eq!(fs::read(&path).unwrap(), b"fresh");
        assert_eq!(fs::read(archive_path(&path)).unwrap(), b"123456");
    }
}
