//! WEB-28 T2: `POST /api/update/install` and `GET /api/update/status` — start
//! `scripts/self-update.sh` in a transient systemd unit and report the status file it
//! writes.
//!
//! The server never builds anything itself. When the operator has switched self-update on,
//! the route asks `systemd-run --user` for a unit named [`UPDATE_UNIT`], which outlives the
//! restart of the main service the script performs. Everything that decides "may we?" lives
//! in [`resolve`], read from the environment on every request; `/api/capabilities` and the
//! install route both go through it, so the flag the console shows and the answer the route
//! gives cannot differ. The process launcher is a trait ([`UpdateSpawner`]) so tests inject
//! their own and never start a unit.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Name of the transient unit. A second `systemd-run --unit` with the same name fails while
/// the first one is alive, which is the "an update is already running" signal.
pub const UPDATE_UNIT: &str = "cc-switch-web-update";

const SELF_UPDATE_ENV: &str = "CC_SWITCH_WEB_SELF_UPDATE";
const SOURCE_DIR_ENV: &str = "CC_SWITCH_WEB_SOURCE_DIR";
/// Set by systemd for every unit it starts. Not `CC_SWITCH_WEB_SUPERVISED`: that one is the
/// operator's word for Docker and others, and `systemd-run --user` needs systemd.
const SYSTEMD_ENV: &str = "INVOCATION_ID";
const SCRIPT_PATH: &str = "scripts/self-update.sh";
/// Variables of the server's own environment the script needs and a transient unit does not
/// inherit (it gets the manager's environment, not the service's).
const FORWARDED_ENV: [&str; 5] = [
    "HOST",
    "PORT",
    "WEB_DIST",
    "XDG_STATE_HOME",
    "CC_SWITCH_WEB_BUILD_PATH",
];
/// Where `scripts/self-update.sh` writes, below `${XDG_STATE_HOME:-$HOME/.local/state}`.
const STATUS_SUBPATH: &str = "cc-switch-web/update-status.json";
/// The script writes a few hundred bytes; anything bigger is not its file.
const STATUS_MAX_BYTES: u64 = 64 * 1024;
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);
const CAPTURE_MAX_BYTES: u64 = 8 * 1024;
const MESSAGE_MAX_CHARS: usize = 300;
const ALREADY_RUNNING: &str = "an update is already running";

/// What [`UpdateSpawner::spawn`] is asked to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateLaunch {
    pub unit: &'static str,
    /// `<CC_SWITCH_WEB_SOURCE_DIR>/scripts/self-update.sh`.
    pub script: PathBuf,
    pub source_dir: PathBuf,
    /// From `CC_SWITCH_WEB_UPDATE_BRANCH` (or its default), never from a request.
    pub branch: String,
    /// The forwarded variables that are set in the server's environment.
    pub env: Vec<(&'static str, OsString)>,
    /// The file the script reports into; cleared before the unit starts.
    pub status_file: PathBuf,
}

impl UpdateLaunch {
    /// The argument vector of `systemd-run`: one element per value, no shell anywhere.
    /// `--` ends its own options, so a source dir or branch that starts with `-` stays an
    /// argument of the script.
    pub fn systemd_run_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            "--user".into(),
            format!("--unit={}", self.unit).into(),
            "--collect".into(),
        ];
        for (name, value) in &self.env {
            let mut arg = OsString::from(format!("--setenv={name}="));
            arg.push(value);
            args.push(arg);
        }
        args.push("--".into());
        args.push(self.script.clone().into());
        args.push(self.source_dir.clone().into());
        args.push(self.branch.clone().into());
        args
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpawnError {
    /// The unit name is taken: an update is running.
    AlreadyRunning,
    /// The unit could not be started; the text is safe to show.
    Failed(String),
}

/// Starts the update unit. Blocking: the route calls it from the blocking pool.
pub trait UpdateSpawner: Send + Sync {
    /// Whether the update unit is alive right now.
    fn is_running(&self) -> Result<bool, String>;
    fn spawn(&self, launch: &UpdateLaunch) -> Result<(), SpawnError>;
}

/// `systemd-run --user` / `systemctl --user`, found through the server's `PATH`.
pub struct SystemdRunSpawner {
    systemd_run: PathBuf,
    systemctl: PathBuf,
    timeout: Duration,
}

impl Default for SystemdRunSpawner {
    fn default() -> Self {
        Self::with_programs("systemd-run".into(), "systemctl".into(), LAUNCH_TIMEOUT)
    }
}

impl SystemdRunSpawner {
    /// Other programs and a shorter bound, for a test that plays systemd with a script.
    pub fn with_programs(systemd_run: PathBuf, systemctl: PathBuf, timeout: Duration) -> Self {
        Self {
            systemd_run,
            systemctl,
            timeout,
        }
    }
}

