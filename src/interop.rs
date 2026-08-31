use chrono::{DateTime, TimeDelta, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;

use crate::application::{
    AgendaTodoSnapshot, ProjectionCompleteness, ProjectionDiagnostic, TodoProjectionMetadata,
};
use crate::config::ConnectionSettings;
use crate::domain::todo::{ProjectId, TagId, Todo, TodoId};
use crate::storage::{StorageError, export_snapshot_sources};

const INTEROP_SCHEMA: &str = "mg.interop/1";
const PRODUCER_APP: &str = "mg-calr";
const TODO_PRODUCER_APP: &str = "mg-todo";
const MAX_PROJECTION_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PROJECTION_RECORDS: usize = 100_000;
const MAX_PROJECTION_LINKS: usize = 500_000;
const MAX_AGENDA_PROJECTION_AGE: TimeDelta = TimeDelta::hours(24);
const MAX_IMPORT_CLOCK_SKEW: TimeDelta = TimeDelta::minutes(5);

/// A validated, immutable mg-todo snapshot held by mg-calr.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoProjectionSnapshot {
    snapshot: Snapshot,
}

/// Errors returned before any projection file is replaced.
#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("the imported mg-todo projection is missing")]
    Missing,
    #[error("stale mg-todo projection: {0}")]
    Stale(String),
    #[error("conflicting mg-todo projection: {0}")]
    Conflict(String),
    #[error("incomplete mg-todo projection: {0}")]
    Incomplete(String),
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
    /// Rehydrate agenda todo rows from the validated projection payload and links.
    ///
    /// # Errors
    /// Returns an explicit stale or conflict diagnostic when duplicated projection
    /// metadata disagrees, or an invalid diagnostic when a todo payload cannot be used.
    #[allow(clippy::too_many_lines)]
    pub fn agenda_todos(&self) -> Result<AgendaTodoSnapshot, ProjectionError> {
        self.agenda_todos_at(Utc::now())
    }

    /// Rehydrate agenda rows using an injected clock for deterministic policy tests.
    ///
    /// # Errors
    /// Returns a typed projection error if freshness, completeness, payload, or
    /// relationship evidence is invalid.
    #[allow(clippy::too_many_lines)]
    pub fn agenda_todos_at(
        &self,
        now: DateTime<Utc>,
    ) -> Result<AgendaTodoSnapshot, ProjectionError> {
        // Defense in depth if this type is ever made internally mutable.
        Self::validate(self.snapshot.clone())?;
        if self.snapshot.created_at > now {
            return Err(ProjectionError::Invalid(
                "projection created_at is in the future".to_owned(),
            ));
        }
        if now - self.snapshot.created_at > MAX_AGENDA_PROJECTION_AGE {
            return Err(ProjectionError::Stale(
                "projection is older than the 24-hour agenda freshness limit".to_owned(),
            ));
        }
        let degraded = self
            .snapshot
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity != "info");
        if let Some(diagnostic) = degraded {
            return Err(ProjectionError::Incomplete(format!(
                "producer diagnostic {} reports {} coverage",
                diagnostic.code, diagnostic.severity
            )));
        }
        let mut todos = Vec::new();
        let mut agenda_visible = Vec::new();
        let mut todo_indexes = HashMap::new();
        for record in self
            .snapshot
            .records
            .iter()
            .filter(|record| record.origin.kind == "todo")
        {
            let todo: Todo = serde_json::from_value(record.payload.clone()).map_err(|error| {
                ProjectionError::Invalid(format!(
                    "todo payload for {} is invalid: {error}",
                    record.global_id
                ))
            })?;
            if record.origin.local_id != todo.id.to_string() {
                return Err(ProjectionError::Conflict(format!(
                    "todo payload identity disagrees with {}",
                    record.global_id
                )));
            }
            if record.revision != todo.version {
                return Err(ProjectionError::Stale(format!(
                    "record and payload revision disagree for {}",
                    record.global_id
                )));
            }
            if record.observed_at != todo.updated_at {
                return Err(ProjectionError::Stale(format!(
                    "record observation and payload update disagree for {}",
                    record.global_id
                )));
            }
            if record.lifecycle.trashed_at != todo.trashed_at {
                return Err(ProjectionError::Conflict(format!(
                    "record lifecycle and payload disagree for {}",
                    record.global_id
                )));
            }
            let visible = match record.lifecycle.state.as_str() {
                "active" | "trashed" => true,
                "deleted" | "tombstoned" => false,
                state => {
                    return Err(ProjectionError::Conflict(format!(
                        "todo {} has unsupported agenda lifecycle {state}",
                        record.global_id
                    )));
                }
            };
            let todo = todo.rehydrate().map_err(|error| {
                ProjectionError::Invalid(format!(
                    "todo payload for {} failed domain validation: {error}",
                    record.global_id
                ))
            })?;
            todo_indexes.insert(record.global_id.as_str(), todos.len());
            todos.push(todo);
            agenda_visible.push(visible);
        }

        let mut projected_parents: HashMap<usize, TodoId> = HashMap::new();
        let mut projected_projects: HashMap<usize, ProjectId> = HashMap::new();
        let mut projected_dependencies: HashMap<usize, Vec<TodoId>> = HashMap::new();
        let mut projected_tags: HashMap<usize, Vec<TagId>> = HashMap::new();
        for link in &self.snapshot.links {
            match link.relation.as_str() {
                "todo_parent" => {
                    let child = todo_index(&todo_indexes, &link.target_global_id)?;
                    let parent = todo_id_from_global(&link.source_global_id)?;
                    if projected_parents.insert(child, parent).is_some() {
                        return Err(ProjectionError::Conflict(format!(
                            "multiple parent relationships target {}",
                            link.target_global_id
                        )));
                    }
                }
                "todo_depends_on" => {
                    let dependent = todo_index(&todo_indexes, &link.source_global_id)?;
                    projected_dependencies
                        .entry(dependent)
                        .or_default()
                        .push(todo_id_from_global(&link.target_global_id)?);
                }
                "project_contains_todo" => {
                    let todo = todo_index(&todo_indexes, &link.target_global_id)?;
                    let project = link
                        .source_global_id
                        .strip_prefix("mg-todo:project:")
                        .ok_or_else(|| relationship_conflict(link))?
                        .parse::<ProjectId>()
                        .map_err(|_| relationship_conflict(link))?;
                    if projected_projects.insert(todo, project).is_some() {
                        return Err(ProjectionError::Conflict(format!(
                            "multiple project relationships target {}",
                            link.target_global_id
                        )));
                    }
                }
                "todo_tagged" => {
                    let todo = todo_index(&todo_indexes, &link.source_global_id)?;
                    let tag = link
                        .target_global_id
                        .strip_prefix("mg-todo:tag:")
                        .ok_or_else(|| relationship_conflict(link))?
                        .parse::<TagId>()
                        .map_err(|_| relationship_conflict(link))?;
                    projected_tags.entry(todo).or_default().push(tag);
                }
                _ => {}
            }
        }

        for (index, todo) in todos.iter_mut().enumerate() {
            let parent = projected_parents.remove(&index);
            let project = projected_projects.remove(&index);
            let mut dependencies = projected_dependencies.remove(&index).unwrap_or_default();
            let mut tags = projected_tags.remove(&index).unwrap_or_default();
            dependencies.sort_by_key(ToString::to_string);
            tags.sort_by_key(ToString::to_string);
            let mut payload_dependencies = todo.dependency_ids.clone();
            let mut payload_tags = todo.tag_ids.clone();
            payload_dependencies.sort_by_key(ToString::to_string);
            payload_tags.sort_by_key(ToString::to_string);
            if todo.parent_id != parent
                || todo.project_id != project
                || payload_dependencies != dependencies
                || payload_tags != tags
            {
                return Err(ProjectionError::Conflict(format!(
                    "payload and relationship projection disagree for mg-todo:todo:{}",
                    todo.id
                )));
            }
            todo.dependency_ids = dependencies;
            todo.tag_ids = tags;
        }
        let todos = todos
            .into_iter()
            .zip(agenda_visible)
            .filter_map(|(todo, visible)| visible.then_some(todo))
            .collect();
        Ok(AgendaTodoSnapshot {
            todos,
            metadata: self.metadata(),
        })
    }

    /// Borrow the immutable validated source envelope.
    #[must_use]
    pub const fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    fn metadata(&self) -> TodoProjectionMetadata {
        TodoProjectionMetadata {
            producer: self.snapshot.producer.app.clone(),
            producer_version: self.snapshot.producer.app_version.clone(),
            producer_revision: self.snapshot.producer_revision,
            source_revision: self.snapshot.source_revision.clone(),
            content_revision: self.revision(),
            created_at: self.snapshot.created_at,
            completeness: ProjectionCompleteness {
                complete: self.snapshot.completeness.complete,
                record_count: self.snapshot.records.len(),
                todo_count: self
                    .snapshot
                    .records
                    .iter()
                    .filter(|record| record.origin.kind == "todo")
                    .count(),
                link_count: self.snapshot.links.len(),
                diagnostics: self
                    .snapshot
                    .diagnostics
                    .iter()
                    .map(|diagnostic| ProjectionDiagnostic {
                        severity: diagnostic.severity.clone(),
                        code: diagnostic.code.clone(),
                        message: diagnostic.message.clone(),
                    })
                    .collect(),
            },
        }
    }

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
        if snapshot.producer_revision == 0 {
            return Err(ProjectionError::Invalid(
                "producer_revision must be positive".to_owned(),
            ));
        }
        if !snapshot.completeness.complete
            || snapshot.completeness.expected_records != snapshot.records.len()
            || snapshot.completeness.expected_links != snapshot.links.len()
        {
            return Err(ProjectionError::Incomplete(
                "complete-snapshot marker and expected counts must match the envelope".to_owned(),
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
        self.store_at(path, Utc::now())
    }

    /// Store with an injected clock for deterministic import-policy tests.
    ///
    /// # Errors
    /// Returns an error before replacement for future, stale, conflicting,
    /// incomplete, disappearing, or unsafe filesystem state.
    #[allow(clippy::too_many_lines)]
    pub fn store_at(&self, path: &Path, now: DateTime<Utc>) -> Result<(), ProjectionError> {
        if self.snapshot.created_at > now + MAX_IMPORT_CLOCK_SKEW {
            return Err(ProjectionError::Invalid(
                "projection created_at exceeds the five-minute import clock-skew limit".to_owned(),
            ));
        }
        let bytes = serde_json::to_vec_pretty(&self.snapshot).map_err(|error| {
            ProjectionError::Invalid(format!("projection serialization failed: {error}"))
        })?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let filename = path.file_name().ok_or_else(|| {
            ProjectionError::Write(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "projection path must name a file",
            ))
        })?;
        let directory =
            SecureProjectionDirectory::open(parent, true).map_err(ProjectionError::Write)?;
        let _lock =
            ProjectionLock::acquire_at(&directory, filename).map_err(ProjectionError::Write)?;
        if directory
            .entry_exists(filename)
            .map_err(ProjectionError::Write)?
        {
            let existing = Self::load_file(
                directory
                    .open_read(filename)
                    .map_err(ProjectionError::Read)?,
            )?;
            if self.snapshot.producer_revision < existing.snapshot.producer_revision {
                return Err(ProjectionError::Stale(
                    "stale projection snapshot would roll back producer_revision".to_owned(),
                ));
            }
            if self.snapshot.producer_revision == existing.snapshot.producer_revision
                && self.revision() != existing.revision()
            {
                return Err(ProjectionError::Conflict(
                    "conflicting projection revisions at the same created_at".to_owned(),
                ));
            }
            let old_records: HashMap<_, _> = existing
                .snapshot
                .records
                .iter()
                .map(|record| (record.global_id.as_str(), record))
                .collect();
            let new_ids: HashSet<_> = self
                .snapshot
                .records
                .iter()
                .map(|record| record.global_id.as_str())
                .collect();
            if let Some(disappeared) = existing
                .snapshot
                .records
                .iter()
                .find(|record| !new_ids.contains(record.global_id.as_str()))
            {
                return Err(ProjectionError::Incomplete(format!(
                    "record {} disappeared without a retained tombstone",
                    disappeared.global_id
                )));
            }
            for record in &self.snapshot.records {
                if let Some(old) = old_records.get(record.global_id.as_str()) {
                    if record.revision < old.revision {
                        return Err(ProjectionError::Stale(format!(
                            "stale record revision for {}",
                            record.global_id
                        )));
                    }
                    if record.revision == old.revision {
                        let record_bytes = serde_json::to_vec(record).map_err(|error| {
                            ProjectionError::Invalid(format!(
                                "projection record serialization failed: {error}"
                            ))
                        })?;
                        let old_bytes = serde_json::to_vec(old).map_err(|error| {
                            ProjectionError::Invalid(format!(
                                "stored projection record serialization failed: {error}"
                            ))
                        })?;
                        if record_bytes != old_bytes {
                            return Err(ProjectionError::Conflict(format!(
                                "conflicting record revision for {}",
                                record.global_id
                            )));
                        }
                    }
                }
            }
        }
        let temporary = unique_temp_name(filename);
        let result = (|| {
            let mut file = directory
                .create_new(&temporary)
                .map_err(ProjectionError::Write)?;
            file.write_all(&bytes).map_err(ProjectionError::Write)?;
            file.sync_all().map_err(ProjectionError::Write)?;
            directory
                .rename(&temporary, filename)
                .map_err(ProjectionError::Write)?;
            directory.sync().map_err(ProjectionError::Write)
        })();
        if result.is_err() {
            let _ = directory.remove(&temporary);
        }
        result
    }

    /// Read the last explicitly imported local projection.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read, parsed, or validated.
    pub fn load(path: &Path) -> Result<Self, ProjectionError> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let filename = path.file_name().ok_or_else(|| {
            ProjectionError::Read(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "projection path must name a file",
            ))
        })?;
        let directory =
            SecureProjectionDirectory::open(parent, false).map_err(ProjectionError::Read)?;
        let file = directory.open_read(filename).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProjectionError::Missing
            } else {
                ProjectionError::Read(error)
            }
        })?;
        Self::load_file(file)
    }

    fn load_file(file: std::fs::File) -> Result<Self, ProjectionError> {
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

fn todo_index(indexes: &HashMap<&str, usize>, global_id: &str) -> Result<usize, ProjectionError> {
    indexes.get(global_id).copied().ok_or_else(|| {
        ProjectionError::Conflict(format!(
            "relationship references unavailable agenda todo {global_id}"
        ))
    })
}

fn todo_id_from_global(global_id: &str) -> Result<TodoId, ProjectionError> {
    global_id
        .strip_prefix("mg-todo:todo:")
        .ok_or_else(|| {
            ProjectionError::Conflict(format!(
                "relationship endpoint is not an mg-todo todo: {global_id}"
            ))
        })?
        .parse::<TodoId>()
        .map_err(|_| {
            ProjectionError::Conflict(format!(
                "relationship endpoint has an invalid todo identity: {global_id}"
            ))
        })
}

fn relationship_conflict(link: &Link) -> ProjectionError {
    ProjectionError::Conflict(format!(
        "relationship {} has an invalid agenda identity",
        link.link_id
    ))
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
    /// Producer-defined, monotonically increasing complete-snapshot sequence.
    pub producer_revision: u64,
    /// Evidence that the producer emitted the complete authority set.
    pub completeness: SnapshotCompleteness,
    pub records: Vec<Record>,
    pub links: Vec<Link>,
    pub provenance: Vec<Provenance>,
    pub diagnostics: Vec<Diagnostic>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCompleteness {
    pub complete: bool,
    pub expected_records: usize,
    pub expected_links: usize,
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
        producer_revision: 1,
        completeness: SnapshotCompleteness {
            complete: true,
            expected_records: records.len(),
            expected_links: links.len(),
        },
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

struct SecureProjectionDirectory {
    file: std::fs::File,
}

#[cfg(unix)]
impl SecureProjectionDirectory {
    fn open(path: &Path, create: bool) -> Result<Self, std::io::Error> {
        use rustix::fs::{Mode, OFlags, mkdirat, openat};
        use std::path::Component;

        let anchor = if path.is_absolute() {
            Path::new("/")
        } else {
            Path::new(".")
        };
        let mut directory = OpenOptions::new().read(true).open(anchor)?;
        for component in path.components() {
            let Component::Normal(name) = component else {
                if matches!(component, Component::RootDir | Component::CurDir) {
                    continue;
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "projection path may not contain parent traversal",
                ));
            };
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let opened = openat(&directory, name, flags, Mode::empty()).or_else(|error| {
                if create && error == rustix::io::Errno::NOENT {
                    match mkdirat(&directory, name, Mode::RWXU) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(error) => return Err(error),
                    }
                    openat(&directory, name, flags, Mode::empty())
                } else {
                    Err(error)
                }
            });
            directory = std::fs::File::from(opened.map_err(io_error)?);
        }
        Ok(Self { file: directory })
    }

    fn open_with(
        &self,
        name: &std::ffi::OsStr,
        flags: rustix::fs::OFlags,
        mode: rustix::fs::Mode,
    ) -> Result<std::fs::File, std::io::Error> {
        rustix::fs::openat(
            &self.file,
            name,
            flags | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            mode,
        )
        .map(std::fs::File::from)
        .map_err(io_error)
    }

    fn open_read(&self, name: &std::ffi::OsStr) -> Result<std::fs::File, std::io::Error> {
        let file = self.open_with(name, rustix::fs::OFlags::RDONLY, rustix::fs::Mode::empty())?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "projection target must be a regular file",
            ));
        }
        Ok(file)
    }

    fn create_new(&self, name: &std::ffi::OsStr) -> Result<std::fs::File, std::io::Error> {
        self.open_with(
            name,
            rustix::fs::OFlags::WRONLY | rustix::fs::OFlags::CREATE | rustix::fs::OFlags::EXCL,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
    }

    fn entry_exists(&self, name: &std::ffi::OsStr) -> Result<bool, std::io::Error> {
        match rustix::fs::statat(&self.file, name, rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) if rustix::fs::FileType::from_raw_mode(stat.st_mode).is_file() => Ok(true),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "projection target must be a regular file and not a symbolic link",
            )),
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(error) => Err(io_error(error)),
        }
    }

    fn rename(&self, old: &std::ffi::OsStr, new: &std::ffi::OsStr) -> Result<(), std::io::Error> {
        rustix::fs::renameat(&self.file, old, &self.file, new).map_err(io_error)
    }

    fn remove(&self, name: &std::ffi::OsStr) -> Result<(), std::io::Error> {
        rustix::fs::unlinkat(&self.file, name, rustix::fs::AtFlags::empty()).map_err(io_error)
    }

    fn sync(&self) -> Result<(), std::io::Error> {
        self.file.sync_all()
    }
}

