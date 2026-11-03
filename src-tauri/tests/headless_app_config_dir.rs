//! WEB-26: the headless override of the app config dir (`app_paths.json` under
//! `<config_dir>/cc-switch-web/`) and where the resolver puts it in the order
//! env > override > default.
//!
//! Own test binary: the override cache is process-global and the tests set
//! `CC_SWITCH_CONFIG_DIR`, `CC_SWITCH_TEST_HOME` and `XDG_CONFIG_HOME`, so every
//! test is `#[serial]` and starts from `Fixture::new`, which clears the cache.

#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};

use cc_switch_lib::app_store::{
    headless_override_file_path, load_headless_override, read_headless_override_file,
    write_headless_override_file,
};
use cc_switch_lib::bootstrap::run_headless_bootstrap;
use cc_switch_lib::get_app_config_dir;
use serial_test::serial;
use support::{ensure_test_home, reset_test_fs};

const KEY: &str = "app_config_dir_override";

/// Clean slate for one test: no env knobs, no file, empty cache. Restores the
/// env it changed on drop so a failing test cannot leak into the next one.
struct Fixture {
    home: PathBuf,
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Fixture {
    fn new() -> Self {
        reset_test_fs();
        let home = ensure_test_home().to_path_buf();
        let saved = ["CC_SWITCH_CONFIG_DIR", "XDG_CONFIG_HOME"]
            .into_iter()
            .map(|k| (k, std::env::var_os(k)))
            .collect();
        std::env::remove_var("CC_SWITCH_CONFIG_DIR");
        // A distinct, real-looking config home: if the file path ever followed it,
        // the tests would write into it instead of the test home.
        std::env::set_var("XDG_CONFIG_HOME", home.join("xdg-must-stay-unused"));
        let _ = std::fs::remove_file(headless_override_file_path());
        load_headless_override();
        Self { home, saved }
    }

    fn write_file(&self, body: &str) {
        let file = headless_override_file_path();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, body).unwrap();
    }

    fn target_dir(&self, name: &str) -> PathBuf {
        let dir = self.home.join("elsewhere").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(headless_override_file_path());
        load_headless_override();
        for (k, v) in &self.saved {
            match v {
                Some(v) => std::env::set_var(k, v),
                None => std::env::remove_var(k),
            }
        }
    }
}

fn json_body(path: &Path) -> String {
    serde_json::json!({ KEY: path.to_str().unwrap() }).to_string()
}

#[test]
#[serial]
fn file_path_is_under_the_test_home_even_with_xdg_config_home_set() {
    let fx = Fixture::new();
    let file = headless_override_file_path();
    assert_eq!(
        file,
        fx.home
            .join(".config")
            .join("cc-switch-web")
            .join("app_paths.json")
    );
    assert!(!file.starts_with(fx.home.join("xdg-must-stay-unused")));
}

#[cfg(target_os = "linux")]
#[test]
#[serial]
fn file_path_follows_the_config_dir_without_a_test_home() {
    let fx = Fixture::new();
    let xdg = fx.home.join("xdg-must-stay-unused");
    let saved = std::env::var_os("CC_SWITCH_TEST_HOME");
    std::env::remove_var("CC_SWITCH_TEST_HOME");
    let file = headless_override_file_path();
    std::env::set_var("CC_SWITCH_TEST_HOME", saved.unwrap());
    assert_eq!(file, xdg.join("cc-switch-web").join("app_paths.json"));
}

#[test]
#[serial]
fn override_file_moves_the_resolver_and_env_wins_over_it() {
    let fx = Fixture::new();
    let default_dir = fx.home.join(".cc-switch");
    assert_eq!(get_app_config_dir(), default_dir);

    let target = fx.target_dir("override");
    fx.write_file(&json_body(&target));
    // The file alone changes nothing: only the load step fills the cache.
    assert_eq!(get_app_config_dir(), default_dir);

    assert_eq!(load_headless_override(), Some(target.clone()));
    assert_eq!(get_app_config_dir(), target);

    let env_dir = fx.target_dir("env");
    std::env::set_var("CC_SWITCH_CONFIG_DIR", &env_dir);
    assert_eq!(get_app_config_dir(), env_dir);

    // A blank env value is unset, the override applies again.
    std::env::set_var("CC_SWITCH_CONFIG_DIR", "   ");
    assert_eq!(get_app_config_dir(), target);
}