impl UpdateSpawner for SystemdRunSpawner {
    fn is_running(&self) -> Result<bool, String> {
        let mut cmd = Command::new(&self.systemctl);
        cmd.args(["--user", "is-active", UPDATE_UNIT]);
        let out = run_bounded(cmd, self.timeout).map_err(|e| format!("systemctl: {e}"))?;
        // `is-active` prints the state and exits 3 for every state but active.
        match out.stdout.trim() {
            "active" | "activating" | "deactivating" | "reloading" | "refreshing" => Ok(true),
            "inactive" | "failed" | "unknown" => Ok(false),
            _ => Err(format!(
                "systemctl --user is-active gave no state: {}",
                tidy(&out.stderr)
            )),
        }
    }

    fn spawn(&self, launch: &UpdateLaunch) -> Result<(), SpawnError> {
        let mut cmd = Command::new(&self.systemd_run);
        cmd.args(launch.systemd_run_args());
        let out = run_bounded(cmd, self.timeout)
            .map_err(|e| SpawnError::Failed(format!("systemd-run: {e}")))?;
        if out.success {
            return Ok(());
        }
        // A taken unit name and any other failure look alike on stderr across systemd
        // versions, so ask the manager instead of matching its words.
        if self.is_running() == Ok(true) {
            return Err(SpawnError::AlreadyRunning);
        }
        Err(SpawnError::Failed(format!(
            "systemd-run could not start the update unit: {}",
            tidy(&out.stderr)
        )))
    }
}

struct Captured {
    success: bool,
    stdout: String,
    stderr: String,
}

/// Runs `cmd` with no stdin, at most `timeout`, reading at most [`CAPTURE_MAX_BYTES`] of
/// each output (the readers keep draining, so a chatty child never blocks on a full pipe).
fn run_bounded(mut cmd: Command, timeout: Duration) -> Result<Captured, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("cannot start: {e}"))?;
    let out = drain(child.stdout.take());
    let err = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "did not finish within {}s",
                    timeout.as_secs().max(1)
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => break Err(format!("wait failed: {e}")),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    status.map(|status| Captured {
        success: status.success(),
        stdout,
        stderr,
    })
}

/// Keeps the first [`CAPTURE_MAX_BYTES`] of a pipe and drains the rest, so the child never
/// blocks on a full pipe.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = (&mut pipe).take(CAPTURE_MAX_BYTES).read_to_end(&mut kept);
            let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        }
        String::from_utf8_lossy(&kept).into_owned()
    })
}

/// A tool's output as one short printable line: it ends up in an unauthenticated response.
fn tidy(text: &str) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_ascii_graphic() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    line.chars().take(MESSAGE_MAX_CHARS).collect()
}

type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

pub(super) fn process_env(key: &str) -> Option<OsString> {
    std::env::var_os(key)
}

fn non_empty(env: Env, key: &str) -> Option<OsString> {
    env(key).filter(|v| !v.is_empty())
}

/// The status file, at the path `scripts/self-update.sh` writes: `$XDG_STATE_HOME` when
/// set and non-empty, else `$HOME/.local/state`. A relative `XDG_STATE_HOME` is refused:
/// the script and the server would not share a working directory.
pub(super) fn status_path(env: Env) -> Result<PathBuf, String> {
    let base = match non_empty(env, "XDG_STATE_HOME") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            if !dir.is_absolute() {
                return Err("XDG_STATE_HOME must be an absolute path".into());
            }
            dir
        }
        None => match non_empty(env, "HOME") {
            Some(home) => PathBuf::from(home).join(".local/state"),
            None => return Err("neither XDG_STATE_HOME nor HOME is set".into()),
        },
    };
    Ok(base.join(STATUS_SUBPATH))
}

/// Every condition of a self-update, in the order an operator fixes them. `Err` is the
/// reason shown in the 409. Paths are named by variable, not echoed.
pub(super) fn resolve(env: Env, branch: &str) -> Result<UpdateLaunch, String> {
    if env(SELF_UPDATE_ENV).as_deref() != Some("1".as_ref()) {
        return Err(format!(
            "self-update is off: set {SELF_UPDATE_ENV}=1 and {SOURCE_DIR_ENV} in the server's environment, or update by hand with the commands of the update check"
        ));
    }
    if non_empty(env, SYSTEMD_ENV).is_none() {
        return Err(format!(
            "self-update needs the server to run as a systemd user unit ({SYSTEMD_ENV} is not set); in Docker rebuild the image with the commands of the update check"
        ));
    }
    let Some(source_dir) = non_empty(env, SOURCE_DIR_ENV).map(PathBuf::from) else {
        return Err(format!("{SOURCE_DIR_ENV} is not set"));
    };
    if !source_dir.is_absolute() {
        return Err(format!("{SOURCE_DIR_ENV} must be an absolute path"));
    }
    if !source_dir.is_dir() {
        return Err(format!("{SOURCE_DIR_ENV} is not a directory"));
    }
    let script = source_dir.join(SCRIPT_PATH);
    if !is_executable_file(&script) {
        return Err(format!("{SOURCE_DIR_ENV} has no executable {SCRIPT_PATH}"));
    }
    if branch.is_empty()
        || branch.starts_with('-')
        || branch.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("CC_SWITCH_WEB_UPDATE_BRANCH is not a usable branch name".into());
    }
    let status_file = status_path(env)?;
    let forwarded = FORWARDED_ENV
        .iter()
        .filter_map(|&name| non_empty(env, name).map(|value| (name, value)))
        .collect();
    Ok(UpdateLaunch {
        unit: UPDATE_UNIT,
        script,
        source_dir,
        branch: branch.to_string(),
        env: forwarded,
        status_file,
    })
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// What the router layer carries: the launcher, or nothing (`router(state)` as the tests
/// and the init-error mode build it — self-update is then off whatever the environment says).
#[derive(Clone)]
pub(super) struct UpdateControl(pub Option<Arc<UpdateRunner>>);

