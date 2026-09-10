use chrono::{DateTime, Utc};
use mg_calr::interop::{
    Diagnostic, Lifecycle, Link, Origin, Producer, ProjectionError, Record, Snapshot,
    SnapshotCompleteness, TodoProjectionSnapshot,
};
use serde_json::json;
use std::thread;
use tempfile::tempdir;

fn snapshot() -> Snapshot {
    let observed_at: DateTime<Utc> = "2026-08-24T12:00:00Z".parse().unwrap();
    Snapshot {
        interop_schema: "mg.interop/1".to_owned(),
        kind: "snapshot".to_owned(),
        producer: Producer {
            app: "mg-remindr".to_owned(),
            app_version: "0.1.0".to_owned(),
        },
        export_id: "mg-remindr:snapshot:fixture".to_owned(),
        created_at: observed_at,
        source_revision: "authoritative-revision-7".to_owned(),
        producer_revision: 7,
        completeness: SnapshotCompleteness {
            complete: true,
            expected_records: 1,
            expected_links: 0,
        },
        records: vec![Record {
            global_id: "mg-remindr:todo:todo-1".to_owned(),
            origin: Origin {
                app: "mg-remindr".to_owned(),
                kind: "todo".to_owned(),
                local_id: "todo-1".to_owned(),
            },
            revision: 7,
            observed_at,
            lifecycle: Lifecycle {
                state: "active".to_owned(),
                deleted_at: None,
                tombstoned_at: None,
                trashed_at: None,
                archived_at: None,
                purged: false,
            },
            payload: json!({
                "id": "todo-1",
                "version": 7,
                "parent_id": "todo-parent",
                "recurrence": {"frequency": "DAILY", "interval": 1, "count": 3},
                "reminders": [{"minutes_before": 30, "repeatable": true}],
                "delivery_identity": "delivery-1"
            }),
        }],
        links: vec![],
        provenance: vec!["mg-remindr"]
            .into_iter()
            .map(|source| mg_calr::interop::Provenance {
                source: source.to_owned(),
                boundary: "mg-remindr export".to_owned(),
            })
            .collect(),
        diagnostics: vec![Diagnostic {
            severity: "info".to_owned(),
            code: "fresh".to_owned(),
            message: "fixture".to_owned(),
        }],
    }
}

#[test]
fn validates_mg_todo_only_and_preserves_lossless_payload_metadata() {
    let projection = TodoProjectionSnapshot::validate(snapshot()).unwrap();
    let record = &projection.snapshot().records[0];
    assert_eq!(record.global_id, "mg-remindr:todo:todo-1");
    assert_eq!(record.revision, 7);
    assert_eq!(record.payload["reminders"][0]["minutes_before"], 30);
    assert_eq!(projection.snapshot().diagnostics[0].code, "fresh");
}

#[test]
fn accepts_the_mg_todo_mvp_neutral_export_unchanged() {
    let mut source = snapshot();
    source.records[0].payload = json!({
        "id": "todo-1",
        "title": "ship snapshot",
        "due": null,
        "recurrence": null,
        "reminders": [],
        "priority": "none",
        "project_id": null,
        "tag_ids": [],
        "dependency_ids": [],
        "notes": null,
        "parent_id": null,
        "completed_at": null,
        "trashed_at": null,
        "version": 7,
        "created_at": "2026-08-24T12:00:00Z",
        "updated_at": "2026-08-24T12:00:00Z"
    });
    let json = serde_json::to_string(&source).unwrap();
    let projection = TodoProjectionSnapshot::parse(&json).unwrap();
    let record = &projection.snapshot().records[0];
    assert_eq!(record.payload["title"], "ship snapshot");
    assert!(record.payload["due"].is_null());
    assert_eq!(record.payload["reminders"], json!([]));
}

#[test]
fn rejects_non_todo_producer_and_does_not_replace_existing_store() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    std::fs::write(&path, "sentinel").unwrap();
    let mut invalid = snapshot();
    invalid.producer.app = "mg-calr".to_owned();
    let result = TodoProjectionSnapshot::validate(invalid);
    assert!(matches!(result, Err(ProjectionError::Invalid(_))));
    assert_eq!(std::fs::read_to_string(path).unwrap(), "sentinel");
}

#[test]
fn store_load_and_revision_are_deterministic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("nested/projection.json");
    let projection = TodoProjectionSnapshot::validate(snapshot()).unwrap();
    let revision = projection.revision();
    projection.store(&path).unwrap();
    let loaded = TodoProjectionSnapshot::load(&path).unwrap();
    assert_eq!(loaded.revision(), revision);
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&projection).unwrap()
    );
}