#[test]
#[serial]
fn reload_after_the_file_is_gone_drops_the_cached_override() {
    let fx = Fixture::new();
    let target = fx.target_dir("override");
    fx.write_file(&json_body(&target));
    load_headless_override();
    assert_eq!(get_app_config_dir(), target);

    std::fs::remove_file(headless_override_file_path()).unwrap();
    assert_eq!(load_headless_override(), None);
    assert_eq!(get_app_config_dir(), fx.home.join(".cc-switch"));
}

#[test]
#[serial]
fn broken_or_unusable_file_is_ignored_with_the_default_kept() {
    let fx = Fixture::new();
    let default_dir = fx.home.join(".cc-switch");
    let missing = fx.home.join("elsewhere").join("does-not-exist");
    let cases = [
        ("not json", "{ nope".to_string()),
        ("wrong type", format!(r#"{{"{KEY}": 7}}"#)),
        ("blank value", format!(r#"{{"{KEY}": "  "}}"#)),
        ("missing key", r#"{"other": "x"}"#.to_string()),
        ("path that does not exist", json_body(&missing)),
    ];
    for (name, body) in cases {
        fx.write_file(&body);
        assert_eq!(load_headless_override(), None, "{name}");
        assert_eq!(get_app_config_dir(), default_dir, "{name}");
    }
}

#[test]
#[serial]
fn tilde_in_the_file_expands_to_the_home_dir() {
    let fx = Fixture::new();
    let target = fx.home.join("tilde-target");
    std::fs::create_dir_all(&target).unwrap();
    fx.write_file(&format!(r#"{{"{KEY}": "~/tilde-target"}}"#));
    assert_eq!(read_headless_override_file(), Some(target.clone()));
    assert_eq!(load_headless_override(), Some(target));
}

#[test]
#[serial]
fn write_is_atomic_leaves_no_temp_files_and_keeps_the_cache() {
    let fx = Fixture::new();
    let first = fx.target_dir("first");
    let second = fx.target_dir("second");

    write_headless_override_file(Some(&first)).unwrap();
    assert_eq!(read_headless_override_file(), Some(first.clone()));
    // The running process keeps its directory until the next load.
    assert_eq!(get_app_config_dir(), fx.home.join(".cc-switch"));

    write_headless_override_file(Some(&second)).unwrap();
    assert_eq!(read_headless_override_file(), Some(second.clone()));

    let dir = headless_override_file_path()
        .parent()
        .unwrap()
        .to_path_buf();
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec!["app_paths.json".to_string()]);

    // Next start picks the written value up.
    assert_eq!(load_headless_override(), Some(second.clone()));
    assert_eq!(get_app_config_dir(), second);
}

#[test]
#[serial]
fn write_none_removes_only_the_key_and_is_a_noop_without_a_file() {
    let fx = Fixture::new();
    write_headless_override_file(None).unwrap();
    assert!(!headless_override_file_path().exists());

    let target = fx.target_dir("override");
    fx.write_file(&format!(
        r#"{{"{KEY}": {}, "keep": "me"}}"#,
        serde_json::to_string(target.to_str().unwrap()).unwrap()
    ));
    write_headless_override_file(None).unwrap();
    assert_eq!(read_headless_override_file(), None);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(headless_override_file_path()).unwrap())
            .unwrap();
    assert_eq!(doc, serde_json::json!({ "keep": "me" }));
}

#[test]
#[serial]
fn headless_bootstrap_opens_the_database_under_the_override() {
    let fx = Fixture::new();
    let target = fx.target_dir("db-home");
    fx.write_file(&json_body(&target));

    run_headless_bootstrap().expect("bootstrap");

    assert!(
        target.join("cc-switch.db").exists(),
        "database must be created under the override"
    );
    assert!(
        !fx.home.join(".cc-switch").join("cc-switch.db").exists(),
        "default dir must stay untouched"
    );
}
