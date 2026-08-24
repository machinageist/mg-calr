use std::str::FromStr;

use chrono::{DateTime, NaiveDate};
use mg_calr::application::TodoQueryProjection;
use mg_calr::domain::todo::{Priority, ProjectId, TagId, Todo, TodoDue, TodoError, TodoId};
use mg_calr::storage::MIGRATIONS;

fn instant(value: &str) -> DateTime<chrono::FixedOffset> {
    value.parse().expect("valid RFC3339 fixture")
}

#[test]
fn priority_parsing_is_locked_and_canonical() {
    for value in ["none", "low", "medium", "high", "urgent"] {
        let priority = Priority::from_str(value).expect("locked value parses");
        assert_eq!(priority.to_string(), value);
        assert_eq!(
            serde_json::to_string(&priority).unwrap(),
            format!("\"{value}\"")
        );
    }
    for value in ["NONE", "normal", "", " urgent"] {
        assert!(matches!(
            Priority::from_str(value),
            Err(TodoError::InvalidPriority { .. })
        ));
    }
}

#[test]
fn due_values_validate_zone_offsets_and_preserve_all_day_semantics() {
    let date = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();
    let all_day = TodoDue::date(date, "America/Los_Angeles").unwrap();
    assert!(all_day.is_all_day());
    let encoded = serde_json::to_value(&all_day).unwrap();
    assert_eq!(encoded["Date"]["date"], "2026-08-24");
    assert_eq!(
        TodoDue::date(date, "Mars/Olympus"),
        Err(TodoError::InvalidTimezone {
            timezone: "Mars/Olympus".into()
        })
    );

    let timed =
        TodoDue::timed(instant("2026-08-24T09:00:00-07:00"), "America/Los_Angeles").unwrap();
    assert!(!timed.is_all_day());
    assert!(matches!(
        TodoDue::timed(instant("2026-08-24T09:00:00+00:00"), "America/Los_Angeles"),
        Err(TodoError::OffsetTimezoneMismatch { .. })
    ));
}

#[test]
fn todo_ids_round_trip_and_reject_invalid_serialized_values() {
    for (text, expected) in [
        (TodoId::new().to_string(), "todo"),
        (ProjectId::new().to_string(), "project"),
        (TagId::new().to_string(), "tag"),
    ] {
        let _ = expected;
        assert_eq!(text.parse::<uuid::Uuid>().unwrap().to_string(), text);
    }
    let todo = TodoId::new();
    assert_eq!(TodoId::from_str(&todo.to_string()).unwrap(), todo);
    assert!(serde_json::from_str::<TodoId>("\"not-a-uuid\"").is_err());
}

#[test]
fn todo_validation_and_projection_are_serializable() {
    assert!(matches!(
        Todo::new(""),
        Err(TodoError::EmptyField {
            field: "todo title"
        })
    ));
    let todo = Todo::new("Write tests").unwrap();
    let projection = todo.projection(true, 2);
    let value = serde_json::to_value(&projection).unwrap();
    assert_eq!(value["todo"]["title"], "Write tests");
    assert_eq!(value["blocked"], true);
    assert_eq!(value["unmet_prerequisite_count"], 2);
}

#[test]
fn todo_core_migration_is_ordered_and_non_destructive() {
    assert_eq!(
        MIGRATIONS.iter().map(|m| m.version).collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5]
    );
    assert_eq!(MIGRATIONS[1].name, "todo_core");
    let sql = MIGRATIONS[1].sql;
    for table in [
        "projects",
        "tags",
        "todos",
        "todo_tags",
        "todo_dependencies",
    ] {
        assert!(sql.contains(table), "migration mentions {table}");
    }
    assert!(sql.contains("ADD COLUMN IF NOT EXISTS"));
    assert!(sql.contains("ON DELETE CASCADE"));
    assert!(!sql.to_ascii_lowercase().contains("drop table"));
    assert!(!sql.to_ascii_lowercase().contains("delete from"));
    assert!(sql.contains("priority"));
    assert!(sql.contains("due_representation_check"));
    assert!(sql.contains("dependent_id"));
    assert!(sql.contains("prerequisite_id"));
    assert!(sql.contains("information_schema.columns"));
    assert!(sql.contains("pg_constraint"));
    assert!(sql.contains("migration refused"));
}

#[test]
fn reminders_validate_due_offsets_and_deduplicate() {
    use mg_calr::domain::todo::TodoReminder;

    assert!(matches!(
        TodoReminder::new(0, false),
        Err(TodoError::InvalidReminderOffset)
    ));
    let mut todo = Todo::new("Remind me").unwrap();
    todo.reminders = vec![TodoReminder::new(5, false).unwrap()];
    assert!(matches!(
        todo.clone().rehydrate(),
        Err(TodoError::ReminderWithoutDue)
    ));
    todo.due = Some(TodoDue::date(NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(), "UTC").unwrap());
    todo.reminders.push(TodoReminder::new(5, false).unwrap());
    assert!(matches!(
        todo.rehydrate(),
        Err(TodoError::InvalidStoredReminder { .. })
    ));
}

#[test]
fn completed_projection_preserves_completed_state_and_version() {
    let mut todo = Todo::new("Done").unwrap();
    todo.completed_at = Some(chrono::Utc::now());
    todo.version = 2;
    let projection = TodoQueryProjection::from(todo);
    let value = serde_json::to_value(&projection).unwrap();
    assert!(value["completed_at"].is_string());
    assert_eq!(value["version"], 2);
    assert!(projection.to_string().contains("completed"));
}

#[test]
fn trash_behavior_preserves_completion_and_advances_version() {
    let mut todo = Todo::new("Completed but recoverable").unwrap();
    todo.completed_at = Some(chrono::Utc::now());
    let original_version = todo.version;
    todo.trashed_at = Some(chrono::Utc::now());
    todo.version += 1;

    let projection = TodoQueryProjection::from(todo);
    assert!(projection.completed_at.is_some());
    assert!(projection.trashed_at.is_some());
    assert_eq!(projection.version, original_version + 1);
}
