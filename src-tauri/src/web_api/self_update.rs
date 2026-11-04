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
const ALREADY_RUNNING: &str = "обновление уже идёт";

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
mod tests {
    use super::*;
    use serial_test::serial;
    use std::collections::HashMap;

    /// Everything a self-update needs, in a directory that has the script.
    struct Rig {
        dir: tempfile::TempDir,
        vars: HashMap<&'static str, OsString>,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir.path().join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        let script = scripts.join("self-update.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let vars = HashMap::from([
            (SELF_UPDATE_ENV, "1".into()),
            (SYSTEMD_ENV, "0f3c9a5e1d2b4c6f".into()),
            (SOURCE_DIR_ENV, dir.path().as_os_str().to_owned()),
            ("HOME", "/home/svc".into()),
        ]);
        Rig { dir, vars }
    }

    impl Rig {
        fn resolve(&self, branch: &str) -> Result<UpdateLaunch, String> {
            resolve(&|k: &str| self.vars.get(k).cloned(), branch)
        }
    }

    #[test]
    fn a_complete_environment_resolves_to_the_launch() {
        let mut rig = rig();
        rig.vars.insert("HOST", "100.64.0.7".into());
        rig.vars.insert("PORT", "18766".into());
        rig.vars.insert("WEB_DIST", "/srv/dist-web".into());
        rig.vars.insert("XDG_STATE_HOME", "/srv/state".into());
        rig.vars
            .insert("CC_SWITCH_WEB_BUILD_PATH", "/opt/node/bin".into());

        let launch = rig.resolve("release").unwrap();

        assert_eq!(launch.unit, "cc-switch-web-update");
        assert_eq!(launch.source_dir, rig.dir.path());
        assert_eq!(launch.script, rig.dir.path().join("scripts/self-update.sh"));
        assert_eq!(launch.branch, "release");
        assert_eq!(
            launch.status_file,
            PathBuf::from("/srv/state/cc-switch-web/update-status.json")
        );
        assert_eq!(
            launch.env,
            [
                ("HOST", OsString::from("100.64.0.7")),
                ("PORT", "18766".into()),
                ("WEB_DIST", "/srv/dist-web".into()),
                ("XDG_STATE_HOME", "/srv/state".into()),
                ("CC_SWITCH_WEB_BUILD_PATH", "/opt/node/bin".into()),
            ]
        );
    }

    #[test]
    fn only_the_set_variables_are_forwarded() {
        let mut rig = rig();
        rig.vars.insert("PORT", "19001".into());
        rig.vars.insert("HOST", "".into());
        rig.vars.insert("WEB_DIST", "".into());

        let launch = rig.resolve("main").unwrap();

        assert_eq!(launch.env, [("PORT", OsString::from("19001"))]);
    }

    #[test]
    fn the_status_file_falls_back_to_home_when_xdg_state_home_is_empty() {
        let mut rig = rig();
        rig.vars.insert("XDG_STATE_HOME", "".into());

        let launch = rig.resolve("main").unwrap();

        assert_eq!(
            launch.status_file,
            PathBuf::from("/home/svc/.local/state/cc-switch-web/update-status.json")
        );
        assert!(launch.env.iter().all(|(k, _)| *k != "XDG_STATE_HOME"));
    }

    #[test]
    fn the_conditions_each_refuse_with_their_own_reason() {
        let cases: [(&str, fn(&mut Rig), &str); 9] = [
            (
                "flag unset",
                |r| {
                    r.vars.remove(SELF_UPDATE_ENV);
                },
                "CC_SWITCH_WEB_SELF_UPDATE=1",
            ),
            (
                "flag is not 1",
                |r| {
                    r.vars.insert(SELF_UPDATE_ENV, "true".into());
                },
                "CC_SWITCH_WEB_SELF_UPDATE=1",
            ),
            (
                "no systemd",
                |r| {
                    r.vars.remove(SYSTEMD_ENV);
                },
                "systemd user unit",
            ),
            (
                "empty INVOCATION_ID",
                |r| {
                    r.vars.insert(SYSTEMD_ENV, "".into());
                },
                "systemd user unit",
            ),
            (
                "no source dir",
                |r| {
                    r.vars.remove(SOURCE_DIR_ENV);
                },
                "CC_SWITCH_WEB_SOURCE_DIR is not set",
            ),
            (
                "relative source dir",
                |r| {
                    r.vars.insert(SOURCE_DIR_ENV, "src/checkout".into());
                },
                "must be an absolute path",
            ),
            (
                "missing source dir",
                |r| {
                    r.vars
                        .insert(SOURCE_DIR_ENV, "/nonexistent/cc-switch-web".into());
                },
                "is not a directory",
            ),
            (
                "relative state dir",
                |r| {
                    r.vars.insert("XDG_STATE_HOME", "state".into());
                },
                "XDG_STATE_HOME must be an absolute path",
            ),
            (
                "nowhere to keep the status",
                |r| {
                    r.vars.remove("HOME");
                },
                "neither XDG_STATE_HOME nor HOME",
            ),
        ];
        for (name, break_it, reason) in cases {
            let mut rig = rig();
            break_it(&mut rig);
            let err = rig.resolve("main").expect_err(name);
            assert!(err.contains(reason), "{name}: {err}");
            assert!(
                !err.contains(rig.dir.path().to_str().unwrap()),
                "{name}: a path leaked into the reason: {err}"
            );
        }
    }

    #[test]
    fn the_flag_is_checked_before_systemd_and_the_directory() {
        let mut rig = rig();
        rig.vars.remove(SELF_UPDATE_ENV);
        rig.vars.remove(SYSTEMD_ENV);
        rig.vars.remove(SOURCE_DIR_ENV);

        let err = rig.resolve("main").unwrap_err();

        assert!(err.contains("CC_SWITCH_WEB_SELF_UPDATE=1"), "{err}");
    }

    #[test]
    fn a_source_dir_without_the_script_or_without_the_exec_bit_is_refused() {
        let rig_missing = rig();
        std::fs::remove_file(rig_missing.dir.path().join(SCRIPT_PATH)).unwrap();
        let err = rig_missing.resolve("main").unwrap_err();
        assert!(
            err.contains("no executable scripts/self-update.sh"),
            "{err}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let rig_plain = rig();
            let script = rig_plain.dir.path().join(SCRIPT_PATH);
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
            let err = rig_plain.resolve("main").unwrap_err();
            assert!(
                err.contains("no executable scripts/self-update.sh"),
                "{err}"
            );
        }
    }

    #[test]
    fn a_branch_that_could_be_an_option_or_carry_whitespace_is_refused() {
        let rig = rig();
        for branch in ["", "-rf", "--upload-pack=x", "a b", "a\nb", "a\tb"] {
            let err = rig.resolve(branch).expect_err(branch);
            assert!(
                err.contains("not a usable branch name"),
                "{branch:?}: {err}"
            );
        }
        assert!(rig.resolve("feat/x-1").is_ok());
    }

    #[test]
    fn the_systemd_run_arguments_are_one_element_per_value() {
        let launch = UpdateLaunch {
            unit: UPDATE_UNIT,
            script: "/srv/cc switch/scripts/self-update.sh".into(),
            source_dir: "/srv/cc switch".into(),
            branch: "feat/x y".into(),
            env: vec![
                ("PORT", "18766".into()),
                ("CC_SWITCH_WEB_BUILD_PATH", "/a:/b c".into()),
            ],
            status_file: "/s/update-status.json".into(),
        };

        let args: Vec<String> = launch
            .systemd_run_args()
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();

        assert_eq!(
            args,
            [
                "--user",
                "--unit=cc-switch-web-update",
                "--collect",
                "--setenv=PORT=18766",
                "--setenv=CC_SWITCH_WEB_BUILD_PATH=/a:/b c",
                "--",
                "/srv/cc switch/scripts/self-update.sh",
                "/srv/cc switch",
                "feat/x y",
            ]
        );
    }

    #[test]
    fn the_status_path_is_the_one_the_script_writes() {
        let env = |k: &str| match k {
            "XDG_STATE_HOME" => Some(OsString::from("/x/state")),
            "HOME" => Some(OsString::from("/home/u")),
            _ => None,
        };
        assert_eq!(
            status_path(&env).unwrap(),
            PathBuf::from("/x/state/cc-switch-web/update-status.json")
        );
        let blank = |k: &str| match k {
            "XDG_STATE_HOME" => Some(OsString::new()),
            "HOME" => Some(OsString::from("/home/u")),
            _ => None,
        };
        assert_eq!(
            status_path(&blank).unwrap(),
            PathBuf::from("/home/u/.local/state/cc-switch-web/update-status.json")
        );
    }

    #[test]
    fn a_status_file_is_read_field_by_field_and_nothing_else_passes_through() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-status.json");
        assert_eq!(read_status(&path), Ok(None), "no file: no status");

        std::fs::write(
            &path,
            r#"{"stage":"building","ok":true,"message":"building 1c24a43","fromSha":"aaa1111","toSha":null,"startedAt":"2026-11-04T10:00:00Z","finishedAt":null,"token":"ghp_secret"}"#,
        )
        .unwrap();
        let status = read_status(&path).unwrap().unwrap();
        assert_eq!(status.stage, "building");
        assert!(status.ok);
        assert_eq!(status.message, "building 1c24a43");
        assert_eq!(status.from_sha.as_deref(), Some("aaa1111"));
        assert_eq!(status.to_sha, None);
        assert_eq!(status.finished_at, None);
        let echoed = serde_json::to_string(&status).unwrap();
        assert!(!echoed.contains("ghp_secret"), "{echoed}");
        assert!(echoed.contains(r#""fromSha":"aaa1111""#), "{echoed}");
    }

    #[test]
    fn a_broken_status_file_gives_a_fixed_error_that_does_not_quote_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-status.json");
        for body in [
            "{\"stage\":\"build",
            "not json at all, password=hunter2",
            "{\"stage\":1,\"ok\":\"yes\"}",
            "",
        ] {
            std::fs::write(&path, body).unwrap();
            let err = read_status(&path).unwrap_err();
            assert_eq!(err, "the update status file is not valid JSON", "{body:?}");
        }
        std::fs::write(&path, vec![b' '; (STATUS_MAX_BYTES + 1) as usize]).unwrap();
        assert_eq!(
            read_status(&path).unwrap_err(),
            "the update status file is not a status file"
        );
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert_eq!(
            read_status(&path).unwrap_err(),
            "the update status file is not a status file"
        );
    }

    // --- the real launcher, against scripts that play systemd ----------------------------

    #[cfg(unix)]
    mod launcher {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn stub(dir: &Path, name: &str, body: &str) -> PathBuf {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        fn launch(dir: &Path) -> UpdateLaunch {
            UpdateLaunch {
                unit: UPDATE_UNIT,
                script: dir.join("scripts/self-update.sh"),
                source_dir: dir.to_path_buf(),
                branch: "main".into(),
                env: vec![("PORT", "18766".into())],
                status_file: dir.join("update-status.json"),
            }
        }

        #[test]
        #[serial]
        fn systemd_run_receives_the_arguments_and_success_is_started() {
            let dir = tempfile::tempdir().unwrap();
            let record = dir.path().join("argv");
            let run = stub(
                dir.path(),
                "systemd-run",
                &format!(
                    "for a in \"$@\"; do printf '%s\\n' \"$a\"; done >{}",
                    record.display()
                ),
            );
            let ctl = stub(dir.path(), "systemctl", "echo inactive; exit 3");
            let spawner = SystemdRunSpawner::with_programs(run, ctl, Duration::from_secs(10));
            let launch = launch(dir.path());

            assert_eq!(spawner.spawn(&launch), Ok(()));

            let got = std::fs::read_to_string(&record).unwrap();
            let want: Vec<String> = launch
                .systemd_run_args()
                .into_iter()
                .map(|a| a.into_string().unwrap())
                .collect();
            assert_eq!(got.lines().collect::<Vec<_>>(), want);
        }

        #[test]
        #[serial]
        fn a_failed_start_is_failed_unless_the_unit_is_alive() {
            let dir = tempfile::tempdir().unwrap();
            let run = stub(
                dir.path(),
                "systemd-run",
                "echo 'Failed to start transient service unit: Unit cc-switch-web-update.service was already loaded' >&2; exit 1",
            );
            let idle = stub(dir.path(), "systemctl", "echo inactive; exit 3");
            let busy = stub(dir.path(), "systemctl-busy", "echo active; exit 0");
            let launch = launch(dir.path());

            let spawner =
                SystemdRunSpawner::with_programs(run.clone(), idle, Duration::from_secs(10));
            match spawner.spawn(&launch) {
                Err(SpawnError::Failed(why)) => {
                    assert!(why.contains("already loaded"), "{why}")
                }
                other => panic!("{other:?}"),
            }

            let spawner = SystemdRunSpawner::with_programs(run, busy, Duration::from_secs(10));
            assert_eq!(spawner.spawn(&launch), Err(SpawnError::AlreadyRunning));
        }

        #[test]
        #[serial]
        fn is_running_reads_the_state_systemctl_prints() {
            let dir = tempfile::tempdir().unwrap();
            for (state, running) in [
                ("active", true),
                ("activating", true),
                ("inactive", false),
                ("failed", false),
            ] {
                let ctl = stub(dir.path(), "systemctl", &format!("echo {state}; exit 3"));
                let run = dir.path().join("unused");
                let spawner = SystemdRunSpawner::with_programs(run, ctl, Duration::from_secs(10));
                assert_eq!(spawner.is_running(), Ok(running), "{state}");
            }
            let ctl = stub(
                dir.path(),
                "systemctl",
                "echo 'Failed to connect to bus' >&2; exit 1",
            );
            let spawner = SystemdRunSpawner::with_programs(
                dir.path().join("unused"),
                ctl,
                Duration::from_secs(10),
            );
            let err = spawner.is_running().unwrap_err();
            assert!(err.contains("Failed to connect to bus"), "{err}");
        }

        #[test]
        #[serial]
        fn a_launcher_that_hangs_is_killed_at_the_bound() {
            let dir = tempfile::tempdir().unwrap();
            let run = stub(dir.path(), "systemd-run", "exec sleep 30");
            let ctl = stub(dir.path(), "systemctl", "echo inactive; exit 3");
            let spawner = SystemdRunSpawner::with_programs(run, ctl, Duration::from_millis(300));

            let started = Instant::now();
            let result = spawner.spawn(&launch(dir.path()));

            assert!(started.elapsed() < Duration::from_secs(10));
            match result {
                Err(SpawnError::Failed(why)) => assert!(why.contains("did not finish"), "{why}"),
                other => panic!("{other:?}"),
            }
        }

        #[test]
        #[serial]
        fn a_missing_program_is_a_failed_start_not_a_panic() {
            let dir = tempfile::tempdir().unwrap();
            let spawner = SystemdRunSpawner::with_programs(
                dir.path().join("no-such-systemd-run"),
                dir.path().join("no-such-systemctl"),
                Duration::from_secs(10),
            );
            match spawner.spawn(&launch(dir.path())) {
                Err(SpawnError::Failed(why)) => assert!(why.contains("cannot start"), "{why}"),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn tidy_keeps_one_short_printable_line() {
        assert_eq!(tidy("a\n  b\t\u{7}c é"), "a b c");
        assert_eq!(tidy(&"x".repeat(1000)).len(), MESSAGE_MAX_CHARS);
    }
}
