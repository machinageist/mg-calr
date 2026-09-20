// Author: Jeff
// Date: 2026-09-20
// Description: What mg-calr's embedded schema is allowed to be — append-only, and exactly the
//              tables each migration claims
// Notes: The PostgreSQL store reached nine migrations because each shipped separately. The
//        SQLite store starts from two, and the rules that kept those nine honest still hold

use mg_calr::storage::{FOUNDATION_SQL, MIGRATIONS, PROJECTS_AND_TAGS_SQL};

#[test]
fn the_foundation_owns_calendars_and_events_and_nothing_else() {
    assert_eq!(MIGRATIONS.len(), 2);
    assert_eq!(MIGRATIONS[0].version, 1);
    assert_eq!(MIGRATIONS[0].name, "foundation");
    assert_eq!(MIGRATIONS[0].sql, FOUNDATION_SQL);
    assert_eq!(MIGRATIONS[0].tables, ["calendars", "events"]);

    assert_eq!(MIGRATIONS[1].version, 2);
    assert_eq!(MIGRATIONS[1].name, "projects_and_tags");
    assert_eq!(MIGRATIONS[1].sql, PROJECTS_AND_TAGS_SQL);
    assert_eq!(MIGRATIONS[1].tables, ["projects", "tags"]);
}

#[test]
fn every_migration_creates_exactly_the_tables_its_ledger_entry_names() {
    for migration in MIGRATIONS {
        for table in migration.tables {
            assert!(
                migration.sql.contains(&format!("CREATE TABLE {table}")),
                "migration {} claims {table} without creating it",
                migration.version
            );
        }
        assert_eq!(
            migration.sql.matches("CREATE TABLE ").count(),
            migration.tables.len(),
            "migration {} creates a table its ledger entry does not name",
            migration.version
        );
    }
}

#[test]
fn migrations_are_append_only_and_never_destructive() {
    for migration in MIGRATIONS {
        // IF NOT EXISTS would let an edited migration pass over a store it does not match
        assert!(!migration.sql.contains("IF NOT EXISTS"));
        assert!(!migration.sql.contains("DROP "));
        assert!(!migration.sql.contains("DELETE FROM"));
        assert!(!migration.sql.contains("ALTER TABLE"));
    }
}

#[test]
fn the_foundation_keeps_the_invariants_the_domain_relies_on() {
    // one event is wholly timed or wholly all-day, which is what EventTime is
    assert!(FOUNDATION_SQL.contains("CHECK ((timezone IS NOT NULL AND starts_at IS NOT NULL"));
    assert!(FOUNDATION_SQL.contains("CHECK (ends_at IS NULL OR ends_at > starts_at)"));
    assert!(FOUNDATION_SQL.contains("CHECK (version >= 1)"));
    // one live default calendar, and a deleted one releases the claim
    assert!(FOUNDATION_SQL.contains("CREATE UNIQUE INDEX calendars_one_default"));
    // two spellings of one name cannot become two records
    assert!(PROJECTS_AND_TAGS_SQL.contains("projects_normalized_name_unique"));
    assert!(PROJECTS_AND_TAGS_SQL.contains("tags_normalized_name_unique"));
}

#[test]
fn migration_versions_are_strictly_increasing_and_unique() {
    for pair in MIGRATIONS.windows(2) {
        assert!(pair[0].version < pair[1].version);
    }
}