/// The launcher and the lock that makes "is one running?" and "start one" a single step.
pub struct UpdateRunner {
    spawner: Arc<dyn UpdateSpawner>,
    start: Mutex<()>,
}

impl UpdateRunner {
    pub fn new(spawner: Arc<dyn UpdateSpawner>) -> Self {
        Self {
            spawner,
            start: Mutex::new(()),
        }
    }
}

pub(super) enum Start {
    Started(UpdateLaunch),
    /// 409, with the reason.
    Refused(String),
    /// 500, with a text that is safe to show.
    Failed(String),
}

impl UpdateControl {
    /// All conditions, read from the environment now. The capabilities flag is
    /// `launch().is_ok()`, the install route starts from the same value.
    pub(super) fn launch(&self) -> Result<UpdateLaunch, String> {
        let branch = crate::services::web_update::UpdateSourceConfig::from_env().branch;
        let launch = resolve(&process_env, &branch)?;
        if self.0.is_none() {
            return Err("this server was started without an update launcher".into());
        }
        Ok(launch)
    }

    pub(super) fn enabled(&self) -> bool {
        self.launch().is_ok()
    }

    pub(super) async fn start(&self) -> Start {
        let launch = match self.launch() {
            Ok(launch) => launch,
            Err(reason) => return Start::Refused(reason),
        };
        let Some(runner) = self.0.clone() else {
            return Start::Refused("this server was started without an update launcher".into());
        };
        match tokio::task::spawn_blocking(move || runner.run(launch)).await {
            Ok(outcome) => outcome,
            Err(e) => Start::Failed(format!("update launcher task failed: {e}")),
        }
    }
}

impl UpdateRunner {
    /// One start at a time: the check and the launch below are not atomic by themselves, and
    /// a second request that slipped between them would clear the status the first unit is
    /// already writing. The unit name is the second line of defence (another process).
    fn run(&self, launch: UpdateLaunch) -> Start {
        let _one_at_a_time = self.start.lock().unwrap_or_else(|e| e.into_inner());
        match self.spawner.is_running() {
            Ok(true) => return Start::Refused(ALREADY_RUNNING.into()),
            Ok(false) => {}
            Err(e) => {
                return Start::Failed(format!(
                    "cannot tell whether an update is running: {}",
                    tidy(&e)
                ))
            }
        }
        // The previous run's last stage must not be read as this run's result, and a launch
        // that fails must not leave a stage behind: clear first, start second.
        if let Err(e) = clear_status(&launch.status_file) {
            return Start::Failed(format!("cannot clear the previous update status: {e}"));
        }
        match self.spawner.spawn(&launch) {
            Ok(()) => Start::Started(launch),
            Err(SpawnError::AlreadyRunning) => Start::Refused(ALREADY_RUNNING.into()),
            Err(SpawnError::Failed(why)) => Start::Failed(tidy(&why)),
        }
    }
}

fn clear_status(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.kind().to_string()),
    }
}

/// The fields `scripts/self-update.sh` writes. Parsing into a struct and serialising it
/// again means the response carries these fields and nothing else from the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRunStatus {
    pub stage: String,
    pub ok: bool,
    #[serde(default)]
    pub message: String,
    pub from_sha: Option<String>,
    pub to_sha: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// `Ok(None)`: no update has run (or its status was cleared for the next one). `Err` is a
/// fixed text: the file's content never reaches the caller through an error.
pub(super) fn read_status(path: &Path) -> Result<Option<UpdateRunStatus>, String> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("the update status cannot be read".into()),
    };
    // Opening a FIFO would block the worker for good.
    if !meta.is_file() || meta.len() > STATUS_MAX_BYTES {
        return Err("the update status file is not a status file".into());
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("the update status cannot be read".into()),
    };
    let mut bytes = Vec::new();
    file.take(STATUS_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "the update status cannot be read".to_string())?;
    if bytes.len() as u64 > STATUS_MAX_BYTES {
        return Err("the update status file is not a status file".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "the update status file is not valid JSON".into())
}

#[cfg(test)]
mod tests;
