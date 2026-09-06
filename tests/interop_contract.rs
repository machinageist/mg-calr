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

/// Guards one property with no cheap behavioural proxy: a snapshot must be read
/// under a single repeatable-read transaction, or it can interleave writes and
/// describe a state the database never held. Proving that from the outside needs
/// a concurrent writer racing an export, which is slower and flakier than reading
/// the isolation level the code asks for.
///
/// Determinism and date handling used to be asserted here by grepping the source
/// too. They are behaviour, and are now tested as behaviour in
/// `postgres_integration.rs::snapshot_identity_is_deterministic_and_dates_are_not_invented`
/// and end to end in `geistos/tests/suite-pipe.sh`.
#[test]
fn snapshot_is_read_under_one_repeatable_read_transaction() {
    let storage = include_str!("../src/storage.rs");
    assert!(storage.contains("IsolationLevel::RepeatableRead"));
    assert!(storage.contains("export_snapshot_sources"));
}