#[test]
fn rejects_relationships_with_missing_endpoints() {
    let mut value = snapshot();
    value.links.push(Link {
        link_id: "mg-remindr:todo:todo-1--todo_parent--mg-remindr:todo:missing".to_owned(),
        source_global_id: "mg-remindr:todo:todo-1".to_owned(),
        target_global_id: "mg-remindr:todo:missing".to_owned(),
        relation: "todo_parent".to_owned(),
        created_by: "mg-remindr".to_owned(),
        created_at: None,
        provenance: "exported relationship".to_owned(),
    });
    value.completeness.expected_links = value.links.len();
    assert!(matches!(
        TodoProjectionSnapshot::validate(value),
        Err(ProjectionError::Invalid(message)) if message.contains("endpoint")
    ));
}

#[test]
fn rejects_noncanonical_identity_and_relationship_metadata() {
    let mut value = snapshot();
    value.records[0].global_id = "mg-remindr:project:todo-1".to_owned();
    assert!(
        matches!(TodoProjectionSnapshot::validate(value), Err(ProjectionError::Invalid(message)) if message.contains("global_id"))
    );

    let mut value = snapshot();
    value.links.push(Link {
        link_id: "mg-remindr:todo:todo-1--todo_tagged--mg-remindr:todo:todo-1".to_owned(),
        source_global_id: "mg-remindr:todo:todo-1".to_owned(),
        target_global_id: "mg-remindr:todo:todo-1".to_owned(),
        relation: "todo_tagged".to_owned(),
        created_by: "other".to_owned(),
        created_at: None,
        provenance: "fixture".to_owned(),
    });
    value.completeness.expected_links = value.links.len();
    assert!(
        matches!(TodoProjectionSnapshot::validate(value), Err(ProjectionError::Invalid(message)) if message.contains("created_by"))
    );
}

/// The bug this guards: the producer's `created_at` is the newest record's
/// `observed_at`, so it stops moving as soon as nothing changes. Judging agenda
/// freshness by it meant a quiet todo list went stale after 24 hours and could
/// never be refreshed out of it — re-importing produced a byte-identical
/// envelope with the same old timestamp, and the whole agenda failed, events
/// included, even though events never come from this projection at all.
#[test]
fn a_just_imported_projection_is_fresh_however_old_its_data_is() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    // The fixture's created_at is 2026-08-24, far outside the 24-hour limit
    TodoProjectionSnapshot::validate(snapshot())
        .unwrap()
        .store(&path)
        .unwrap();

    let loaded = TodoProjectionSnapshot::load(&path).unwrap();
    assert!(
        !matches!(loaded.agenda_todos(), Err(ProjectionError::Stale(_))),
        "a projection imported moments ago must not be judged stale: {:?}",
        loaded.agenda_todos().err()
    );

    // Without a file behind it there is no import time, and the envelope's own
    // timestamp is all there is to judge by
    let parsed = TodoProjectionSnapshot::validate(snapshot()).unwrap();
    assert!(
        matches!(parsed.agenda_todos(), Err(ProjectionError::Stale(_))),
        "a snapshot with no import time still falls back to created_at"
    );
}

#[test]
fn rejects_stale_and_conflicting_replacements() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    let projection = TodoProjectionSnapshot::validate(snapshot()).unwrap();
    projection.store(&path).unwrap();

    let mut stale = snapshot();
    stale.producer_revision = 6;
    stale.created_at = "2026-08-23T12:00:00Z".parse().unwrap();
    stale.records[0].observed_at = stale.created_at;
    assert!(
        matches!(TodoProjectionSnapshot::validate(stale).unwrap().store(&path), Err(ProjectionError::Stale(message)) if message.contains("stale"))
    );

    let mut conflict = snapshot();
    conflict.records[0].payload["title"] = json!("changed");
    assert!(
        matches!(TodoProjectionSnapshot::validate(conflict).unwrap().store(&path), Err(ProjectionError::Conflict(message)) if message.contains("conflicting"))
    );
}

