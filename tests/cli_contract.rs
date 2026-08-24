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

#[test]
fn no_input_calendar_create_reports_missing_name_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "calendar",
            "create",
        ])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains(
            "\"code\":\"required_input_missing\"",
        ))
        .stderr(predicate::str::contains("calendar name"));
}

#[test]
fn no_input_event_create_reports_missing_fields_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "event",
            "create",
        ])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains(
            "\"code\":\"required_input_missing\"",
        ))
        .stderr(predicate::str::contains("calendar"));
}

#[test]
fn event_create_requires_one_explicit_temporal_form() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "event",
            "create",
            "--calendar",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--title",
            "Standup",
            "--start",
            "2026-08-24T09:00:00-07:00",
            "--end",
            "2026-08-24T09:15:00-07:00",
        ])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains("timezone"));
}

#[test]
fn event_create_rejects_mixed_timed_and_all_day_flags() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "event",
            "create",
            "--calendar",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--title",
            "Standup",
            "--start",
            "2026-08-24T09:00:00-07:00",
            "--end",
            "2026-08-24T09:15:00-07:00",
            "--timezone",
            "America/Los_Angeles",
            "--all-day-start",
            "2026-08-24",
            "--all-day-end",
            "2026-08-25",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn event_create_rejects_unknown_iana_timezone_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "event",
            "create",
            "--calendar",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--title",
            "Standup",
            "--start",
            "2026-08-24T09:00:00-07:00",
            "--end",
            "2026-08-24T09:15:00-07:00",
            "--timezone",
            "Mars/Olympus",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("valid IANA timezone"));
}
