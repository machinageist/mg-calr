use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

/// A store path under a directory that cannot exist, so a command that reaches storage
/// fails there — and one that validates its input first never gets that far.
const UNWRITABLE_STORE: &str = "/nonexistent/mg-calr-test/calr.sqlite";

#[test]
fn event_import_help_exposes_file_argument() {
    cargo_bin_cmd!("mg-calr")
        .args(["event", "import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--file"));
}

#[test]
fn event_import_validates_before_database_access_and_hides_file_path() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("invalid-events.json");
    std::fs::write(&file, r#"{"schema_version":2,"calendars":[],"events":[]}"#).unwrap();
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--db",
            UNWRITABLE_STORE,
            "event",
            "import",
            "--file",
        ])
        .arg(&file)
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("\"code\":\"import_invalid\""))
        .stderr(predicate::str::contains(file.to_string_lossy().as_ref()).not());
}

#[test]
fn event_export_help_is_available_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args(["event", "export", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Export all calendars and events"));
}

#[test]
fn event_edit_help_exposes_optimistic_and_temporal_arguments() {
    cargo_bin_cmd!("mg-calr")
        .args(["event", "edit", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--event-id"))
        .stdout(predicate::str::contains("--version"))
        .stdout(predicate::str::contains("--start"))
        .stdout(predicate::str::contains("--all-day-start"));
}

#[test]
fn event_edit_rejects_partial_temporal_form_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--db",
            UNWRITABLE_STORE,
            "event",
            "edit",
            "--event-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
            "--start",
            "2026-08-24T09:00:00-07:00",
            "--timezone",
            "America/Los_Angeles",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("requires --end"));
}

#[test]
fn event_edit_rejects_empty_update_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "event",
            "edit",
            "--event-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("at least one editable field"));
}

#[test]
fn event_restore_help_exposes_optimistic_arguments() {
    cargo_bin_cmd!("mg-calr")
        .args(["event", "restore", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--event-id"))
        .stdout(predicate::str::contains("--version"));
}

#[test]
fn agenda_help_exposes_explicit_window_timezone_and_lifecycle_flags() {
    cargo_bin_cmd!("mg-calr")
        .args(["agenda", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--todo-projection"))
        .stdout(predicate::str::contains("--start"))
        .stdout(predicate::str::contains("--end"))
        .stdout(predicate::str::contains("--timezone"))
        .stdout(predicate::str::contains("--include-completed"))
        .stdout(predicate::str::contains("--include-trashed"))
        .stdout(predicate::str::contains("--include-blocked"));
}

#[test]
fn agenda_reports_missing_projection_before_calendar_database_failure() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing-projection.json");
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--db",
            UNWRITABLE_STORE,
            "agenda",
            "--todo-projection",
        ])
        .arg(&missing)
        .args([
            "--start",
            "2026-08-24",
            "--end",
            "2026-08-25",
            "--timezone",
            "UTC",
        ])
        .assert()
        .failure()
        .code(74)
        .stderr(predicate::str::contains("\"code\":\"projection_missing\""))
        .stderr(predicate::str::contains(missing.to_string_lossy().as_ref()).not());

    cargo_bin_cmd!("mg-calr")
        .args(["--db", UNWRITABLE_STORE, "agenda", "--todo-projection"])
        .arg(&missing)
        .args([
            "--start",
            "2026-08-24",
            "--end",
            "2026-08-25",
            "--timezone",
            "UTC",
        ])
        .assert()
        .failure()
        .code(74)
        .stderr(predicate::eq(
            "mg-calr: the imported mg-remindr projection is missing\n",
        ))
        .stderr(predicate::str::contains(missing.to_string_lossy().as_ref()).not());
}

#[test]
fn agenda_rejects_invalid_window_and_timezone_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--db",
            UNWRITABLE_STORE,
            "agenda",
            "--start",
            "2026-08-25",
            "--end",
            "2026-08-24",
            "--timezone",
            "Not/AZone",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("\"code\":\"invalid_input\""))
        .stderr(predicate::str::contains("valid IANA timezone"));
}

#[test]
fn agenda_rejects_equal_window_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--db",
            UNWRITABLE_STORE,
            "agenda",
            "--start",
            "2026-08-24",
            "--end",
            "2026-08-24",
            "--timezone",
            "UTC",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains(
            "agenda --start must be before --end",
        ));
}

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
        .args(["--json", "--db", UNWRITABLE_STORE, "init"])
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
            "--db",
            UNWRITABLE_STORE,
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
            "--db",
            UNWRITABLE_STORE,
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

#[test]
fn no_input_project_create_reports_missing_name_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "--db",
            UNWRITABLE_STORE,
            "project",
            "create",
        ])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains(
            "\"code\":\"required_input_missing\"",
        ))
        .stderr(predicate::str::contains("project name"));
}

#[test]
fn event_create_exposes_the_repeat_rule_and_names_what_it_refuses() {
    cargo_bin_cmd!("mg-calr")
        .args(["event", "create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--repeat"))
        .stdout(predicate::str::contains("--by-weekday"))
        .stdout(predicate::str::contains("--count"))
        .stdout(predicate::str::contains("--until"));

    // Each refusal names the flag or the domain rule, never a leaked Debug value
    let calendar = "01a06dc9-bed7-7af2-aaa4-f7e8c22fe49b";
    let timed = [
        "--start",
        "2026-09-07T07:00:00-07:00",
        "--end",
        "2026-09-07T07:30:00-07:00",
        "--timezone",
        "US/Pacific",
        "--title",
        "probe",
        "--calendar",
        calendar,
    ];

    cargo_bin_cmd!("mg-calr")
        .args(["event", "create"])
        .args(timed)
        .args(["--repeat", "yearly", "--count", "3"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "is not a repeat frequency; use daily, weekly, or monthly",
        ));

    cargo_bin_cmd!("mg-calr")
        .args(["event", "create"])
        .args(timed)
        .args([
            "--repeat",
            "weekly",
            "--count",
            "3",
            "--by-weekday",
            "funday",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("is not a weekday; use mon"));

    cargo_bin_cmd!("mg-calr")
        .args(["event", "create"])
        .args(timed)
        .args(["--interval", "2"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--repeat"));

    cargo_bin_cmd!("mg-calr")
        .args(["event", "create"])
        .args(timed)
        .args([
            "--repeat",
            "weekly",
            "--count",
            "3",
            "--until",
            "2026-10-01",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}
