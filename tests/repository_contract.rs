use std::fs;

#[test]
fn postgres_repository_contract_uses_transactional_parameterized_inserts() {
    let source = fs::read_to_string("src/storage.rs").expect("storage source is available");

    assert!(source.contains("let transaction = client.transaction().await"));
    assert!(source.contains("SELECT deleted_at IS NULL FROM calendars WHERE id = $1 FOR UPDATE"));
    assert!(source.contains("INSERT INTO calendars"));
    assert!(source.contains("INSERT INTO events"));
    assert!(source.contains("VALUES ($1, $2, $3, $4, $5, $6, $7)"));
    assert!(source.contains("VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10"));
    assert!(source.contains("transaction.commit().await"));
    assert!(!source.contains("format!(\"INSERT"));
    assert!(!source.contains("format!(\"SELECT"));
}

#[test]
fn postgres_read_contract_is_parameterized_and_deterministically_ordered() {
    let source = fs::read_to_string("src/storage.rs").expect("storage source is available");

    assert!(source.contains("WHERE e.id = $1"));
    assert!(source.contains("$1::uuid IS NULL OR e.calendar_id = $1"));
    assert!(source.contains("e.all_day_start < $2"));
    assert!(source.contains("e.starts_at < $4"));
    assert!(source.contains("ORDER BY lower(name), id"));
    assert!(source.contains("lower(e.title), e.id"));
    assert!(!source.contains("format!(\"SELECT"));
}

#[test]
fn repository_preserves_standard_event_fields_in_storage_contract() {
    let source = fs::read_to_string("src/storage.rs").expect("storage source is available");

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
fn todo_complete_contract_is_parameterized_and_version_guarded() {
    let source = fs::read_to_string("src/storage.rs").expect("storage source is available");

    assert!(source.contains("UPDATE todos SET completed_at = CURRENT_TIMESTAMP"));
    assert!(source.contains("version = version + 1"));
    assert!(source.contains("WHERE id = $1 AND trashed_at IS NULL"));
    assert!(source.contains("AND deleted_at IS NULL AND completed_at IS NULL AND version = $2"));
    assert!(source.contains("RETURNING id, parent_id, title"));
    assert!(source.contains("TodoVersionConflict"));
    assert!(source.contains("TodoNotFound"));
}

#[test]
fn todo_trash_and_restore_contracts_are_atomic_and_legacy_safe() {
    let source = fs::read_to_string("src/storage.rs").expect("storage source is available");

    assert!(source.contains("UPDATE todos SET trashed_at = CURRENT_TIMESTAMP"));
    assert!(source.contains("UPDATE todos SET trashed_at = NULL, deleted_at = NULL"));
    assert!(source.contains("version = version + 1"));
    assert!(source.contains("updated_at = CURRENT_TIMESTAMP"));
    assert!(source.contains("trashed_at IS NULL AND deleted_at IS NULL AND version = $2"));
    assert!(source.contains("(trashed_at IS NOT NULL OR deleted_at IS NOT NULL) AND version = $2"));
    assert!(source.contains("SELECT version, trashed_at, deleted_at FROM todos WHERE id = $1"));
    assert!(source.contains("COALESCE(trashed_at, deleted_at) AS trashed_at"));
    assert!(!source.contains("SET deleted_at = CURRENT_TIMESTAMP"));
}
