use mg_calr::storage::{
    FOUNDATION_MIGRATION, MIGRATIONS, TODO_CORE_MIGRATION, TODO_RECURRENCE_MIGRATION,
};

#[test]
fn foundation_migration_is_embedded_and_covers_only_foundation_entities() {
    assert_eq!(MIGRATIONS.len(), 5);
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

#[test]
fn todo_core_migration_owns_project_schema_without_rewriting_history() {
    assert!(TODO_CORE_MIGRATION.contains("CREATE TABLE IF NOT EXISTS projects"));
    assert!(TODO_CORE_MIGRATION.contains("normalized_name"));
    assert!(TODO_CORE_MIGRATION.contains("projects_normalized_name_unique"));
    assert!(TODO_CORE_MIGRATION.contains("ALTER TABLE todos ADD COLUMN IF NOT EXISTS project_id"));
    assert!(!TODO_CORE_MIGRATION.contains("DROP TABLE projects"));
}

#[test]
fn recurrence_migration_is_parameterized_foundation_and_non_destructive() {
    assert_eq!(MIGRATIONS[2].version, 3);
    assert_eq!(MIGRATIONS[2].sql, TODO_RECURRENCE_MIGRATION);
    assert!(TODO_RECURRENCE_MIGRATION.contains("recurrence_rule jsonb"));
    assert!(TODO_RECURRENCE_MIGRATION.contains("todos_recurrence_rule_check"));
    assert!(!TODO_RECURRENCE_MIGRATION.contains("DROP TABLE"));
}

#[test]
fn reminder_migration_bridges_delivery_identity_without_external_transport() {
    let sql = mg_calr::storage::TODO_REMINDERS_MIGRATION;
    assert!(sql.contains("ALTER TABLE reminders ADD COLUMN IF NOT EXISTS repeatable"));
    assert!(sql.contains("reminders_todo_schedule_unique"));
    assert!(sql.contains("WITH duplicate_deliveries AS"));
    assert!(sql.contains("DELETE FROM reminder_deliveries delivery"));
    assert!(sql.contains("earlier.id < delivery.id"));
    assert!(sql.contains("UPDATE reminder_deliveries delivery"));
    assert!(sql.contains("MIN(id::text)::uuid AS keeper_id"));
    assert!(sql.contains("duplicate.id <> canonical.keeper_id"));
    assert!(mg_calr::storage::FOUNDATION_MIGRATION.contains("UNIQUE (reminder_id, scheduled_for)"));
    assert!(!sql.to_ascii_lowercase().contains("notify"));
}
