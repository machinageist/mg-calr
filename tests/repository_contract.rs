// Author: Jeff
// Date: 2026-09-20
// Description: How the store is allowed to talk to SQLite — parameters, transactions, and the
//              order in which a lifecycle write checks things
// Notes: These read the source, because the point is the shape of the statements, not their
//        results. Statements are never built with format!: a value reaches SQLite as a bound
//        parameter or not at all

use std::fs;

fn storage_source() -> String {
    fs::read_to_string("src/storage.rs").expect("storage source is available")
}

#[test]
fn writes_are_transactional_and_parameterized() {
    let source = storage_source();

    assert!(source.contains("transaction_with_behavior(TransactionBehavior::Immediate)"));
    assert!(source.contains("INSERT INTO calendars"));
    assert!(source.contains("INSERT INTO events"));
    assert!(source.contains("transaction.commit()"));
    // a calendar must be live before an event may point at it, checked inside the write
    assert!(source.contains("SELECT deleted_at IS NULL FROM calendars WHERE id = ?1"));
    assert!(!source.contains("format!(\"INSERT"));
    assert!(!source.contains("format!(\"UPDATE events"));
}

#[test]
fn reads_are_parameterized_and_deterministically_ordered() {
    let source = storage_source();

    assert!(source.contains("WHERE e.id = ?1"));
    assert!(source.contains("ORDER BY lower(name), id"));
    assert!(source.contains("lower(e.title), e.id"));
    assert!(!source.contains("format!(\"SELECT"));
}

#[test]
fn every_standard_event_field_is_persisted() {
    let source = storage_source();

    for field in [
        "rfc_uid",
        "description",
        "location",
        "url",
        "status",
        "busy",
        "timezone",
        "starts_at",
        "ends_at",
        "all_day_start",
        "all_day_end",
        "recurrence_rule",
        "extension_properties",
        "created_at",
        "updated_at",
        "deleted_at",
        "remote_tombstoned_at",
    ] {
        assert!(
            source.contains(field),
            "missing persisted event field: {field}"
        );
    }
}

#[test]
fn a_lifecycle_write_answers_the_lifecycle_before_the_version() {
    let source = storage_source();

    assert!(source.contains("pub fn cancel_event"));
    assert!(source.contains("pub fn restore_event"));
    assert!(source.contains("SELECT version, deleted_at FROM events WHERE id = ?1"));
    assert!(source.contains("version = version + 1"));
    assert!(source.contains("WHERE id = ?1 AND version = ?2"));

    // a caller holding a stale version is told what is actually wrong: that the event
    // is already cancelled, or already live, rather than that its version is old
    let write = &source[source
        .find("fn set_event_deletion")
        .expect("the shared lifecycle write exists")..];
    let lifecycle_check = write
        .find("if deleted_at.is_some() == cancelling")
        .expect("the write checks lifecycle");
    let version_check = write
        .find("if actual_version != expected_version")
        .expect("the write checks the optimistic version");
    assert!(lifecycle_check < version_check);
}