#[test]
fn rejects_a_global_purge_notice_without_record_scoped_evidence() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    TodoProjectionSnapshot::validate(snapshot())
        .unwrap()
        .store(&path)
        .unwrap();

    let mut purged = snapshot();
    purged.producer_revision = 8;
    purged.created_at = "2026-08-25T12:00:00Z".parse().unwrap();
    purged.records.clear();
    purged.completeness.expected_records = 0;
    purged.diagnostics = vec![Diagnostic {
        severity: "info".to_owned(),
        code: "purged_absence".to_owned(),
        message: "Purged rows are absent from the authority export.".to_owned(),
    }];
    assert!(matches!(
        TodoProjectionSnapshot::validate(purged).unwrap().store(&path),
        Err(ProjectionError::Incomplete(message)) if message.contains("disappeared")
    ));
}

#[test]
fn rejects_a_disappeared_record_without_a_purge_diagnostic() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    TodoProjectionSnapshot::validate(snapshot())
        .unwrap()
        .store(&path)
        .unwrap();

    let mut unexplained = snapshot();
    unexplained.producer_revision = 8;
    unexplained.created_at = "2026-08-25T12:00:00Z".parse().unwrap();
    unexplained.records.clear();
    unexplained.completeness.expected_records = 0;
    unexplained.diagnostics.clear();
    assert!(matches!(
        TodoProjectionSnapshot::validate(unexplained).unwrap().store(&path),
        Err(ProjectionError::Incomplete(message)) if message.contains("disappeared")
    ));
}

#[test]
fn rejects_a_purge_explanation_when_a_blocking_diagnostic_is_also_present() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    TodoProjectionSnapshot::validate(snapshot())
        .unwrap()
        .store(&path)
        .unwrap();

    let mut mixed = snapshot();
    mixed.producer_revision = 8;
    mixed.created_at = "2026-08-25T12:00:00Z".parse().unwrap();
    mixed.records.clear();
    mixed.completeness.expected_records = 0;
    mixed.diagnostics = vec![
        Diagnostic {
            severity: "info".to_owned(),
            code: "purged_absence".to_owned(),
            message: "Purged rows are absent from the authority export.".to_owned(),
        },
        Diagnostic {
            severity: "warning".to_owned(),
            code: "partial_read".to_owned(),
            message: "A shard did not respond.".to_owned(),
        },
    ];
    assert!(matches!(
        TodoProjectionSnapshot::validate(mixed).unwrap().store(&path),
        Err(ProjectionError::Incomplete(message)) if message.contains("disappeared")
    ));
}

#[test]
fn rejects_a_disappeared_record_when_a_diagnostic_reports_something_worse() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    let projection = TodoProjectionSnapshot::validate(snapshot()).unwrap();
    projection.store(&path).unwrap();

    let mut degraded = snapshot();
    degraded.producer_revision = 8;
    degraded.created_at = "2026-08-25T12:00:00Z".parse().unwrap();
    degraded.records.clear();
    degraded.completeness.expected_records = 0;
    degraded.diagnostics = vec![Diagnostic {
        severity: "warning".to_owned(),
        code: "partial_read".to_owned(),
        message: "a shard did not respond".to_owned(),
    }];
    assert!(matches!(
        TodoProjectionSnapshot::validate(degraded).unwrap().store(&path),
        Err(ProjectionError::Incomplete(message)) if message.contains("disappeared")
    ));
}

#[test]
fn rejects_invalid_lifecycle_combinations() {
    let mut value = snapshot();
    value.records[0].lifecycle.state = "trashed".to_owned();
    assert!(
        matches!(TodoProjectionSnapshot::validate(value), Err(ProjectionError::Invalid(message)) if message.contains("lifecycle"))
    );
}

#[test]
fn rejects_purged_present_records() {
    let mut value = snapshot();
    value.records[0].lifecycle.purged = true;
    assert!(
        matches!(TodoProjectionSnapshot::validate(value), Err(ProjectionError::Invalid(message)) if message.contains("purged"))
    );
}

