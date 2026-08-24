use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn version_json_has_a_stable_envelope() {
    cargo_bin_cmd!("mg-calr")
        .args(["--json", "version"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"schema_version\":1"))
        .stdout(predicate::str::contains("\"command\":\"version\""))
        .stdout(predicate::str::contains("\"ok\":true"));
}

#[test]
fn config_paths_json_respects_xdg_without_touching_database() {
    cargo_bin_cmd!("mg-calr")
        .env("HOME", "/home/tester")
        .env("XDG_CONFIG_HOME", "/tmp/mg-calr-config")
        .args(["--json", "config", "paths"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "/tmp/mg-calr-config/mg-calr/config.toml",
        ));
}

#[test]
fn no_color_environment_is_accepted_for_human_output() {
    cargo_bin_cmd!("mg-calr")
        .env("NO_COLOR", "1")
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::is_match("\\x1b\\[").unwrap().not());
}

#[test]
fn invalid_configuration_is_a_stable_json_error() {
    let temp = tempfile::tempdir().unwrap();
    let config_home = temp.path().join("config");
    std::fs::create_dir_all(config_home.join("mg-calr")).unwrap();
    std::fs::write(config_home.join("mg-calr/config.toml"), "not = [valid").unwrap();

    cargo_bin_cmd!("mg-calr")
        .env("XDG_CONFIG_HOME", &config_home)
        .env("HOME", temp.path())
        .args(["--json", "config", "paths"])
        .assert()
        .failure()
        .code(78)
        .stderr(predicate::str::contains("\"code\":\"config_invalid\""));
}

#[test]
fn init_reports_unavailable_database_without_failing_or_mutating() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "init",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"command\":\"init\""))
        .stdout(predicate::str::contains("\"database_reachable\":false"))
        .stdout(predicate::str::contains("administrator_guidance"));
}
