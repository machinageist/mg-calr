use assert_cmd::cargo::cargo_bin_cmd;
use chrono::Utc;
use mg_calr::application::{AgendaKind, AgendaOutput, AgendaQuery};
use mg_calr::domain::todo::{Todo, TodoDue};
use mg_calr::domain::{Calendar, Event, EventTime};
use mg_calr::interop::{
    Lifecycle, Link, Origin, Producer, ProjectionError, Record, Snapshot, SnapshotCompleteness,
    TodoProjectionSnapshot,
};
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::tempdir;

fn date(value: &str) -> chrono::NaiveDate {
    value.parse().expect("valid date fixture")
}

fn projection_with(todos: &[Todo], links: Vec<Link>) -> TodoProjectionSnapshot {
    let created_at = Utc::now();
    TodoProjectionSnapshot::validate(Snapshot {
        interop_schema: "mg.interop/1".to_owned(),
        kind: "snapshot".to_owned(),
        producer: Producer {
            app: "mg-todo".to_owned(),
            app_version: "0.1.0".to_owned(),
        },
        export_id: "mg-todo:snapshot:agenda-fixture".to_owned(),
        created_at,
        source_revision: "agenda-revision-1".to_owned(),
        producer_revision: 1,
        completeness: SnapshotCompleteness {
            complete: true,
            expected_records: todos.len(),
            expected_links: links.len(),
        },
        records: todos
            .iter()
            .map(|todo| Record {
                global_id: format!("mg-todo:todo:{}", todo.id),
                origin: Origin {
                    app: "mg-todo".to_owned(),
                    kind: "todo".to_owned(),
                    local_id: todo.id.to_string(),
                },
                revision: todo.version,
                observed_at: todo.updated_at,
                lifecycle: Lifecycle {
                    state: if todo.trashed_at.is_some() {
                        "trashed".to_owned()
                    } else {
                        "active".to_owned()
                    },
                    deleted_at: None,
                    tombstoned_at: None,
                    trashed_at: todo.trashed_at,
                    archived_at: None,
                    purged: false,
                },
                payload: serde_json::to_value(todo).unwrap(),
            })
            .collect(),
        links,
        provenance: Vec::new(),
        diagnostics: Vec::new(),
    })
    .unwrap()
}

fn dependency_link(dependent: &Todo, prerequisite: &Todo) -> Link {
    let source = format!("mg-todo:todo:{}", dependent.id);
    let target = format!("mg-todo:todo:{}", prerequisite.id);
    Link {
        link_id: format!("{source}--todo_depends_on--{target}"),
        source_global_id: source,
        target_global_id: target,
        relation: "todo_depends_on".to_owned(),
        created_by: "mg-todo".to_owned(),
        created_at: None,
        provenance: "fixture".to_owned(),
    }
}

#[test]
fn agenda_todos_are_rehydrated_only_from_the_validated_projection() {
    let mut prerequisite = Todo::new("Prerequisite").unwrap();
    prerequisite.updated_at = "2026-08-24T11:00:00Z".parse().unwrap();
    let mut dependent = Todo::new("Projected agenda item").unwrap();
    dependent.updated_at = "2026-08-24T11:30:00Z".parse().unwrap();
    dependent.due = Some(TodoDue::date("2026-08-24".parse().unwrap(), "UTC").unwrap());
    dependent.dependency_ids = vec![prerequisite.id];

    let projection = projection_with(
        &[prerequisite.clone(), dependent.clone()],
        vec![dependency_link(&dependent, &prerequisite)],
    );
    let loaded = projection.agenda_todos().unwrap();

    assert_eq!(loaded.todos, vec![prerequisite, dependent]);
}

