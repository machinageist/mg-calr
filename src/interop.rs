use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::config::ConnectionSettings;
use crate::storage::{StorageError, export_snapshot_sources};

const INTEROP_SCHEMA: &str = "mg.interop/1";
const PRODUCER_APP: &str = "mg-calr";
const TODO_PRODUCER_APP: &str = "mg-todo";
const MAX_PROJECTION_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PROJECTION_RECORDS: usize = 100_000;
const MAX_PROJECTION_LINKS: usize = 500_000;

/// A validated, immutable mg-todo snapshot held by mg-calr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoProjectionSnapshot {
    pub snapshot: Snapshot,
}

/// Errors returned before any projection file is replaced.
#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("could not read projection snapshot: {0}")]
    Read(#[source] std::io::Error),
    #[error("could not write projection snapshot: {0}")]
    Write(#[source] std::io::Error),
    #[error("projection snapshot JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid mg-todo projection: {0}")]
    Invalid(String),
}

impl TodoProjectionSnapshot {
    /// Parse and validate an mg-todo envelope without database access.
    ///
    /// # Errors
    /// Returns an error for invalid JSON or a contract violation.
    pub fn parse(json: &str) -> Result<Self, ProjectionError> {
        if u64::try_from(json.len()).unwrap_or(u64::MAX) > MAX_PROJECTION_BYTES {
            return Err(ProjectionError::Invalid(format!(
                "projection exceeds the {MAX_PROJECTION_BYTES}-byte limit"
            )));
        }
        let snapshot: Snapshot = serde_json::from_str(json)?;
        Self::validate(snapshot)
    }

    /// Validate the read-only producer and relationship boundary.
    ///
    /// # Errors
    /// Returns an error when the producer, records, revisions, or links are invalid.
    #[allow(clippy::too_many_lines)]
    pub fn validate(snapshot: Snapshot) -> Result<Self, ProjectionError> {
        if snapshot.interop_schema != INTEROP_SCHEMA {
            return Err(ProjectionError::Invalid(format!(
                "unsupported schema '{}'",
                snapshot.interop_schema
            )));
        }
        if snapshot.kind != "snapshot" || snapshot.producer.app != TODO_PRODUCER_APP {
            return Err(ProjectionError::Invalid(
                "expected an mg-todo snapshot producer".to_owned(),
            ));
        }
        if snapshot.records.len() > MAX_PROJECTION_RECORDS
            || snapshot.links.len() > MAX_PROJECTION_LINKS
        {
            return Err(ProjectionError::Invalid(
                "projection record or relationship limit exceeded".to_owned(),
            ));
        }
        let mut ids = HashSet::with_capacity(snapshot.records.len());
        for record in &snapshot.records {
            if !matches!(
                record.origin.kind.as_str(),
                "project" | "tag" | "todo" | "reminder" | "delivery"
            ) {
                return Err(ProjectionError::Invalid(format!(
                    "record kind '{}' is outside the todo projection",
                    record.origin.kind
                )));
            }
            if record.origin.app != TODO_PRODUCER_APP || record.origin.local_id.is_empty() {
                return Err(ProjectionError::Invalid(
                    "record origin must identify mg-todo and a local ID".to_owned(),
                ));
            }
            let expected_global_id = format!(
                "{TODO_PRODUCER_APP}:{}:{}",
                record.origin.kind, record.origin.local_id
            );
            if record.global_id != expected_global_id {
                return Err(ProjectionError::Invalid(format!(
                    "global_id '{}' does not match canonical origin '{}', '{}', '{}'",
                    record.global_id, record.origin.app, record.origin.kind, record.origin.local_id
                )));
            }
            if record.revision < 1 || !ids.insert(record.global_id.clone()) {
                return Err(ProjectionError::Invalid(
                    "record IDs must be unique and revisions positive".to_owned(),
                ));
            }
            if record.observed_at > snapshot.created_at {
                return Err(ProjectionError::Invalid(
                    "record observed_at is newer than snapshot created_at".to_owned(),
                ));
            }
            validate_lifecycle(&record.lifecycle, record.observed_at)?;
        }
        let mut link_ids = HashSet::with_capacity(snapshot.links.len());
        let mut graph_edges: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for link in &snapshot.links {
            if !ids.contains(&link.source_global_id) || !ids.contains(&link.target_global_id) {
                return Err(ProjectionError::Invalid(
                    "relationship endpoint is not present in the snapshot".to_owned(),
                ));
            }
            let expected = format!(
                "{}--{}--{}",
                link.source_global_id, link.relation, link.target_global_id
            );
            if link.link_id != expected || !link_ids.insert(link.link_id.clone()) {
                return Err(ProjectionError::Invalid(
                    "relationship IDs must be deterministic and unique".to_owned(),
                ));
            }
            if link.created_by != TODO_PRODUCER_APP {
                return Err(ProjectionError::Invalid(
                    "relationship created_by must be mg-todo".to_owned(),
                ));
            }
            if !relationship_allowed(
                &link.relation,
                record_kind(&link.source_global_id),
                record_kind(&link.target_global_id),
            ) {
                return Err(ProjectionError::Invalid(format!(
                    "relationship '{}' has invalid endpoint kinds or direction",
                    link.relation
                )));
            }
            if matches!(link.relation.as_str(), "todo_parent" | "todo_depends_on") {
                if link.source_global_id == link.target_global_id {
                    return Err(ProjectionError::Invalid(format!(
                        "{} relationship cannot link a todo to itself",
                        link.relation
                    )));
                }
                graph_edges
                    .entry(link.relation.as_str())
                    .or_default()
                    .push((
                        link.source_global_id.as_str(),
                        link.target_global_id.as_str(),
                    ));
            }
        }
        for relation in ["todo_parent", "todo_depends_on"] {
            if has_cycle(graph_edges.get(relation).into_iter().flatten().copied()) {
                return Err(ProjectionError::Invalid(format!(
                    "{relation} relationships contain a cycle"
                )));
            }
        }
        Ok(Self { snapshot })
    }

    /// Return a stable digest of the complete imported envelope.
    ///
    /// # Panics
    /// Panics only if a validated snapshot cannot be serialized, which would indicate a
    /// programming error in the contract types.
    #[must_use]
    pub fn revision(&self) -> String {
        hex_digest(&serde_json::to_vec(&self.snapshot).expect("snapshot is serializable"))
    }

    /// Atomically replace the local projection file, never contacting mg-todo.
    ///
    /// # Errors
    /// Returns an error when the projection directory or replacement file cannot be written.
    ///
    /// # Panics
    /// Panics only if a validated snapshot cannot be serialized, which would indicate a
    /// programming error in the contract types.
    pub fn store(&self, path: &Path) -> Result<(), ProjectionError> {
        let bytes = serde_json::to_vec_pretty(&self.snapshot).expect("snapshot is serializable");
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent).map_err(ProjectionError::Write)?;
        reject_symlink(parent).map_err(ProjectionError::Write)?;
        if path_entry_exists(path).map_err(ProjectionError::Write)? {
            reject_symlink(path).map_err(ProjectionError::Write)?;
        }
        let _lock = ProjectionLock::acquire(path).map_err(ProjectionError::Write)?;
        if path_entry_exists(path).map_err(ProjectionError::Write)? {
            let existing = Self::load(path)?;
            if self.snapshot.created_at < existing.snapshot.created_at {
                return Err(ProjectionError::Invalid(
                    "stale projection snapshot would roll back created_at".to_owned(),
                ));
            }
            if self.snapshot.created_at == existing.snapshot.created_at
                && self.revision() != existing.revision()
            {
                return Err(ProjectionError::Invalid(
                    "conflicting projection revisions at the same created_at".to_owned(),
                ));
            }
            let old_records: HashMap<_, _> = existing
                .snapshot
                .records
                .iter()
                .map(|record| (record.global_id.as_str(), record))
                .collect();
            for record in &self.snapshot.records {
                if let Some(old) = old_records.get(record.global_id.as_str()) {
                    if record.revision < old.revision {
                        return Err(ProjectionError::Invalid(format!(
                            "stale record revision for {}",
                            record.global_id
                        )));
                    }
                    if record.revision == old.revision
                        && serde_json::to_vec(record).expect("record is serializable")
                            != serde_json::to_vec(old).expect("record is serializable")
                    {
                        return Err(ProjectionError::Invalid(format!(
                            "conflicting record revision for {}",
                            record.global_id
                        )));
                    }
                }
            }
        }
        let temporary = unique_temp_path(path);
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut file = options.open(&temporary).map_err(ProjectionError::Write)?;
            file.write_all(&bytes).map_err(ProjectionError::Write)?;
            file.sync_all().map_err(ProjectionError::Write)?;
            std::fs::rename(&temporary, path).map_err(ProjectionError::Write)?;
            sync_parent_directory(parent).map_err(ProjectionError::Write)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    /// Read the last explicitly imported local projection.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read, parsed, or validated.
    pub fn load(path: &Path) -> Result<Self, ProjectionError> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        options.custom_flags(libc::O_NOFOLLOW);
        let file = options.open(path).map_err(ProjectionError::Read)?;
        let metadata = file.metadata().map_err(ProjectionError::Read)?;
        if metadata.len() > MAX_PROJECTION_BYTES {
            return Err(ProjectionError::Invalid(format!(
                "projection exceeds the {MAX_PROJECTION_BYTES}-byte limit"
            )));
        }
        let mut bytes = Vec::with_capacity(
            usize::try_from(metadata.len().min(MAX_PROJECTION_BYTES)).unwrap_or(0),
        );
        file.take(MAX_PROJECTION_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(ProjectionError::Read)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PROJECTION_BYTES {
            return Err(ProjectionError::Invalid(format!(
                "projection exceeds the {MAX_PROJECTION_BYTES}-byte limit"
            )));
        }
        let json = String::from_utf8(bytes).map_err(|error| {
            ProjectionError::Read(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        Self::parse(&json)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub interop_schema: String,
    pub kind: String,
    pub producer: Producer,
    pub export_id: String,
    pub created_at: DateTime<Utc>,
    pub source_revision: String,
    pub records: Vec<Record>,
    pub links: Vec<Link>,
    pub provenance: Vec<Provenance>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    pub app: String,
    pub app_version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub global_id: String,
    pub origin: Origin,
    pub revision: i64,
    pub observed_at: DateTime<Utc>,
    pub lifecycle: Lifecycle,
    pub payload: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub app: String,
    pub kind: String,
    pub local_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lifecycle {
    pub state: String,
    pub deleted_at: Option<DateTime<Utc>>,
    pub tombstoned_at: Option<DateTime<Utc>>,
    pub trashed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub purged: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub link_id: String,
    pub source_global_id: String,
    pub target_global_id: String,
    pub relation: String,
    pub created_by: String,
    /// Relationship tables do not store link creation time; never infer it.
    pub created_at: Option<DateTime<Utc>>,
    pub provenance: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source: String,
    pub boundary: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub severity: String,
    pub code: String,
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
                state: state.to_owned(),
                deleted_at: calendar.deleted_at,
                tombstoned_at: None,
                trashed_at: None,
                archived_at: None,
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
                state: state.to_owned(),
                deleted_at: event.deleted_at,
                tombstoned_at: event.remote_tombstoned_at,
                trashed_at: None,
                archived_at: None,
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
                state: state.to_owned(),
                deleted_at: None,
                tombstoned_at: None,
                trashed_at: None,
                archived_at: project.archived_at,
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
                state: "active".to_owned(),
                deleted_at: None,
                tombstoned_at: None,
                trashed_at: None,
                archived_at: None,
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
                state: state.to_owned(),
                deleted_at,
                tombstoned_at: None,
                trashed_at: todo.trashed_at,
                archived_at: None,
                purged: false,
            },
            &todo,
        ));
    }
    // Purged rows are absent by definition; explicitly document that absence
    // instead of manufacturing tombstones or pretending history is complete.
    diagnostics.push(Diagnostic { severity: "info".to_owned(), code: "purged_absence".to_owned(), message: "Purged rows are not present in authoritative exports; absence is not interpreted as deletion.".to_owned() });
    records.sort_by(|a, b| a.global_id.cmp(&b.global_id));
    links.sort_by(|a, b| a.link_id.cmp(&b.link_id));
    let created_at = records
        .iter()
        .map(|r| r.observed_at)
        .max()
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
    let provenance = vec![Provenance {
        source: PRODUCER_APP.to_owned(),
        boundary: "authoritative read-only PostgreSQL repeatable-read transaction".to_owned(),
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
        interop_schema: INTEROP_SCHEMA.to_owned(),
        kind: "snapshot".to_owned(),
        producer: Producer {
            app: PRODUCER_APP.to_owned(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
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

fn validate_lifecycle(
    lifecycle: &Lifecycle,
    observed_at: DateTime<Utc>,
) -> Result<(), ProjectionError> {
    if lifecycle.purged {
        return Err(ProjectionError::Invalid(
            "purged records are not valid in a present-row snapshot".to_owned(),
        ));
    }
    let timestamps = [
        lifecycle.deleted_at,
        lifecycle.tombstoned_at,
        lifecycle.trashed_at,
        lifecycle.archived_at,
    ];
    if timestamps
        .iter()
        .flatten()
        .any(|timestamp| *timestamp > observed_at)
    {
        return Err(ProjectionError::Invalid(
            "lifecycle timestamp is newer than observed_at".to_owned(),
        ));
    }
    let expected = match lifecycle.state.as_str() {
        "active" => timestamps.iter().all(Option::is_none) && !lifecycle.purged,
        "trashed" => {
            lifecycle.trashed_at.is_some()
                && lifecycle.deleted_at.is_none()
                && lifecycle.tombstoned_at.is_none()
                && lifecycle.archived_at.is_none()
                && !lifecycle.purged
        }
        "deleted" => {
            lifecycle.deleted_at.is_some()
                && lifecycle.tombstoned_at.is_none()
                && lifecycle.trashed_at.is_none()
                && lifecycle.archived_at.is_none()
        }
        "tombstoned" => {
            lifecycle.tombstoned_at.is_some()
                && lifecycle.deleted_at.is_none()
                && lifecycle.trashed_at.is_none()
                && lifecycle.archived_at.is_none()
                && !lifecycle.purged
        }
        "archived" => {
            lifecycle.archived_at.is_some()
                && lifecycle.deleted_at.is_none()
                && lifecycle.tombstoned_at.is_none()
                && lifecycle.trashed_at.is_none()
                && !lifecycle.purged
        }
        _ => false,
    };
    if !expected {
        return Err(ProjectionError::Invalid(format!(
            "invalid lifecycle state '{}' and timestamps",
            lifecycle.state
        )));
    }
    Ok(())
}

fn record_kind(global_id: &str) -> Option<&str> {
    let mut parts = global_id.splitn(3, ':');
    (parts.next() == Some(TODO_PRODUCER_APP))
        .then(|| parts.next())
        .flatten()
}

fn relationship_allowed(relation: &str, source: Option<&str>, target: Option<&str>) -> bool {
    matches!(
        (relation, source, target),
        ("project_contains_todo", Some("project"), Some("todo"))
            | ("todo_parent", Some("todo"), Some("todo"))
            | ("todo_depends_on", Some("todo"), Some("todo"))
            | ("todo_tagged", Some("todo"), Some("tag"))
            | ("todo_has_reminder", Some("todo"), Some("reminder"))
            | ("todo_has_delivery", Some("todo"), Some("delivery"))
    )
}

fn has_cycle<'a>(edges: impl Iterator<Item = (&'a str, &'a str)>) -> bool {
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    for (source, target) in edges {
        adjacency.entry(source).or_default().push(target);
        incoming.entry(source).or_insert(0);
        *incoming.entry(target).or_insert(0) += 1;
    }
    let node_count = incoming.len();
    let mut ready: Vec<_> = incoming
        .iter()
        .filter_map(|(node, count)| (*count == 0).then_some(*node))
        .collect();
    let mut visited = 0;
    while let Some(node) = ready.pop() {
        visited += 1;
        for target in adjacency.get(node).into_iter().flatten() {
            let count = incoming
                .get_mut(target)
                .expect("all relationship targets have an incoming count");
            *count -= 1;
            if *count == 0 {
                ready.push(target);
            }
        }
    }
    visited != node_count
}

struct ProjectionLock {
    // The kernel releases this advisory lock when the process or descriptor exits.
    // The lock file remains in place so contenders always lock the same inode.
    file: std::fs::File,
}

impl ProjectionLock {
    fn acquire(path: &Path) -> Result<Self, std::io::Error> {
        let lock_path = path.with_file_name(format!(
            ".{}.lock",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("projection")
        ));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let file = options.open(&lock_path)?;
        loop {
            match FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(Self { file }),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "timed out waiting for projection lock",
                        ));
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for ProjectionLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn reject_symlink(path: &Path) -> Result<(), std::io::Error> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "projection paths must not be symbolic links",
        ));
    }
    Ok(())
}

fn path_entry_exists(path: &Path) -> Result<bool, std::io::Error> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

// Persist the directory entry after rename where the platform supports directory fsync.
#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<(), std::io::Error> {
    OpenOptions::new().read(true).open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<(), std::io::Error> {
    // Windows does not expose a portable directory fsync through std; the file is durable.
    Ok(())
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("projection.json");
    path.with_file_name(format!(".{filename}.tmp-{}-{nonce}", std::process::id()))
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
            app: PRODUCER_APP.to_owned(),
            kind: kind.to_owned(),
            local_id,
        },
        revision,
        observed_at,
        lifecycle,
        payload: serde_json::to_value(payload).expect("domain export payloads are serializable"),
    }
}
fn link(source: &str, target: &str, relation: &'static str) -> Link {
    Link { link_id: format!("{source}--{relation}--{target}"), source_global_id: source.to_owned(), target_global_id: target.to_owned(), relation: relation.to_owned(), created_by: PRODUCER_APP.to_owned(), created_at: None, provenance: "relationship derived from authoritative foreign-key/join state; creation time unavailable".to_owned() }
}

#[cfg(test)]
mod lock_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn reuses_lock_file_after_owner_exits() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("projection.json");
        let lock_path = path.with_file_name(".projection.json.lock");
        let first = ProjectionLock::acquire(&path).unwrap();
        assert!(lock_path.exists());
        drop(first);
        let second = ProjectionLock::acquire(&path).unwrap();
        assert!(lock_path.exists());
        drop(second);
    }

    #[test]
    fn never_reclaims_lock_owned_by_live_process() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("projection.json");
        let lock = ProjectionLock::acquire(&path).unwrap();
        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.with_file_name(".projection.json.lock"))
            .unwrap();
        assert_eq!(
            FileExt::try_lock_exclusive(&contender).unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        drop(lock);
    }
}
