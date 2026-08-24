use mg_calr::storage::{
    FOUNDATION_MIGRATION, MIGRATIONS, TODO_CORE_MIGRATION, TODO_RECURRENCE_MIGRATION,
};

#[test]
fn foundation_migration_is_embedded_and_covers_only_foundation_entities() {
    assert_eq!(MIGRATIONS.len(), 3);
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