#[test]
fn projection_lifecycle_and_agenda_flags_filter_truthfully() {
    let mut active = Todo::new("Active").unwrap();
    active.updated_at = "2026-08-24T10:00:00Z".parse().unwrap();
    active.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());
    let mut trashed = Todo::new("Trashed").unwrap();
    trashed.updated_at = "2026-08-24T10:30:00Z".parse().unwrap();
    trashed.trashed_at = Some("2026-08-24T10:15:00Z".parse().unwrap());
    trashed.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());
    let mut deleted = Todo::new("Deleted").unwrap();
    deleted.updated_at = "2026-08-24T11:00:00Z".parse().unwrap();
    deleted.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());

    let mut projection = projection_with(&[active, trashed], Vec::new());
    let mut deleted_record = projection_with(&[deleted], Vec::new())
        .snapshot()
        .records
        .clone()
        .remove(0);
    deleted_record.lifecycle.state = "deleted".to_owned();
    deleted_record.lifecycle.deleted_at = Some("2026-08-24T10:45:00Z".parse().unwrap());
    let mut raw = projection.snapshot().clone();
    raw.records.push(deleted_record);
    raw.completeness.expected_records = raw.records.len();
    projection = TodoProjectionSnapshot::validate(raw).unwrap();
    let projected = projection.agenda_todos().unwrap();
    assert_eq!(
        projected
            .todos
            .iter()
            .map(|todo| todo.title.as_str())
            .collect::<Vec<_>>(),
        ["Active", "Trashed"]
    );

    let default = AgendaOutput::from_projection(
        AgendaQuery::new(date("2026-08-24"), date("2026-08-25")),
        Vec::new(),
        projected.clone(),
    )
    .unwrap();
    assert_eq!(default.items.len(), 1);
    assert_eq!(default.items[0].title, "Active");

    let mut include_trashed = AgendaQuery::new(date("2026-08-24"), date("2026-08-25"));
    include_trashed.include_trashed = true;
    let inclusive = AgendaOutput::from_projection(include_trashed, Vec::new(), projected).unwrap();
    assert_eq!(
        inclusive
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        ["Active", "Trashed"]
    );
}

#[test]
fn current_projection_composes_with_events_in_stable_json_order() {
    let calendar = Calendar::new("Work").unwrap();
    let event = Event::new(
        calendar.id,
        "Meeting",
        EventTime::all_day(date("2026-08-24"), date("2026-08-25")).unwrap(),
    )
    .unwrap();
    let mut zulu = Todo::new("Zulu").unwrap();
    zulu.updated_at = "2026-08-24T10:00:00Z".parse().unwrap();
    zulu.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());
    let mut alpha = Todo::new("alpha").unwrap();
    alpha.updated_at = "2026-08-24T11:00:00Z".parse().unwrap();
    alpha.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());
    let projection = projection_with(&[zulu, alpha], Vec::new());
    let query = AgendaQuery::new(date("2026-08-24"), date("2026-08-25"));

    let first = AgendaOutput::from_projection(
        query.clone(),
        vec![event.clone()],
        projection.agenda_todos().unwrap(),
    )
    .unwrap();
    let second =
        AgendaOutput::from_projection(query, vec![event], projection.agenda_todos().unwrap())
            .unwrap();

    assert_eq!(
        first
            .items
            .iter()
            .map(|item| (&item.kind, item.title.as_str()))
            .collect::<Vec<_>>(),
        [
            (&AgendaKind::Event, "Meeting"),
            (&AgendaKind::Todo, "alpha"),
            (&AgendaKind::Todo, "Zulu"),
        ]
    );
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
}

#[test]
fn empty_current_projection_preserves_calendar_only_agenda() {
    let calendar = Calendar::new("Work").unwrap();
    let event = Event::new(
        calendar.id,
        "Calendar only",
        EventTime::all_day(date("2026-08-24"), date("2026-08-25")).unwrap(),
    )
    .unwrap();
    let projection = projection_with(&[], Vec::new());

    let output = AgendaOutput::from_projection(
        AgendaQuery::new(date("2026-08-24"), date("2026-08-25")),
        vec![event],
        projection.agenda_todos().unwrap(),
    )
    .unwrap();

    assert_eq!(output.items.len(), 1);
    assert_eq!(output.items[0].kind, AgendaKind::Event);
    assert_eq!(output.items[0].title, "Calendar only");
}

