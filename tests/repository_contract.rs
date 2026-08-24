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
