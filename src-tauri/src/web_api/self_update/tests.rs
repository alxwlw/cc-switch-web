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