#[test]
fn agenda_projection_reports_missing_stale_and_conflicting_state_explicitly() {
    let directory = tempdir().unwrap();
    let missing = directory.path().join("missing.json");
    assert!(matches!(
        TodoProjectionSnapshot::load(&missing),
        Err(ProjectionError::Missing)
    ));

    let mut todo = Todo::new("Stale").unwrap();
    todo.updated_at = "2026-08-24T11:00:00Z".parse().unwrap();
    let mut stale = projection_with(&[todo.clone()], Vec::new());
    let mut raw = stale.snapshot().clone();
    raw.records[0].revision += 1;
    stale = TodoProjectionSnapshot::validate(raw).unwrap();
    assert!(matches!(
        stale.agenda_todos(),
        Err(ProjectionError::Stale(message)) if message.contains("revision")
    ));

    let mut prerequisite = Todo::new("Prerequisite").unwrap();
    prerequisite.updated_at = "2026-08-24T11:30:00Z".parse().unwrap();
    todo.dependency_ids = vec![prerequisite.id];
    let conflicting = projection_with(&[todo, prerequisite], Vec::new());
    assert!(matches!(
        conflicting.agenda_todos(),
        Err(ProjectionError::Conflict(message)) if message.contains("relationship")
    ));
}

#[test]
fn agenda_cli_reports_stale_projection_without_leaking_its_path() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("private-stale-projection.json");
    let mut todo = Todo::new("Stale").unwrap();
    todo.updated_at = "2026-08-24T11:00:00Z".parse().unwrap();
    let mut stale = projection_with(&[todo], Vec::new());
    let mut raw = stale.snapshot().clone();
    raw.records[0].revision += 1;
    stale = TodoProjectionSnapshot::validate(raw).unwrap();
    stale.store(&path).unwrap();

    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "agenda",
            "--todo-projection",
        ])
        .arg(&path)
        .args([
            "--start",
            "2026-08-24",
            "--end",
            "2026-08-25",
            "--timezone",
            "UTC",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(contains("\"code\":\"projection_stale\""))
        .stderr(contains(path.to_string_lossy().as_ref()).not());
}

#[test]
fn agenda_cli_reports_projection_conflict_without_leaking_its_path() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("private-conflicting-projection.json");
    let mut prerequisite = Todo::new("Prerequisite").unwrap();
    prerequisite.updated_at = "2026-08-24T10:00:00Z".parse().unwrap();
    let mut dependent = Todo::new("Dependent").unwrap();
    dependent.updated_at = "2026-08-24T11:00:00Z".parse().unwrap();
    dependent.dependency_ids = vec![prerequisite.id];
    let conflicting = projection_with(&[dependent, prerequisite], Vec::new());
    conflicting.store(&path).unwrap();

    cargo_bin_cmd!("mg-calr")
        .args([
            "--json",
            "--database-url",
            "postgresql://127.0.0.1:1/mg_calr",
            "agenda",
            "--todo-projection",
        ])
        .arg(&path)
        .args([
            "--start",
            "2026-08-24",
            "--end",
            "2026-08-25",
            "--timezone",
            "UTC",
        ])
        .assert()
        .failure()
        .code(65)
        .stderr(contains("\"code\":\"projection_conflict\""))
        .stderr(contains(path.to_string_lossy().as_ref()).not());
}

#[test]
fn agenda_repository_source_has_no_legacy_todo_database_read() {
    let source = include_str!("../src/storage.rs");
    let implementation = source
        .split("impl AsyncAgendaRepository for ProjectionAgendaRepository")
        .nth(1)
        .expect("projection agenda repository implementation");
    let implementation = implementation
        .split("/// Export all todo-related state")
        .next()
        .unwrap();
    assert!(implementation.contains("TodoProjectionSnapshot::load"));
    assert!(!implementation.contains("PostgresTodoRepository"));
    assert!(!implementation.contains("list_todos_with_trashed"));
}