#[cfg(unix)]
fn io_error(error: rustix::io::Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(error.raw_os_error())
}

struct ProjectionLock {
    // The kernel releases this advisory lock when the process or descriptor exits.
    // The lock file remains in place so contenders always lock the same inode.
    file: std::fs::File,
}

impl ProjectionLock {
    fn acquire_at(
        directory: &SecureProjectionDirectory,
        filename: &std::ffi::OsStr,
    ) -> Result<Self, std::io::Error> {
        let lock_name = std::ffi::OsString::from(format!(".{}.lock", filename.to_string_lossy()));
        let deadline = Instant::now() + Duration::from_secs(5);
        let file = directory.open_with(
            &lock_name,
            rustix::fs::OFlags::RDWR | rustix::fs::OFlags::CREATE,
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )?;
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

fn unique_temp_name(filename: &std::ffi::OsStr) -> std::ffi::OsString {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::ffi::OsString::from(format!(
        ".{}.tmp-{}-{nonce}",
        filename.to_string_lossy(),
        std::process::id()
    ))
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
        let secure = SecureProjectionDirectory::open(directory.path(), false).unwrap();
        let first = ProjectionLock::acquire_at(&secure, path.file_name().unwrap()).unwrap();
        assert!(lock_path.exists());
        drop(first);
        let second = ProjectionLock::acquire_at(&secure, path.file_name().unwrap()).unwrap();
        assert!(lock_path.exists());
        drop(second);
    }

    #[test]
    fn never_reclaims_lock_owned_by_live_process() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("projection.json");
        let secure = SecureProjectionDirectory::open(directory.path(), false).unwrap();
        let lock = ProjectionLock::acquire_at(&secure, path.file_name().unwrap()).unwrap();
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
