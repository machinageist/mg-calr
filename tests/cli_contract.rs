use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn reminder_scan_contract_is_explicitly_dry_run_capable() {
    cargo_bin_cmd!("mg-calr")
        .args(["todo", "scan-reminders", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--at"))
        .stdout(predicate::str::contains("--dry-run"));
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

#[test]
fn todo_create_no_input_requires_title_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args(["--json", "--no-input", "todo", "create"])
        .assert()
        .failure()
        .code(64)
        .stderr(predicate::str::contains(
            "\"code\":\"required_input_missing\"",
        ))
        .stderr(predicate::str::contains("title"));
}

#[test]
fn todo_import_validates_before_database_access_and_uses_stable_error() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("invalid.json");
    std::fs::write(
        &file,
        r#"{"schema_version":2,"projects":[],"tags":[],"todos":[]}"#,
    )
    .unwrap();
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "todo",
            "import",
            "--file",
        ])
        .arg(file)
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("\"code\":\"import_invalid\""));
}

#[test]
fn todo_create_rejects_timezone_without_due_form_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "todo",
            "create",
            "--title",
            "Write tests",
            "--timezone",
            "America/Los_Angeles",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("\"code\":\"invalid_input\""))
        .stderr(predicate::str::contains("requires --due-date or --due-at"));
}

#[test]
fn todo_create_rejects_mixed_due_forms_at_clap_boundary() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "todo",
            "create",
            "--title",
            "Write tests",
            "--due-date",
            "2026-08-24",
            "--due-at",
            "2026-08-24T09:00:00-07:00",
            "--timezone",
            "America/Los_Angeles",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn todo_show_requires_todo_id_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args(["--json", "--no-input", "todo", "show"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--todo-id"));
}

#[test]
fn todo_complete_requires_id_and_version_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args(["--json", "--no-input", "todo", "complete"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--todo-id"));
}

#[test]
fn todo_edit_accepts_parent_assignment_and_clear_flags() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "todo",
            "edit",
            "--todo-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
            "--parent-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020001",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("database"));
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "todo",
            "edit",
            "--todo-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
            "--clear-parent",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("database"));
}

#[test]
fn todo_edit_requires_id_and_version_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args(["--json", "--no-input", "todo", "edit"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("--todo-id"));
}

#[test]
fn todo_edit_requires_at_least_one_field_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "todo",
            "edit",
            "--todo-id",
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
fn todo_edit_matches_create_due_timezone_validation_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "todo",
            "edit",
            "--todo-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
            "--title",
            "Updated",
            "--timezone",
            "America/Los_Angeles",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("requires --due-date or --due-at"));
}

#[test]
fn todo_edit_rejects_mixed_due_forms_at_clap_boundary() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "todo",
            "edit",
            "--todo-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
            "--due-date",
            "2026-08-24",
            "--due-at",
            "2026-08-24T09:00:00-07:00",
            "--timezone",
            "America/Los_Angeles",
        ])
        .assert()
        .failure()
        .code(2);
}

#[test]
fn todo_trash_and_restore_require_id_and_version_without_database_access() {
    for command in ["trash", "restore"] {
        cargo_bin_cmd!("mg-calr")
            .args(["--json", "--no-input", "todo", command])
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains("--todo-id"));
    }
}

#[test]
fn no_input_project_create_reports_missing_name_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
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
fn todo_purge_requires_explicit_confirmation_without_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "todo",
            "purge",
            "--todo-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("todo purge requires --yes"))
        .stderr(predicate::str::contains("no database was accessed"));
}

#[test]
fn todo_edit_accepts_repeatable_dependency_assignment_and_clear() {
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "todo",
            "edit",
            "--todo-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
            "--depends-on",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020001",
            "--depends-on",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020002",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("database"));
    cargo_bin_cmd!("mg-calr")
        .args([
            "--no-input",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "todo",
            "edit",
            "--todo-id",
            "018fd2c0-2f14-7b1a-9e3b-4abef1020000",
            "--version",
            "1",
            "--clear-dependencies",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("database"));
}
