use super::*;
use serial_test::serial;
use std::collections::HashMap;

/// A POSIX-looking absolute path that is also absolute on Windows (`/srv/x` has no drive
/// there, so `Path::is_absolute` says no); identity elsewhere.
fn abs(path: &str) -> String {
    if cfg!(windows) {
        format!("C:{path}")
    } else {
        path.to_string()
    }
}

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
        resolve(
            &|k: &str| self.vars.get(k).cloned(),
            branch,
            "o/r",
            BuildMode::Checkout,
        )
    }
}

/// A release install: an install dir holding an executable `self-update.sh`, no source dir.
fn release_rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("self-update.sh");
    std::fs::write(&script, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let vars = HashMap::from([
        (SELF_UPDATE_ENV, "1".into()),
        (SYSTEMD_ENV, "0f3c9a5e1d2b4c6f".into()),
        (INSTALL_DIR_ENV, dir.path().as_os_str().to_owned()),
        ("HOME", "/home/svc".into()),
    ]);
    Rig { dir, vars }
}

impl Rig {
    fn resolve_release(&self, repo: &str) -> Result<UpdateLaunch, String> {
        // A branch that the checkout mode would refuse: the release mode must not look at it.
        resolve(
            &|k: &str| self.vars.get(k).cloned(),
            "-not a branch",
            repo,
            BuildMode::Release,
        )
    }
}

