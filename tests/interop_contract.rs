use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn interop_export_help_exposes_json_snapshot_command() {
    cargo_bin_cmd!("mg-calr")
        .args(["interop", "export", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Export calendars, events, projects, tags, todos",
        ))
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn interop_export_requires_json_before_database_access() {
    cargo_bin_cmd!("mg-calr")
        .args(["interop", "export"])
        .assert()
        .failure()
        .code(65)
        .stderr(predicate::str::contains("interop export requires --json"));
}

#[test]
fn snapshot_contract_uses_one_repeatable_read_and_deterministic_identity() {
    let storage = include_str!("../src/storage.rs");
    let interop = include_str!("../src/interop.rs");
    assert!(storage.contains("IsolationLevel::RepeatableRead"));
    assert!(storage.contains("export_snapshot_sources"));
    assert!(interop.contains("Sha256::digest"));
    assert!(interop.contains("source_revision: digest"));
    assert!(!interop.contains("created_at: Utc::now()"));
}

#[test]
fn relationship_contract_does_not_invent_creation_times() {
    let interop = include_str!("../src/interop.rs");
    assert!(interop.contains("created_at: None"));
    assert!(interop.contains("creation time unavailable"));
    assert!(interop.contains("purged_absence"));
    assert!(interop.contains("state"));
}
