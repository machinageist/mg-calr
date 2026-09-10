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
fn event_cancel_contract_is_parameterized_and_version_guarded() {
    let source = fs::read_to_string("src/storage.rs").expect("storage source is available");

    assert!(source.contains("pub async fn cancel_event"));
    assert!(source.contains("SELECT version, deleted_at FROM events WHERE id = $1 FOR UPDATE"));
    assert!(source.contains("UPDATE events SET deleted_at = CURRENT_TIMESTAMP"));
    assert!(source.contains("version = version + 1"));
    assert!(source.contains("AND deleted_at IS NULL AND version = $2"));
    assert!(source.contains("EventVersionConflict"));
    assert!(source.contains("EventNotFound"));
    let cancelled_check = source
        .find("if row.get::<_, Option<DateTime<Utc>>>(1).is_some()")
        .expect("cancel checks deleted events");
    let version_check = source
        .find("if actual_version != expected_version")
        .expect("cancel checks optimistic version");
    assert!(cancelled_check < version_check);
    assert!(!source.contains("format!(\"UPDATE events"));
}

#[test]
fn event_restore_contract_is_parameterized_and_version_guarded() {
    let source = fs::read_to_string("src/storage.rs").expect("storage source is available");

    assert!(source.contains("pub async fn restore_event"));
    assert!(source.contains("UPDATE events SET deleted_at = NULL"));
    assert!(source.contains("AND deleted_at IS NOT NULL AND version = $2"));
    assert!(source.contains("EventVersionConflict"));
    assert!(source.contains("EventNotFound"));
    let restore_source = &source[source
        .find("pub async fn restore_event")
        .expect("restore implementation exists")..];
    let cancelled_check = restore_source
        .find("if row.get::<_, Option<DateTime<Utc>>>(1).is_none()")
        .expect("restore checks cancelled events");
    let version_check = restore_source
        .find("if actual_version != expected_version")
        .expect("restore checks optimistic version");
    assert!(cancelled_check < version_check);
    assert!(!source.contains("format!(\"UPDATE events"));
}