#[test]
fn a_complete_environment_resolves_to_the_launch() {
    let mut rig = rig();
    rig.vars.insert("HOST", "100.64.0.7".into());
    rig.vars.insert("PORT", "18766".into());
    rig.vars.insert("WEB_DIST", "/srv/dist-web".into());
    rig.vars.insert("XDG_STATE_HOME", abs("/srv/state").into());
    rig.vars
        .insert("CC_SWITCH_WEB_BUILD_PATH", "/opt/node/bin".into());

    let launch = rig.resolve("release").unwrap();

    assert_eq!(launch.unit, "cc-switch-web-update");
    assert_eq!(
        launch.source,
        UpdateSource::Checkout {
            source_dir: rig.dir.path().to_path_buf(),
            branch: "release".into(),
        }
    );
    assert_eq!(launch.script, rig.dir.path().join("scripts/self-update.sh"));
    assert_eq!(
        launch.status_file,
        PathBuf::from(abs("/srv/state/cc-switch-web/update-status.json"))
    );
    assert_eq!(
        launch.env,
        [
            ("HOST", OsString::from("100.64.0.7")),
            ("PORT", "18766".into()),
            ("WEB_DIST", "/srv/dist-web".into()),
            ("XDG_STATE_HOME", abs("/srv/state").into()),
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
                let missing = r.dir.path().join("missing");
                r.vars.insert(SOURCE_DIR_ENV, missing.into_os_string());
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
        source: UpdateSource::Checkout {
            source_dir: "/srv/cc switch".into(),
            branch: "feat/x y".into(),
        },
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
        "XDG_STATE_HOME" => Some(OsString::from(abs("/x/state"))),
        "HOME" => Some(OsString::from("/home/u")),
        _ => None,
    };
    assert_eq!(
        status_path(&env).unwrap(),
        PathBuf::from(abs("/x/state/cc-switch-web/update-status.json"))
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
    assert_eq!(
        status.from_version, None,
        "a checkout run reports no versions"
    );
    assert_eq!(status.to_version, None);
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

#[test]
fn a_release_build_resolves_to_the_installed_script_and_the_repo() {
    let rig = release_rig();

    let launch = rig.resolve_release("o/r").unwrap();

    assert_eq!(launch.script, rig.dir.path().join("self-update.sh"));
    assert_eq!(
        launch.source,
        UpdateSource::Release {
            install_dir: rig.dir.path().to_path_buf(),
            repo: "o/r".into(),
        }
    );
    let args: Vec<String> = launch
        .systemd_run_args()
        .into_iter()
        .map(|a| a.into_string().unwrap())
        .collect();
    let tail = &args[args.len() - 4..];
    assert_eq!(
        tail,
        [
            "--".to_string(),
            rig.dir
                .path()
                .join("self-update.sh")
                .to_str()
                .unwrap()
                .to_string(),
            "--release".into(),
            "o/r".into(),
        ]
    );
}

#[test]
fn a_release_build_does_not_need_a_source_dir() {
    let rig = release_rig();
    assert!(!rig.vars.contains_key(SOURCE_DIR_ENV));

    assert!(rig.resolve_release("o/r").is_ok());
}

#[test]
fn a_release_build_refuses_without_the_install_dir_or_its_script() {
    let cases: [(&str, fn(&mut Rig), &str); 4] = [
        (
            "no install dir",
            |r| {
                r.vars.remove(INSTALL_DIR_ENV);
            },
            "CC_SWITCH_WEB_INSTALL_DIR is not set",
        ),
        (
            "relative install dir",
            |r| {
                r.vars.insert(INSTALL_DIR_ENV, "share/cc-switch-web".into());
            },
            "CC_SWITCH_WEB_INSTALL_DIR must be an absolute path",
        ),
        (
            "missing install dir",
            |r| {
                let missing = r.dir.path().join("missing");
                r.vars.insert(INSTALL_DIR_ENV, missing.into_os_string());
            },
            "CC_SWITCH_WEB_INSTALL_DIR is not a directory",
        ),
        (
            "no script",
            |r| {
                std::fs::remove_file(r.dir.path().join(INSTALLED_SCRIPT)).unwrap();
            },
            "CC_SWITCH_WEB_INSTALL_DIR has no executable self-update.sh",
        ),
    ];
    for (name, break_it, reason) in cases {
        let mut rig = release_rig();
        break_it(&mut rig);
        let err = rig.resolve_release("o/r").expect_err(name);
        assert_eq!(err, reason, "{name}");
        assert!(
            !err.contains(rig.dir.path().to_str().unwrap()),
            "{name}: a path leaked into the reason: {err}"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_release_build_refuses_an_install_script_without_the_exec_bit() {
    use std::os::unix::fs::PermissionsExt;
    let rig = release_rig();
    let script = rig.dir.path().join(INSTALLED_SCRIPT);
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();

    let err = rig.resolve_release("o/r").unwrap_err();

    assert_eq!(
        err,
        "CC_SWITCH_WEB_INSTALL_DIR has no executable self-update.sh"
    );
}

#[test]
fn a_release_build_has_its_own_flag_and_systemd_text() {
    let mut rig = release_rig();
    rig.vars.remove(SELF_UPDATE_ENV);
    let err = rig.resolve_release("o/r").unwrap_err();
    assert_eq!(
        err,
        "self-update is off: set CC_SWITCH_WEB_SELF_UPDATE=1 in the server's environment (install.sh --self-update), or update by hand with the commands of the update check"
    );

    let mut rig = release_rig();
    rig.vars.remove(SYSTEMD_ENV);
    let err = rig.resolve_release("o/r").unwrap_err();
    assert_eq!(
        err,
        "self-update needs the server to run as a systemd user unit (INVOCATION_ID is not set), and this server does not run under systemd; update by hand with the commands of the update check"
    );
    assert!(!err.contains("rebuild the image"), "{err}");

    let mut checkout = self::rig();
    checkout.vars.remove(SYSTEMD_ENV);
    let err = checkout.resolve("main").unwrap_err();
    assert!(err.contains("in Docker rebuild the image"), "{err}");
}

#[test]
fn a_repo_that_is_not_owner_slash_name_is_refused() {
    let rig = release_rig();
    for repo in [
        "", "o", "o/r/x", "o r/x", "o/r\n", "/r", "o/", "o/r;x", "ö/r",
    ] {
        let err = rig.resolve_release(repo).expect_err(repo);
        assert_eq!(
            err, "CC_SWITCH_WEB_UPDATE_REPO is not a usable owner/name",
            "{repo:?}"
        );
    }
    for repo in ["o/r", "alxwlw/cc-switch-web", "A_b.c-1/x.y_z-2"] {
        assert!(rig.resolve_release(repo).is_ok(), "{repo}");
    }
}

#[test]
fn the_release_conditions_keep_their_order_flag_systemd_dir_repo() {
    let mut rig = release_rig();
    rig.vars.remove(INSTALL_DIR_ENV);
    let err = rig.resolve_release("bad").unwrap_err();
    assert!(
        err.contains("INSTALL_DIR is not set"),
        "dir before repo: {err}"
    );
}

#[test]
fn the_release_variables_are_forwarded_when_set() {
    let mut rig = release_rig();
    rig.vars
        .insert("CC_SWITCH_WEB_UPDATE_API", "http://127.0.0.1:9/api".into());
    rig.vars.insert(
        "CC_SWITCH_WEB_DOWNLOAD_BASE",
        "http://127.0.0.1:9/dl".into(),
    );
    rig.vars.insert("BIN_DIR", "/home/svc/.local/bin".into());
    rig.vars.insert("PORT", "19001".into());

    let launch = rig.resolve_release("o/r").unwrap();

    assert_eq!(
        launch.env,
        [
            ("PORT", OsString::from("19001")),
            (
                "CC_SWITCH_WEB_INSTALL_DIR",
                rig.dir.path().as_os_str().to_owned()
            ),
            ("CC_SWITCH_WEB_UPDATE_API", "http://127.0.0.1:9/api".into()),
            (
                "CC_SWITCH_WEB_DOWNLOAD_BASE",
                "http://127.0.0.1:9/dl".into()
            ),
            ("BIN_DIR", "/home/svc/.local/bin".into()),
        ]
    );
    let blank = release_rig();
    let launch = blank.resolve_release("o/r").unwrap();
    assert_eq!(
        launch.env,
        [(
            "CC_SWITCH_WEB_INSTALL_DIR",
            blank.dir.path().as_os_str().to_owned()
        )]
    );
}

#[test]
fn a_status_file_with_versions_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("update-status.json");
    std::fs::write(
        &path,
        r#"{"stage":"restarting","ok":true,"message":"restarting cc-switch-web","fromSha":null,"toSha":null,"fromVersion":"4.0.35-web.1","toVersion":"4.0.35-web.2","startedAt":"2026-11-04T10:00:00Z","finishedAt":null,"token":"ghp_secret"}"#,
    )
    .unwrap();

    let status = read_status(&path).unwrap().unwrap();

    assert_eq!(status.from_version.as_deref(), Some("4.0.35-web.1"));
    assert_eq!(status.to_version.as_deref(), Some("4.0.35-web.2"));
    assert_eq!(status.from_sha, None);
    assert_eq!(status.to_sha, None);
    let echoed = serde_json::to_value(&status).unwrap();
    assert_eq!(echoed["fromVersion"], "4.0.35-web.1");
    assert_eq!(echoed["toVersion"], "4.0.35-web.2");
    assert!(echoed.get("token").is_none(), "{echoed}");
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
            source: UpdateSource::Checkout {
                source_dir: dir.to_path_buf(),
                branch: "main".into(),
            },
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

        let spawner = SystemdRunSpawner::with_programs(run.clone(), idle, Duration::from_secs(10));
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
            ("deactivating", true),
            ("reloading", true),
            ("refreshing", true),
            ("inactive", false),
            ("failed", false),
            ("unknown", false),
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