#[test]
fn rejects_self_links_and_cycles_in_todo_graphs() {
    let mut self_link = snapshot();
    self_link.links.push(Link {
        link_id: "mg-remindr:todo:todo-1--todo_parent--mg-remindr:todo:todo-1".to_owned(),
        source_global_id: "mg-remindr:todo:todo-1".to_owned(),
        target_global_id: "mg-remindr:todo:todo-1".to_owned(),
        relation: "todo_parent".to_owned(),
        created_by: "mg-remindr".to_owned(),
        created_at: None,
        provenance: "fixture".to_owned(),
    });
    self_link.completeness.expected_links = self_link.links.len();
    assert!(
        matches!(TodoProjectionSnapshot::validate(self_link), Err(ProjectionError::Invalid(message)) if message.contains("itself"))
    );

    let mut cycle = snapshot();
    let mut second = cycle.records[0].clone();
    second.global_id = "mg-remindr:todo:todo-2".to_owned();
    second.origin.local_id = "todo-2".to_owned();
    cycle.records.push(second);
    for (source, target) in [("todo-1", "todo-2"), ("todo-2", "todo-1")] {
        cycle.links.push(Link {
            link_id: format!("mg-remindr:todo:{source}--todo_depends_on--mg-remindr:todo:{target}"),
            source_global_id: format!("mg-remindr:todo:{source}"),
            target_global_id: format!("mg-remindr:todo:{target}"),
            relation: "todo_depends_on".to_owned(),
            created_by: "mg-remindr".to_owned(),
            created_at: None,
            provenance: "fixture".to_owned(),
        });
    }
    cycle.completeness.expected_records = cycle.records.len();
    cycle.completeness.expected_links = cycle.links.len();
    assert!(
        matches!(TodoProjectionSnapshot::validate(cycle), Err(ProjectionError::Invalid(message)) if message.contains("cycle"))
    );
}

#[test]
fn concurrent_store_calls_cannot_stale_overwrite() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("projection.json");
    let mut first = snapshot();
    let mut second = snapshot();
    first.records[0].payload["title"] = json!("first");
    second.records[0].payload["title"] = json!("second");
    let first = TodoProjectionSnapshot::validate(first).unwrap();
    let second = TodoProjectionSnapshot::validate(second).unwrap();
    thread::scope(|scope| {
        let first_path = path.clone();
        let second_path = path.clone();
        let first_handle = scope.spawn(move || first.store(&first_path));
        let second_handle = scope.spawn(move || second.store(&second_path));
        let first_result = first_handle.join().unwrap();
        let second_result = second_handle.join().unwrap();
        assert_eq!(
            usize::from(first_result.is_ok()) + usize::from(second_result.is_ok()),
            1
        );
    });
    let stored = TodoProjectionSnapshot::load(&path).unwrap();
    let title = stored.snapshot().records[0].payload["title"]
        .as_str()
        .unwrap();
    assert!(title == "first" || title == "second");
}

#[test]
fn rejects_records_observed_after_snapshot_creation() {
    let mut value = snapshot();
    value.created_at = "2026-08-24T11:59:59Z".parse().unwrap();
    assert!(
        matches!(TodoProjectionSnapshot::validate(value), Err(ProjectionError::Invalid(message)) if message.contains("observed_at"))
    );
}

#[test]
fn rejects_projection_files_above_the_resource_limit_before_parsing() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("oversized.json");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(16 * 1024 * 1024 + 1).unwrap();
    assert!(
        matches!(TodoProjectionSnapshot::load(&path), Err(ProjectionError::Invalid(message)) if message.contains("byte limit"))
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_projection_files_and_parent_directories() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().unwrap();
    let real_file = directory.path().join("real.json");
    std::fs::write(
        &real_file,
        serde_json::to_vec(&snapshot()).expect("snapshot serializes"),
    )
    .unwrap();
    let linked_file = directory.path().join("linked.json");
    symlink(&real_file, &linked_file).unwrap();
    assert!(matches!(
        TodoProjectionSnapshot::load(&linked_file),
        Err(ProjectionError::Read(_))
    ));

    let projection = TodoProjectionSnapshot::validate(snapshot()).unwrap();
    let dangling_file = directory.path().join("dangling.json");
    symlink(directory.path().join("missing.json"), &dangling_file).unwrap();
    assert!(matches!(
        projection.store(&dangling_file),
        Err(ProjectionError::Write(_))
    ));
    assert!(
        std::fs::symlink_metadata(&dangling_file)
            .unwrap()
            .file_type()
            .is_symlink()
    );

    let real_parent = directory.path().join("real-parent");
    std::fs::create_dir(&real_parent).unwrap();
    let linked_parent = directory.path().join("linked-parent");
    symlink(&real_parent, &linked_parent).unwrap();
    assert!(matches!(
        projection.store(&linked_parent.join("projection.json")),
        Err(ProjectionError::Write(_))
    ));
}

#[test]
fn rejects_unknown_envelope_fields_instead_of_silently_dropping_them() {
    let mut value = serde_json::to_value(snapshot()).unwrap();
    value["unrecognized_authority_field"] = json!("must not disappear");
    assert!(matches!(
        TodoProjectionSnapshot::parse(&serde_json::to_string(&value).unwrap()),
        Err(ProjectionError::Json(_))
    ));
}
