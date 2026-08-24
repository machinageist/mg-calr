use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

use crate::config::ConnectionSettings;
use crate::storage::{StorageError, export_snapshot_sources};

const INTEROP_SCHEMA: &str = "mg.interop/1";
const PRODUCER_APP: &str = "mg-calr";

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub interop_schema: &'static str,
    pub kind: &'static str,
    pub producer: Producer,
    pub export_id: String,
    pub created_at: DateTime<Utc>,
    pub source_revision: String,
    pub records: Vec<Record>,
    pub links: Vec<Link>,
    pub provenance: Vec<Provenance>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Serialize)]
pub struct Producer {
    pub app: &'static str,
    pub app_version: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Record {
    pub global_id: String,
    pub origin: Origin,
    pub revision: i64,
    pub observed_at: DateTime<Utc>,
    pub lifecycle: Lifecycle,
    pub payload: Value,
}
#[derive(Debug, Serialize)]
pub struct Origin {
    pub app: &'static str,
    pub kind: &'static str,
    pub local_id: String,
}
#[derive(Debug, Serialize)]
pub struct Lifecycle {
    pub state: &'static str,
    pub deleted_at: Option<DateTime<Utc>>,
    pub tombstoned_at: Option<DateTime<Utc>>,
    pub purged: bool,
}
#[derive(Debug, Serialize)]
pub struct Link {
    pub link_id: String,
    pub source_global_id: String,
    pub target_global_id: String,
    pub relation: &'static str,
    pub created_by: &'static str,
    /// Relationship tables do not store link creation time; never infer it.
    pub created_at: Option<DateTime<Utc>>,
    pub provenance: String,
}
#[derive(Debug, Serialize)]
pub struct Provenance {
    pub source: &'static str,
    pub boundary: &'static str,
}
#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub code: &'static str,
    pub message: String,
}

