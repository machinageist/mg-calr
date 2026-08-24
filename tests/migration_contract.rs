use mg_calr::storage::{FOUNDATION_MIGRATION, MIGRATIONS};

#[test]
fn foundation_migration_is_embedded_and_covers_only_foundation_entities() {
    assert_eq!(MIGRATIONS.len(), 1);
    assert_eq!(MIGRATIONS[0].version, 1);
    assert_eq!(MIGRATIONS[0].sql, FOUNDATION_MIGRATION);

    for table in [
        "calendars",
        "events",
        "todos",
        "reminders",
        "reminder_deliveries",
        "audit_log",
    ] {
        assert!(
            FOUNDATION_MIGRATION.contains(&format!("CREATE TABLE {table}")),
            "missing foundation table {table}"
        );
    }

    assert!(!FOUNDATION_MIGRATION.contains("DROP TABLE"));
    assert!(!FOUNDATION_MIGRATION.contains("CREATE EXTENSION"));
}

#[test]
fn migration_versions_are_strictly_increasing_and_unique() {
    for pair in MIGRATIONS.windows(2) {
        assert!(pair[0].version < pair[1].version);
    }
}