/// Read all authoritative calendar state under one repeatable-read transaction.
#[allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines
)]
pub async fn export_snapshot(settings: &ConnectionSettings) -> Result<Snapshot, StorageError> {
    let (events, todos, todo_deleted) = export_snapshot_sources(settings).await?;
    let mut records = Vec::new();
    let mut links = Vec::new();
    let mut diagnostics = Vec::new();
    for calendar in events.calendars {
        let state = if calendar.deleted_at.is_some() {
            "deleted"
        } else {
            "active"
        };
        records.push(record(
            "calendar",
            calendar.id.to_string(),
            1,
            calendar.updated_at,
            Lifecycle {
                state,
                deleted_at: calendar.deleted_at,
                tombstoned_at: None,
                purged: false,
            },
            &calendar,
        ));
    }
    for event in events.events {
        let id = event.id.to_string();
        let state = if event.deleted_at.is_some() {
            "deleted"
        } else if event.remote_tombstoned_at.is_some() {
            "tombstoned"
        } else {
            "active"
        };
        let event_global = global_id("event", &id);
        links.push(link(
            &global_id("calendar", &event.calendar_id.to_string()),
            &event_global,
            "calendar_contains_event",
        ));
        records.push(record(
            "event",
            id,
            event.version,
            event.updated_at,
            Lifecycle {
                state,
                deleted_at: event.deleted_at,
                tombstoned_at: event.remote_tombstoned_at,
                purged: false,
            },
            &event,
        ));
    }
    for project in todos.projects {
        let state = if project.archived_at.is_some() {
            "archived"
        } else {
            "active"
        };
        records.push(record(
            "project",
            project.id.to_string(),
            project.version,
            project.updated_at,
            Lifecycle {
                state,
                deleted_at: None,
                tombstoned_at: None,
                purged: false,
            },
            &project,
        ));
    }
    for tag in todos.tags {
        records.push(record(
            "tag",
            tag.id.to_string(),
            1,
            tag.updated_at,
            Lifecycle {
                state: "active",
                deleted_at: None,
                tombstoned_at: None,
                purged: false,
            },
            &tag,
        ));
    }
    for todo in todos.todos {
        let id = todo.id.to_string();
        let todo_global = global_id("todo", &id);
        let deleted_at = todo_deleted.get(&todo.id.as_uuid()).copied().flatten();
        let state = if deleted_at.is_some() {
            "deleted"
        } else if todo.trashed_at.is_some() {
            "trashed"
        } else {
            "active"
        };
        if let Some(project_id) = todo.project_id {
            links.push(link(
                &global_id("project", &project_id.to_string()),
                &todo_global,
                "project_contains_todo",
            ));
        }
        if let Some(parent_id) = todo.parent_id {
            links.push(link(
                &global_id("todo", &parent_id.to_string()),
                &todo_global,
                "todo_parent",
            ));
        }
        for dependency_id in &todo.dependency_ids {
            links.push(link(
                &todo_global,
                &global_id("todo", &dependency_id.to_string()),
                "todo_depends_on",
            ));
        }
        for tag_id in &todo.tag_ids {
            links.push(link(
                &todo_global,
                &global_id("tag", &tag_id.to_string()),
                "todo_tagged",
            ));
        }
        records.push(record(
            "todo",
            id,
            todo.version,
            todo.updated_at,
            Lifecycle {
                state,
                deleted_at,
                tombstoned_at: None,
                purged: false,
            },
            &todo,
        ));
    }
    // Purged rows are absent by definition; explicitly document that absence
    // instead of manufacturing tombstones or pretending history is complete.
    diagnostics.push(Diagnostic { severity: "info", code: "purged_absence", message: "Purged rows are not present in authoritative exports; absence is not interpreted as deletion.".to_owned() });
    records.sort_by(|a, b| a.global_id.cmp(&b.global_id));
    links.sort_by(|a, b| a.link_id.cmp(&b.link_id));
    let created_at = records
        .iter()
        .map(|r| r.observed_at)
        .max()
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    let provenance = vec![Provenance {
        source: PRODUCER_APP,
        boundary: "authoritative read-only PostgreSQL repeatable-read transaction",
    }];
    let identity = serde_json::json!({
        "interop_schema": INTEROP_SCHEMA, "kind": "snapshot",
        "producer": { "app": PRODUCER_APP, "app_version": env!("CARGO_PKG_VERSION") },
        "created_at": created_at, "records": &records, "links": &links,
        "provenance": &provenance, "diagnostics": &diagnostics
    });
    let digest =
        hex_digest(&serde_json::to_vec(&identity).expect("interop identity is serializable"));
    Ok(Snapshot {
        interop_schema: INTEROP_SCHEMA,
        kind: "snapshot",
        producer: Producer {
            app: PRODUCER_APP,
            app_version: env!("CARGO_PKG_VERSION"),
        },
        export_id: format!("{PRODUCER_APP}:snapshot:{digest}"),
        created_at,
        source_revision: digest,
        records,
        links,
        provenance,
        diagnostics,
    })
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
fn global_id(kind: &str, local_id: &str) -> String {
    format!("{PRODUCER_APP}:{kind}:{local_id}")
}
fn record<T: Serialize>(
    kind: &'static str,
    local_id: String,
    revision: i64,
    observed_at: DateTime<Utc>,
    lifecycle: Lifecycle,
    payload: &T,
) -> Record {
    Record {
        global_id: global_id(kind, &local_id),
        origin: Origin {
            app: PRODUCER_APP,
            kind,
            local_id,
        },
        revision,
        observed_at,
        lifecycle,
        payload: serde_json::to_value(payload).expect("domain export payloads are serializable"),
    }
}
fn link(source: &str, target: &str, relation: &'static str) -> Link {
    Link { link_id: format!("{source}--{relation}--{target}"), source_global_id: source.to_owned(), target_global_id: target.to_owned(), relation, created_by: PRODUCER_APP, created_at: None, provenance: "relationship derived from authoritative foreign-key/join state; creation time unavailable".to_owned() }
}
