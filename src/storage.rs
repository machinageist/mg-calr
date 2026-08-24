#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]
use std::collections::{HashMap, HashSet};

use chrono::{DateTime, LocalResult, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls, Row};
use uuid::Uuid;

use crate::application::{
    AsyncAgendaRepository, EventEdit, EventLifecycleError, EventLifecycleErrorMapping,
    ReminderDelivery, TodoEdit,
};
use crate::config::ConnectionSettings;
use crate::domain::{
    Calendar, CalendarId, Event, EventId, EventMetadata, EventStatus, EventTime, RfcUid,
    todo::{
        Priority, Project, ProjectId, RecurrenceRule, Tag, TagId, Todo, TodoDue, TodoError, TodoId,
        TodoReminder,
    },
};

pub const FOUNDATION_MIGRATION: &str = include_str!("../migrations/0001_foundation.sql");
pub const TODO_CORE_MIGRATION: &str = include_str!("../migrations/0002_todo_core.sql");
pub const TODO_RECURRENCE_MIGRATION: &str = include_str!("../migrations/0003_todo_recurrence.sql");
pub const TODO_REMINDERS_MIGRATION: &str = include_str!("../migrations/0004_todo_reminders.sql");
pub const EVENT_LIFECYCLE_MIGRATION: &str = include_str!("../migrations/0005_event_lifecycle.sql");

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "foundation",
        sql: FOUNDATION_MIGRATION,
    },
    Migration {
        version: 2,
        name: "todo_core",
        sql: TODO_CORE_MIGRATION,
    },
    Migration {
        version: 3,
        name: "todo_recurrence",
        sql: TODO_RECURRENCE_MIGRATION,
    },
    Migration {
        version: 4,
        name: "todo_reminders",
        sql: TODO_REMINDERS_MIGRATION,
    },
    Migration {
        version: 5,
        name: "event_lifecycle",
        sql: EVENT_LIFECYCLE_MIGRATION,
    },
];

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid PostgreSQL connection configuration: {0}")]
    InvalidConfiguration(#[source] tokio_postgres::Error),
    #[error(
        "could not connect to PostgreSQL: {0}. Verify PostgreSQL is running and ask an administrator to create a peer-auth role matching the OS user plus database mg_calr; mg-calr never provisions them"
    )]
    Connect(#[source] tokio_postgres::Error),
    #[error("PostgreSQL operation failed: {0}")]
    Query(#[source] tokio_postgres::Error),
    #[error("calendar {calendar_id} does not exist or is deleted")]
    CalendarNotLive { calendar_id: CalendarId },
    #[error("todo {todo_id} does not exist or is trashed")]
    TodoNotFound { todo_id: TodoId },
    #[error("event {event_id} does not exist or is cancelled")]
    EventNotFound { event_id: EventId },
    #[error(
        "event {event_id} version conflict: expected {expected_version}, actual {actual_version}"
    )]
    EventVersionConflict {
        event_id: EventId,
        expected_version: i64,
        actual_version: i64,
    },
    #[error("todo {todo_id} has child todos and cannot be purged")]
    TodoHasChildren { todo_id: TodoId },
    #[error("todo {todo_id} is not trashed")]
    TodoNotTrashed { todo_id: TodoId },
    #[error("project {project_id} does not exist or is archived")]
    ProjectNotFound { project_id: ProjectId },
    #[error("parent todo {todo_id} does not exist or is not live")]
    ParentNotFound { todo_id: TodoId },
    #[error("todo {todo_id} cannot be its own parent")]
    SelfParent { todo_id: TodoId },
    #[error("assigning parent would create a cycle for todo {todo_id}")]
    Cycle { todo_id: TodoId },
    #[error("dependency todo {todo_id} does not exist or is not live")]
    DependencyNotFound { todo_id: TodoId },
    #[error("todo {todo_id} cannot depend on itself")]
    SelfDependency { todo_id: TodoId },
    #[error("adding dependencies would create a cycle for todo {todo_id}")]
    DependencyCycle { todo_id: TodoId },
    #[error("tag '{normalized_name}' already exists")]
    TagAlreadyExists { normalized_name: String },
    #[error("tag {tag_id} does not exist")]
    TagNotFound { tag_id: TagId },
    #[error(
        "todo {todo_id} version conflict: expected {expected_version}, actual {actual_version}"
    )]
    TodoVersionConflict {
        todo_id: TodoId,
        expected_version: i64,
        actual_version: i64,
    },
    #[error("invalid recurrence rule: {reason}")]
    InvalidRecurrence { reason: String },
    #[error("invalid reminder: {reason}")]
    InvalidReminder { reason: String },
    #[error("invalid import payload: {reason}")]
    ImportInvalid { reason: String },
    #[error("import conflicts with existing {kind} {id}")]
    ImportConflict { kind: &'static str, id: String },
    #[error("stored calendar/event data is invalid: {0}")]
    InvalidStoredData(String),
    #[error("migration version {version} is recorded as '{actual}', expected '{expected}'")]
    MigrationDrift {
        version: i64,
        actual: String,
        expected: &'static str,
    },
}

impl EventLifecycleErrorMapping for StorageError {
    fn map_event_lifecycle_error(
        self,
        event_id: EventId,
        expected_version: i64,
    ) -> EventLifecycleError<Self> {
        match self {
            Self::EventNotFound { .. } => EventLifecycleError::NotFound { event_id },
            Self::EventVersionConflict { actual_version, .. } => {
                EventLifecycleError::VersionConflict {
                    event_id,
                    expected_version,
                    actual_version,
                }
            }
            error => EventLifecycleError::Repository(error),
        }
    }
}

/// Versioned, lossless interchange document for local todo state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoExport {
    pub schema_version: u8,
    pub projects: Vec<Project>,
    pub tags: Vec<Tag>,
    pub todos: Vec<Todo>,
}

impl TodoExport {
    /// Parse and validate the complete document without opening a database.
    pub fn parse(json: &str) -> Result<Self, StorageError> {
        let mut payload: Self =
            serde_json::from_str(json).map_err(|error| StorageError::ImportInvalid {
                reason: error.to_string(),
            })?;
        payload.validate()?;
        payload
            .projects
            .sort_by_key(|project| (project.normalized_name.clone(), project.id.as_uuid()));
        payload
            .tags
            .sort_by_key(|tag| (tag.normalized_name.clone(), tag.id.as_uuid()));
        payload
            .todos
            .sort_by_key(|todo| (todo.title.to_lowercase(), todo.id.as_uuid()));
        for todo in &mut payload.todos {
            let tag_count = todo.tag_ids.len();
            todo.tag_ids.sort_unstable_by_key(|id| id.as_uuid());
            todo.tag_ids.dedup();
            if todo.tag_ids.len() != tag_count {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} contains duplicate tags", todo.id),
                });
            }
            let dependency_count = todo.dependency_ids.len();
            todo.dependency_ids.sort_unstable_by_key(|id| id.as_uuid());
            todo.dependency_ids.dedup();
            if todo.dependency_ids.len() != dependency_count {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} contains duplicate dependencies", todo.id),
                });
            }
            todo.reminders
                .sort_by_key(|reminder| (reminder.minutes_before, reminder.repeatable));
        }
        Ok(payload)
    }

    #[allow(clippy::too_many_lines)]
    fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version != 1 {
            return Err(StorageError::ImportInvalid {
                reason: format!("unsupported schema_version {}", self.schema_version),
            });
        }
        let mut projects = HashSet::new();
        let mut project_names = HashSet::new();
        for project in &self.projects {
            if project.version < 1 {
                return Err(StorageError::ImportInvalid {
                    reason: format!("project {} has invalid version", project.id),
                });
            }
            if project.archived_at.is_none()
                && !project_names.insert(project.normalized_name.clone())
            {
                return Err(StorageError::ImportInvalid {
                    reason: format!(
                        "duplicate normalized project name {}",
                        project.normalized_name
                    ),
                });
            }
            project
                .clone()
                .rehydrate()
                .map_err(|error| StorageError::ImportInvalid {
                    reason: error.to_string(),
                })?;
            if !projects.insert(project.id.as_uuid()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("duplicate project {}", project.id),
                });
            }
        }
        let mut tags = HashSet::new();
        let mut tag_names = HashSet::new();
        for tag in &self.tags {
            if !tag_names.insert(tag.normalized_name.clone()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("duplicate normalized tag name {}", tag.normalized_name),
                });
            }
            tag.clone()
                .rehydrate()
                .map_err(|error| StorageError::ImportInvalid {
                    reason: error.to_string(),
                })?;
            if !tags.insert(tag.id.as_uuid()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("duplicate tag {}", tag.id),
                });
            }
        }
        let mut todos = HashSet::new();
        for todo in &self.todos {
            let mut tag_ids = HashSet::new();
            if todo.tag_ids.iter().any(|id| !tag_ids.insert(id.as_uuid())) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} contains duplicate tags", todo.id),
                });
            }
            let mut dependency_ids = HashSet::new();
            if todo
                .dependency_ids
                .iter()
                .any(|id| !dependency_ids.insert(id.as_uuid()))
            {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} contains duplicate dependencies", todo.id),
                });
            }
            if todo.version < 1 {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} has invalid version", todo.id),
                });
            }
            if let Some(due) = &todo.due {
                match due {
                    TodoDue::Date { date, timezone } => TodoDue::date(*date, timezone.clone()),
                    TodoDue::Timed { at, timezone } => TodoDue::timed(*at, timezone.clone()),
                }
                .map_err(|error| StorageError::ImportInvalid {
                    reason: error.to_string(),
                })?;
            }
            todo.clone()
                .rehydrate()
                .map_err(|error| StorageError::ImportInvalid {
                    reason: error.to_string(),
                })?;
            if !todos.insert(todo.id.as_uuid()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("duplicate todo {}", todo.id),
                });
            }
        }
        for todo in &self.todos {
            if todo
                .project_id
                .is_some_and(|id| !projects.contains(&id.as_uuid()))
            {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} references missing project", todo.id),
                });
            }
            if todo.tag_ids.iter().any(|id| !tags.contains(&id.as_uuid())) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} references missing tag", todo.id),
                });
            }
            if todo
                .parent_id
                .is_some_and(|id| id == todo.id || !todos.contains(&id.as_uuid()))
            {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} has invalid parent", todo.id),
                });
            }
            if todo
                .dependency_ids
                .iter()
                .any(|id| *id == todo.id || !todos.contains(&id.as_uuid()))
            {
                return Err(StorageError::ImportInvalid {
                    reason: format!("todo {} has invalid dependency", todo.id),
                });
            }
        }
        let parent_graph: HashMap<Uuid, Vec<Uuid>> = self
            .todos
            .iter()
            .filter_map(|todo| {
                todo.parent_id
                    .map(|parent| (todo.id.as_uuid(), vec![parent.as_uuid()]))
            })
            .collect();
        if import_graph_has_cycle(&parent_graph) {
            return Err(StorageError::ImportInvalid {
                reason: "todo parent graph contains a cycle".to_owned(),
            });
        }
        let dependency_graph: HashMap<Uuid, Vec<Uuid>> = self
            .todos
            .iter()
            .map(|todo| {
                (
                    todo.id.as_uuid(),
                    todo.dependency_ids.iter().map(|id| id.as_uuid()).collect(),
                )
            })
            .collect();
        if import_graph_has_cycle(&dependency_graph) {
            return Err(StorageError::ImportInvalid {
                reason: "todo dependency graph contains a cycle".to_owned(),
            });
        }
        Ok(())
    }
}

fn import_graph_has_cycle(graph: &HashMap<Uuid, Vec<Uuid>>) -> bool {
    fn visit(node: Uuid, graph: &HashMap<Uuid, Vec<Uuid>>, states: &mut HashMap<Uuid, u8>) -> bool {
        match states.get(&node).copied() {
            Some(1) => return true,
            Some(2) => return false,
            _ => {}
        }
        states.insert(node, 1);
        if graph
            .get(&node)
            .into_iter()
            .flatten()
            .any(|next| visit(*next, graph, states))
        {
            return true;
        }
        states.insert(node, 2);
        false
    }
    let mut states = HashMap::new();
    graph
        .keys()
        .copied()
        .any(|node| visit(node, graph, &mut states))
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationState {
    pub version: i64,
    pub name: String,
    pub applied: bool,
}

fn postgres_config(settings: &ConnectionSettings) -> Result<tokio_postgres::Config, StorageError> {
    match settings {
        ConnectionSettings::Url { url, .. } => {
            if let Some(dbname) = url
                .strip_prefix("postgresql:///")
                .or_else(|| url.strip_prefix("postgres:///"))
                && !dbname.is_empty()
                && !dbname.contains(['/', '?', '#'])
            {
                let mut config = tokio_postgres::Config::new();
                config.host_path("/run/postgresql").dbname(dbname);
                return Ok(config);
            }
            url.parse().map_err(StorageError::InvalidConfiguration)
        }
        ConnectionSettings::Peer {
            socket_dir,
            user,
            dbname,
            ..
        } => {
            let mut config = tokio_postgres::Config::new();
            config.host_path(socket_dir).dbname(dbname);
            if let Some(user) = user {
                config.user(user);
            }
            Ok(config)
        }
    }
}

async fn connect(
    settings: &ConnectionSettings,
) -> Result<(Client, JoinHandle<Result<(), tokio_postgres::Error>>), StorageError> {
    let (client, connection) = postgres_config(settings)?
        .connect(NoTls)
        .await
        .map_err(StorageError::Connect)?;
    let task = tokio::spawn(connection);
    Ok((client, task))
}

async fn ensure_migration_table(client: &Client) -> Result<(), StorageError> {
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS mg_calr_schema_migrations (\
             version bigint PRIMARY KEY, \
             name text NOT NULL, \
             applied_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )
        .await
        .map_err(StorageError::Query)
}

/// Read embedded migration status without changing database schema.
///
/// # Errors
///
/// Returns an error for invalid configuration, connection/query failures, or
/// a recorded migration name that conflicts with the embedded contract.
pub async fn migration_status(
    settings: &ConnectionSettings,
) -> Result<Vec<MigrationState>, StorageError> {
    let (client, _connection_task) = connect(settings).await?;
    let table_exists = client
        .query_one(
            "SELECT to_regclass('mg_calr_schema_migrations') IS NOT NULL",
            &[],
        )
        .await
        .map_err(StorageError::Query)?
        .get::<_, bool>(0);
    if !table_exists {
        return Ok(MIGRATIONS
            .iter()
            .map(|migration| MigrationState {
                version: migration.version,
                name: migration.name.to_owned(),
                applied: false,
            })
            .collect());
    }
    let rows = client
        .query(
            "SELECT version, name FROM mg_calr_schema_migrations ORDER BY version",
            &[],
        )
        .await
        .map_err(StorageError::Query)?;
    let applied = rows
        .into_iter()
        .map(|row| (row.get::<_, i64>(0), row.get::<_, String>(1)))
        .collect::<std::collections::HashMap<_, _>>();

    MIGRATIONS
        .iter()
        .map(|migration| {
            if let Some(actual) = applied.get(&migration.version)
                && actual != migration.name
            {
                return Err(StorageError::MigrationDrift {
                    version: migration.version,
                    actual: actual.clone(),
                    expected: migration.name,
                });
            }
            Ok(MigrationState {
                version: migration.version,
                name: migration.name.to_owned(),
                applied: applied.contains_key(&migration.version),
            })
        })
        .collect()
}

/// Apply pending embedded migrations in one advisory-locked transaction.
///
/// # Errors
///
/// Returns an error for invalid configuration, connection/query failures, or
/// migration drift. SQL failures roll back the migration transaction.
pub async fn migrate(settings: &ConnectionSettings) -> Result<Vec<MigrationState>, StorageError> {
    let (mut client, _connection_task) = connect(settings).await?;
    ensure_migration_table(&client).await?;
    let transaction = client.transaction().await.map_err(StorageError::Query)?;
    transaction
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&6_851_863_988_i64])
        .await
        .map_err(StorageError::Query)?;

    for migration in MIGRATIONS {
        let existing = transaction
            .query_opt(
                "SELECT name FROM mg_calr_schema_migrations WHERE version = $1",
                &[&migration.version],
            )
            .await
            .map_err(StorageError::Query)?;
        if let Some(row) = existing {
            let actual = row.get::<_, String>(0);
            if actual != migration.name {
                return Err(StorageError::MigrationDrift {
                    version: migration.version,
                    actual,
                    expected: migration.name,
                });
            }
            continue;
        }
        transaction
            .batch_execute(migration.sql)
            .await
            .map_err(StorageError::Query)?;
        transaction
            .execute(
                "INSERT INTO mg_calr_schema_migrations (version, name) VALUES ($1, $2)",
                &[&migration.version, &migration.name],
            )
            .await
            .map_err(StorageError::Query)?;
    }
    transaction.commit().await.map_err(StorageError::Query)?;
    migration_status(settings).await
}

/// Diagnose connectivity and migration state without applying migrations.
///
/// # Errors
///
/// Returns the same connection, query, and drift errors as [`migration_status`].
pub async fn doctor(settings: &ConnectionSettings) -> Result<Vec<MigrationState>, StorageError> {
    migration_status(settings).await
}

const EVENT_SELECT: &str = "SELECT e.id, e.calendar_id, e.rfc_uid, e.title, e.description, e.location, \
    e.url, e.status, e.busy, e.timezone, e.starts_at, e.ends_at, e.all_day_start, \
    e.all_day_end, e.recurrence_rule, e.extension_properties, e.created_at, e.updated_at, \
    e.deleted_at, e.remote_tombstoned_at, e.version FROM events e JOIN calendars c ON c.id = e.calendar_id";
const EVENT_ORDER: &str = "ORDER BY CASE WHEN e.all_day_start IS NOT NULL THEN 0 ELSE 1 END, \
    COALESCE(e.all_day_start, (e.starts_at AT TIME ZONE 'UTC')::date), \
    e.starts_at NULLS FIRST, lower(e.title), e.id";

const TODO_SELECT: &str = "SELECT t.id, t.parent_id, t.title, t.notes, t.due_date, t.due_at, \
    t.timezone, t.priority, t.project_id, t.completed_at, COALESCE(t.trashed_at, t.deleted_at) AS trashed_at, t.version, \
    t.created_at, t.updated_at, t.recurrence_rule, COALESCE((SELECT jsonb_agg(jsonb_build_object('minutes_before', tr.minutes_before, 'repeatable', tr.repeatable) ORDER BY tr.minutes_before, tr.repeatable) FROM todo_reminders tr WHERE tr.todo_id = t.id), '[]'::jsonb), COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = t.id ORDER BY tt.tag_id), ARRAY[]::uuid[]) AS tag_ids, \
    COALESCE(ARRAY(SELECT td.prerequisite_id FROM todo_dependencies td WHERE td.dependent_id = t.id ORDER BY td.prerequisite_id), ARRAY[]::uuid[]) FROM todos t";
const TODO_ORDER: &str = "ORDER BY CASE WHEN t.due_date IS NULL AND t.due_at IS NULL THEN 1 ELSE 0 END, \
    t.due_date NULLS LAST, t.due_at NULLS LAST, lower(t.title), t.id";

/// PostgreSQL-backed repository for calendar and event commands.
#[derive(Debug, Clone)]
pub struct PostgresCalendarEventRepository {
    settings: ConnectionSettings,
}

impl PostgresCalendarEventRepository {
    /// Create a repository using the supplied PostgreSQL connection settings.
    #[must_use]
    pub const fn new(settings: ConnectionSettings) -> Self {
        Self { settings }
    }

    /// Insert a calendar in a transaction.
    ///
    /// # Errors
    /// Returns a database error; the transaction rolls back on failure.
    pub async fn save_calendar(&self, calendar: &Calendar) -> Result<(), StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        transaction
            .execute(
                "INSERT INTO calendars (id, name, color, is_default, created_at, updated_at, deleted_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &calendar.id.as_uuid(), &calendar.name, &calendar.color,
                    &calendar.is_default, &calendar.created_at, &calendar.updated_at,
                    &calendar.deleted_at,
                ],
            )
            .await
            .map_err(StorageError::Query)?;
        transaction.commit().await.map_err(StorageError::Query)
    }

    /// Insert an event only when its calendar is live.
    ///
    /// # Errors
    /// Returns [`StorageError::CalendarNotLive`] when the parent calendar is
    /// absent or soft-deleted, or a database error otherwise.
    pub async fn save_event(&self, event: &Event) -> Result<(), StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let calendar_is_live = transaction
            .query_opt(
                "SELECT deleted_at IS NULL FROM calendars WHERE id = $1 FOR UPDATE",
                &[&event.calendar_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?
            .is_some_and(|row| row.get::<_, bool>(0));
        if !calendar_is_live {
            return Err(StorageError::CalendarNotLive {
                calendar_id: event.calendar_id,
            });
        }

        let (timezone, starts_at, ends_at, all_day_start, all_day_end) = match &event.time {
            EventTime::Timed {
                start,
                end,
                timezone,
            } => (
                Some(timezone.as_str()),
                Some(*start),
                Some(*end),
                None,
                None,
            ),
            EventTime::AllDay {
                start,
                end_exclusive,
            } => (None, None, None, Some(*start), Some(*end_exclusive)),
        };
        let status = event.metadata.status.map(event_status);
        let extension_properties = serde_json::json!({
            "categories": event.metadata.categories,
            "alarms": event.metadata.alarms,
            "organizer": event.metadata.organizer,
            "attendees": event.metadata.attendees,
        });
        transaction
            .execute(
                "INSERT INTO events (id, calendar_id, rfc_uid, title, description, location, url, status, busy, timezone, starts_at, ends_at, all_day_start, all_day_end, recurrence_rule, extension_properties, created_at, updated_at, deleted_at, remote_tombstoned_at, version) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)",
                &[
                    &event.id.as_uuid(), &event.calendar_id.as_uuid(), &event.rfc_uid.as_str(),
                    &event.title, &event.metadata.description, &event.metadata.location,
                    &event.metadata.url, &status, &event.metadata.busy, &timezone, &starts_at,
                    &ends_at, &all_day_start, &all_day_end, &event.metadata.recurrence_rule,
                    &extension_properties, &event.created_at, &event.updated_at,
                    &event.deleted_at, &event.remote_tombstoned_at, &event.version,
                ],
            )
            .await
            .map_err(StorageError::Query)?;
        transaction.commit().await.map_err(StorageError::Query)
    }

    /// Cancel a live event with an atomic optimistic-version check.
    pub async fn cancel_event(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<Event, StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let current = transaction
            .query_opt(
                "SELECT version, deleted_at FROM events WHERE id = $1 FOR UPDATE",
                &[&event_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = current else {
            return Err(StorageError::EventNotFound { event_id });
        };
        if row.get::<_, Option<DateTime<Utc>>>(1).is_some() {
            return Err(StorageError::EventNotFound { event_id });
        }
        let actual_version = row.get::<_, i64>(0);
        if actual_version != expected_version {
            return Err(StorageError::EventVersionConflict {
                event_id,
                expected_version,
                actual_version,
            });
        }
        transaction
            .execute(
                "UPDATE events SET deleted_at = CURRENT_TIMESTAMP, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL AND version = $2",
                &[&event_id.as_uuid(), &expected_version],
            )
            .await
            .map_err(StorageError::Query)?;
        let event = transaction
            .query_one(
                &format!("{EVENT_SELECT} WHERE e.id = $1"),
                &[&event_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)
            .and_then(|row| event_from_row(&row))?;
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(event)
    }

    /// Restore a cancelled event with an atomic optimistic-version check.
    pub async fn restore_event(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<Event, StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let current = transaction
            .query_opt(
                "SELECT version, deleted_at FROM events WHERE id = $1 FOR UPDATE",
                &[&event_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = current else {
            return Err(StorageError::EventNotFound { event_id });
        };
        if row.get::<_, Option<DateTime<Utc>>>(1).is_none() {
            return Err(StorageError::EventNotFound { event_id });
        }
        let actual_version = row.get::<_, i64>(0);
        if actual_version != expected_version {
            return Err(StorageError::EventVersionConflict {
                event_id,
                expected_version,
                actual_version,
            });
        }
        transaction
            .execute(
                "UPDATE events SET deleted_at = NULL, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NOT NULL AND version = $2",
                &[&event_id.as_uuid(), &expected_version],
            )
            .await
            .map_err(StorageError::Query)?;
        let event = transaction
            .query_one(
                &format!("{EVENT_SELECT} WHERE e.id = $1"),
                &[&event_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)
            .and_then(|row| event_from_row(&row))?;
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(event)
    }

    /// Edit title and/or temporal columns atomically with an optimistic version check.
    pub async fn edit_event(
        &self,
        event_id: EventId,
        expected_version: i64,
        edit: &EventEdit,
    ) -> Result<Event, StorageError> {
        let (timezone, starts_at, ends_at, all_day_start, all_day_end) = match &edit.time {
            Some(EventTime::Timed {
                start,
                end,
                timezone,
            }) => (
                Some(timezone.clone()),
                Some(start.with_timezone(&Utc)),
                Some(end.with_timezone(&Utc)),
                None,
                None,
            ),
            Some(EventTime::AllDay {
                start,
                end_exclusive,
            }) => (None, None, None, Some(*start), Some(*end_exclusive)),
            None => (None, None, None, None, None),
        };
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let current = transaction
            .query_opt(
                "SELECT version, deleted_at FROM events WHERE id = $1 FOR UPDATE",
                &[&event_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = current else {
            return Err(StorageError::EventNotFound { event_id });
        };
        if row.get::<_, Option<DateTime<Utc>>>(1).is_some() {
            return Err(StorageError::EventNotFound { event_id });
        }
        let actual_version = row.get::<_, i64>(0);
        if actual_version != expected_version {
            return Err(StorageError::EventVersionConflict {
                event_id,
                expected_version,
                actual_version,
            });
        }
        let time_changed = edit.time.is_some();
        transaction
            .execute(
                "UPDATE events SET title = COALESCE($3, title), timezone = CASE WHEN $4 THEN $5 ELSE timezone END, starts_at = CASE WHEN $4 THEN $6 ELSE starts_at END, ends_at = CASE WHEN $4 THEN $7 ELSE ends_at END, all_day_start = CASE WHEN $4 THEN $8 ELSE all_day_start END, all_day_end = CASE WHEN $4 THEN $9 ELSE all_day_end END, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND deleted_at IS NULL AND version = $2",
                &[
                    &event_id.as_uuid(),
                    &expected_version,
                    &edit.title,
                    &time_changed,
                    &timezone,
                    &starts_at,
                    &ends_at,
                    &all_day_start,
                    &all_day_end,
                ],
            )
            .await
            .map_err(StorageError::Query)?;
        let event = transaction
            .query_one(
                &format!("{EVENT_SELECT} WHERE e.id = $1"),
                &[&event_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)
            .and_then(|row| event_from_row(&row))?;
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(event)
    }

    /// List live calendars in deterministic name/ID order.
    ///
    /// # Errors
    /// Returns connection, query, or invalid stored-data errors.
    pub async fn list_calendars(&self) -> Result<Vec<Calendar>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        let rows = client
            .query(
                "SELECT id, name, color, is_default, created_at, updated_at, deleted_at \
                 FROM calendars WHERE deleted_at IS NULL ORDER BY lower(name), id",
                &[],
            )
            .await
            .map_err(StorageError::Query)?;
        rows.iter().map(calendar_from_row).collect()
    }

    /// Find one live event. Absence is represented explicitly for the application layer.
    ///
    /// # Errors
    /// Returns connection, query, or invalid stored-data errors.
    pub async fn find_event(&self, event_id: EventId) -> Result<Option<Event>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        client
            .query_opt(
                &format!("{EVENT_SELECT} WHERE e.id = $1 AND e.deleted_at IS NULL AND c.deleted_at IS NULL"),
                &[&event_id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?
            .as_ref()
            .map(event_from_row)
            .transpose()
    }

    /// List live events, optionally scoped to one live calendar, deterministically.
    ///
    /// # Errors
    /// Returns connection, query, or invalid stored-data errors.
    pub async fn list_events(
        &self,
        calendar_id: Option<CalendarId>,
    ) -> Result<Vec<Event>, StorageError> {
        self.list_events_with_trashed(calendar_id, false).await
    }

    pub async fn list_events_with_trashed(
        &self,
        calendar_id: Option<CalendarId>,
        _include_trashed: bool,
    ) -> Result<Vec<Event>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        let calendar_uuid = calendar_id.map(CalendarId::as_uuid);
        let rows = client
            .query(
                &format!(
                    "{EVENT_SELECT} WHERE e.deleted_at IS NULL AND c.deleted_at IS NULL \
                     AND ($1::uuid IS NULL OR e.calendar_id = $1) {EVENT_ORDER}"
                ),
                &[&calendar_uuid],
            )
            .await
            .map_err(StorageError::Query)?;
        rows.iter().map(event_from_row).collect()
    }

    /// List events overlapping a local calendar day in deterministic agenda order.
    ///
    /// # Errors
    /// Returns connection, query, day-overflow, or invalid stored-data errors.
    pub async fn day_agenda(
        &self,
        date: NaiveDate,
        _timezone: &str,
        starts_at: DateTime<chrono::FixedOffset>,
        ends_at: DateTime<chrono::FixedOffset>,
    ) -> Result<Vec<Event>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        let rows = client
            .query(
                &format!(
                    "{EVENT_SELECT} WHERE e.deleted_at IS NULL AND c.deleted_at IS NULL AND (\
                     (e.all_day_start IS NOT NULL AND e.all_day_start < $2 AND e.all_day_end > $1) OR \
                     (e.starts_at IS NOT NULL AND e.starts_at < $4 AND e.ends_at > $3)) {EVENT_ORDER}"
                ),
                &[&date, &date.succ_opt().ok_or_else(|| {
                    StorageError::InvalidStoredData("day agenda date overflow".to_owned())
                })?, &starts_at, &ends_at],
            )
            .await
            .map_err(StorageError::Query)?;
        rows.iter().map(event_from_row).collect()
    }
}

/// PostgreSQL-backed repository for tag metadata.
#[derive(Debug, Clone)]
pub struct PostgresTagRepository {
    settings: ConnectionSettings,
}
impl PostgresTagRepository {
    pub const fn new(settings: ConnectionSettings) -> Self {
        Self { settings }
    }
    pub async fn save_tag(&self, tag: &Tag) -> Result<(), StorageError> {
        let (mut client, _) = connect(&self.settings).await?;
        let tx = client.transaction().await.map_err(StorageError::Query)?;
        tx.execute("INSERT INTO tags (id, name, normalized_name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)", &[&tag.id.as_uuid(), &tag.name, &tag.normalized_name, &tag.created_at, &tag.updated_at]).await.map_err(|error| {
            if error
                .code()
                .is_some_and(|code| code.code() == "23505")
                && error
                    .as_db_error()
                    .and_then(tokio_postgres::error::DbError::constraint)
                    .is_some_and(|constraint| constraint == "tags_normalized_name_unique")
            {
                StorageError::TagAlreadyExists {
                    normalized_name: tag.normalized_name.clone(),
                }
            } else {
                StorageError::Query(error)
            }
        })?;
        tx.commit().await.map_err(StorageError::Query)
    }
    pub async fn list_tags(&self) -> Result<Vec<Tag>, StorageError> {
        let (client, _) = connect(&self.settings).await?;
        let rows = client.query("SELECT id, name, normalized_name, created_at, updated_at FROM tags ORDER BY normalized_name, id", &[]).await.map_err(StorageError::Query)?;
        rows.iter().map(tag_from_row).collect()
    }
}

/// PostgreSQL-backed repository for project metadata.
#[derive(Debug, Clone)]
pub struct PostgresProjectRepository {
    settings: ConnectionSettings,
}

impl PostgresProjectRepository {
    #[must_use]
    pub const fn new(settings: ConnectionSettings) -> Self {
        Self { settings }
    }

    /// Insert one project row in a transaction.
    ///
    /// # Errors
    /// Returns connection or PostgreSQL errors.
    pub async fn save_project(&self, project: &Project) -> Result<(), StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        transaction.execute(
            "INSERT INTO projects (id, name, normalized_name, archived_at, version, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[&project.id.as_uuid(), &project.name, &project.normalized_name,
              &project.archived_at, &project.version, &project.created_at, &project.updated_at],
        ).await.map_err(StorageError::Query)?;
        transaction.commit().await.map_err(StorageError::Query)
    }

    /// Find one project, including archived rows.
    ///
    /// # Errors
    /// Returns connection, query, or invalid stored-data errors.
    pub async fn find_project(&self, id: ProjectId) -> Result<Option<Project>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        client.query_opt(
            "SELECT id, name, normalized_name, archived_at, version, created_at, updated_at FROM projects WHERE id = $1",
            &[&id.as_uuid()],
        ).await.map_err(StorageError::Query)?.as_ref().map(project_from_row).transpose()
    }

    /// List live projects in deterministic name/ID order.
    ///
    /// # Errors
    /// Returns connection, query, or invalid stored-data errors.
    pub async fn list_projects(&self) -> Result<Vec<Project>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        let rows = client.query(
            "SELECT id, name, normalized_name, archived_at, version, created_at, updated_at FROM projects WHERE archived_at IS NULL ORDER BY normalized_name, id",
            &[],
        ).await.map_err(StorageError::Query)?;
        rows.iter().map(project_from_row).collect()
    }
}

/// PostgreSQL-backed repository for todo persistence. Todo rows and their
/// tag join rows are authoritative in PostgreSQL.
#[derive(Debug, Clone)]
pub struct PostgresTodoRepository {
    settings: ConnectionSettings,
}

impl PostgresTodoRepository {
    #[must_use]
    pub const fn new(settings: ConnectionSettings) -> Self {
        Self { settings }
    }

    /// Insert a todo row and its tag join rows in one transaction.
    ///
    /// # Errors
    /// Returns connection or PostgreSQL errors.
    pub async fn save_todo(&self, todo: &Todo) -> Result<(), StorageError> {
        todo.clone().rehydrate().map_err(|error| match error {
            TodoError::InvalidRecurrenceInterval
            | TodoError::InvalidRecurrenceCount
            | TodoError::RecurrenceUntilNotAfterDue
            | TodoError::RecurrenceWithoutDue
            | TodoError::InvalidRecurrenceRange => recurrence_error(&error),
            other => StorageError::InvalidStoredData(other.to_string()),
        })?;
        let recurrence_rule = todo
            .recurrence
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let mut dependency_uuids = todo
            .dependency_ids
            .iter()
            .map(|dependency_id| dependency_id.as_uuid())
            .collect::<Vec<_>>();
        dependency_uuids.sort_unstable();
        dependency_uuids.dedup();
        if dependency_uuids.contains(&todo.id.as_uuid()) {
            return Err(StorageError::SelfDependency { todo_id: todo.id });
        }
        if !dependency_uuids.is_empty() {
            let live_count: i64 = transaction
                .query_one(
                    "SELECT COUNT(*) FROM todos WHERE id = ANY($1::uuid[]) AND trashed_at IS NULL AND deleted_at IS NULL",
                    &[&dependency_uuids],
                )
                .await
                .map_err(StorageError::Query)?
                .get(0);
            if usize::try_from(live_count).ok() != Some(dependency_uuids.len()) {
                let missing = transaction
                    .query_opt(
                        "SELECT requested_id FROM UNNEST($1::uuid[]) AS requested_id LEFT JOIN todos t ON t.id = requested_id AND t.trashed_at IS NULL AND t.deleted_at IS NULL WHERE t.id IS NULL ORDER BY requested_id LIMIT 1",
                        &[&dependency_uuids],
                    )
                    .await
                    .map_err(StorageError::Query)?
                    .map_or(dependency_uuids[0], |row| row.get::<_, Uuid>(0));
                return Err(StorageError::DependencyNotFound {
                    todo_id: TodoId::from_uuid(missing),
                });
            }
        }
        let (due_date, due_at, timezone) = match &todo.due {
            Some(TodoDue::Date { date, timezone }) => (Some(*date), None, Some(timezone.as_str())),
            Some(TodoDue::Timed { at, timezone }) => {
                (None, Some(at.with_timezone(&Utc)), Some(timezone.as_str()))
            }
            None => (None, None, None),
        };
        transaction.execute(
            "INSERT INTO todos (id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, trashed_at, version, created_at, updated_at, recurrence_rule) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
            &[&todo.id.as_uuid(), &todo.parent_id.map(TodoId::as_uuid), &todo.title,
              &todo.notes, &due_date, &due_at, &timezone, &todo.priority.to_string(),
              &todo.project_id.map(ProjectId::as_uuid), &todo.completed_at, &todo.trashed_at,
              &todo.version, &todo.created_at, &todo.updated_at, &recurrence_rule],
        ).await.map_err(StorageError::Query)?;
        for tag_id in &todo.tag_ids {
            transaction
                .execute(
                    "INSERT INTO todo_tags (todo_id, tag_id) VALUES ($1, $2)",
                    &[&todo.id.as_uuid(), &tag_id.as_uuid()],
                )
                .await
                .map_err(StorageError::Query)?;
        }
        for prerequisite_id in &dependency_uuids {
            transaction
                .execute(
                    "INSERT INTO todo_dependencies (dependent_id, prerequisite_id) VALUES ($1, $2)",
                    &[&todo.id.as_uuid(), prerequisite_id],
                )
                .await
                .map_err(StorageError::Query)?;
        }
        for reminder in &todo.reminders {
            transaction
                .execute(
                    "INSERT INTO todo_reminders (todo_id, minutes_before, repeatable) VALUES ($1, $2, $3)",
                    &[&todo.id.as_uuid(), &i32::try_from(reminder.minutes_before).map_err(|_| StorageError::InvalidReminder { reason: "offset overflow".to_owned() })?, &reminder.repeatable],
                )
                .await
                .map_err(StorageError::Query)?;
        }
        transaction.commit().await.map_err(StorageError::Query)
    }

    /// Find a todo, including completed and trashed rows.
    ///
    /// # Errors
    /// Returns connection, query, or invalid stored-data errors.
    pub async fn find_todo(&self, id: TodoId) -> Result<Option<Todo>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        client
            .query_opt(&format!("{TODO_SELECT} WHERE t.id = $1"), &[&id.as_uuid()])
            .await
            .map_err(StorageError::Query)?
            .as_ref()
            .map(todo_from_row)
            .transpose()
    }

    /// List todos in deterministic due/title/ID order.
    ///
    /// # Errors
    /// Returns connection, query, or invalid stored-data errors.
    pub async fn list_todos(&self) -> Result<Vec<Todo>, StorageError> {
        self.list_todos_with_trashed(false).await
    }

    pub async fn list_todos_with_trashed(
        &self,
        include_trashed: bool,
    ) -> Result<Vec<Todo>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        let rows = client
            .query(&format!("{TODO_SELECT} WHERE t.deleted_at IS NULL AND ($1 OR t.trashed_at IS NULL) {TODO_ORDER}"), &[&include_trashed])
            .await
            .map_err(StorageError::Query)?;
        rows.iter().map(todo_from_row).collect()
    }

    /// Complete a live todo only when its caller-supplied version is current.
    ///
    /// The update is atomic and returns the newly completed row. A stale
    /// version or a trashed/missing row never mutates the todo.
    ///
    /// # Errors
    /// Returns a database error, [`StorageError::TodoNotFound`], or
    /// [`StorageError::TodoVersionConflict`].
    pub async fn complete_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> Result<Todo, StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let row = transaction
            .query_opt(
                "UPDATE todos SET completed_at = CURRENT_TIMESTAMP, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND trashed_at IS NULL AND deleted_at IS NULL AND completed_at IS NULL AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, recurrence_rule, COALESCE((SELECT jsonb_agg(jsonb_build_object('minutes_before', tr.minutes_before, 'repeatable', tr.repeatable) ORDER BY tr.minutes_before, tr.repeatable) FROM todo_reminders tr WHERE tr.todo_id = todos.id), '[]'::jsonb), COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[]), COALESCE(ARRAY(SELECT td.prerequisite_id FROM todo_dependencies td WHERE td.dependent_id = todos.id ORDER BY td.prerequisite_id), ARRAY[]::uuid[])",
                &[&id.as_uuid(), &expected_version],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = row else {
            let state = transaction
                .query_opt(
                    "SELECT version FROM todos WHERE id = $1 AND trashed_at IS NULL AND deleted_at IS NULL",
                    &[&id.as_uuid()],
                )
                .await
                .map_err(StorageError::Query)?;
            return match state {
                Some(row) => Err(StorageError::TodoVersionConflict {
                    todo_id: id,
                    expected_version,
                    actual_version: row.get(0),
                }),
                None => Err(StorageError::TodoNotFound { todo_id: id }),
            };
        };
        let todo = todo_from_row(&row)?;
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(todo)
    }

    /// Trash a live todo only when its caller-supplied version is current.
    /// Completed todos are allowed; `deleted_at` remains untouched for legacy
    /// compatibility. Repeating trash is an optimistic conflict.
    ///
    /// # Errors
    /// Returns a database error, not-found error, or optimistic version conflict.
    pub async fn trash_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> Result<Todo, StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let row = transaction
            .query_opt(
                "UPDATE todos SET trashed_at = CURRENT_TIMESTAMP, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND trashed_at IS NULL AND deleted_at IS NULL AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, recurrence_rule, COALESCE((SELECT jsonb_agg(jsonb_build_object('minutes_before', tr.minutes_before, 'repeatable', tr.repeatable) ORDER BY tr.minutes_before, tr.repeatable) FROM todo_reminders tr WHERE tr.todo_id = todos.id), '[]'::jsonb), COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[]), COALESCE(ARRAY(SELECT td.prerequisite_id FROM todo_dependencies td WHERE td.dependent_id = todos.id ORDER BY td.prerequisite_id), ARRAY[]::uuid[])",
                &[&id.as_uuid(), &expected_version],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = row else {
            return todo_lifecycle_conflict(&transaction, id, expected_version, false).await;
        };
        let todo = todo_from_row(&row)?;
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(todo)
    }

    /// Restore a trashed todo only when its caller-supplied version is current.
    /// Legacy `deleted_at` tombstones are cleared as part of restoration.
    ///
    /// # Errors
    /// Returns a database error, not-found error, or optimistic version conflict.
    pub async fn restore_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> Result<Todo, StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let row = transaction
            .query_opt(
                "UPDATE todos SET trashed_at = NULL, deleted_at = NULL, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND (trashed_at IS NOT NULL OR deleted_at IS NOT NULL) AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, recurrence_rule, COALESCE((SELECT jsonb_agg(jsonb_build_object('minutes_before', tr.minutes_before, 'repeatable', tr.repeatable) ORDER BY tr.minutes_before, tr.repeatable) FROM todo_reminders tr WHERE tr.todo_id = todos.id), '[]'::jsonb), COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[]), COALESCE(ARRAY(SELECT td.prerequisite_id FROM todo_dependencies td WHERE td.dependent_id = todos.id ORDER BY td.prerequisite_id), ARRAY[]::uuid[])",
                &[&id.as_uuid(), &expected_version],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = row else {
            return todo_lifecycle_conflict(&transaction, id, expected_version, true).await;
        };
        let todo = todo_from_row(&row)?;
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(todo)
    }

    /// Permanently delete a trashed todo when its caller-supplied version is current.
    /// Join rows are removed explicitly in the same transaction; dependency FKs
    /// use their existing ON DELETE CASCADE semantics.
    pub async fn purge_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> Result<TodoId, StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let Some(row) = transaction
            .query_opt(
                "SELECT version, trashed_at, deleted_at FROM todos WHERE id = $1 FOR UPDATE",
                &[&id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?
        else {
            return Err(StorageError::TodoNotFound { todo_id: id });
        };
        let trashed = row
            .get::<_, Option<chrono::DateTime<chrono::Utc>>>(1)
            .is_some()
            || row
                .get::<_, Option<chrono::DateTime<chrono::Utc>>>(2)
                .is_some();
        if !trashed {
            return Err(StorageError::TodoNotTrashed { todo_id: id });
        }
        let actual_version = row.get::<_, i64>(0);
        if actual_version != expected_version {
            return Err(StorageError::TodoVersionConflict {
                todo_id: id,
                expected_version,
                actual_version,
            });
        }
        let child_count: i64 = transaction
            .query_one(
                "SELECT COUNT(*) FROM todos WHERE parent_id = $1",
                &[&id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?
            .get(0);
        if child_count > 0 {
            return Err(StorageError::TodoHasChildren { todo_id: id });
        }
        transaction
            .execute("DELETE FROM todo_tags WHERE todo_id = $1", &[&id.as_uuid()])
            .await
            .map_err(StorageError::Query)?;
        transaction
            .execute(
                "DELETE FROM todo_dependencies WHERE dependent_id = $1 OR prerequisite_id = $1",
                &[&id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?;
        transaction
            .execute(
                "DELETE FROM todos WHERE id = $1 AND version = $2",
                &[&id.as_uuid(), &expected_version],
            )
            .await
            .map_err(StorageError::Query)?;
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(id)
    }

    /// Edit live core fields atomically when the caller-supplied version is current.
    ///
    /// # Errors
    /// Returns a database error, not-found error, or optimistic version conflict.
    #[allow(clippy::too_many_lines)]
    pub async fn edit_todo(
        &self,
        id: TodoId,
        expected_version: i64,
        edit: TodoEdit,
    ) -> Result<Todo, StorageError> {
        if let Some(Some(rule)) = &edit.recurrence {
            rule.validate().map_err(|error| recurrence_error(&error))?;
        }
        if let Some(reminders) = &edit.reminders {
            for reminder in reminders {
                TodoReminder::new(reminder.minutes_before, reminder.repeatable).map_err(
                    |error| StorageError::InvalidReminder {
                        reason: error.to_string(),
                    },
                )?;
            }
        }
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        // Serialize hierarchy edits before taking target/ancestor row locks so
        // reciprocal assignments cannot acquire rows in opposite orders.
        transaction
            .batch_execute("LOCK TABLE todos IN SHARE ROW EXCLUSIVE MODE")
            .await
            .map_err(StorageError::Query)?;
        if edit.dependency_ids.is_some() {
            transaction
                .batch_execute("LOCK TABLE todo_dependencies IN SHARE ROW EXCLUSIVE MODE")
                .await
                .map_err(StorageError::Query)?;
        }
        let (due_changed, due_date, due_at, timezone) = match edit.due {
            Some(TodoDue::Date { date, timezone }) => (true, Some(date), None, Some(timezone)),
            Some(TodoDue::Timed { at, timezone }) => {
                (true, None, Some(at.with_timezone(&Utc)), Some(timezone))
            }
            None => (false, None, None, None),
        };
        let priority = edit.priority.map(|value| value.to_string());
        let title = edit.title;
        let notes_changed = edit.notes.is_some();
        let notes = edit.notes.flatten();
        let project_changed = edit.project_id.is_some();
        let project_id = edit.project_id.flatten().map(ProjectId::as_uuid);
        let parent_changed = edit.parent_id.is_some();
        let parent_id = edit.parent_id.flatten().map(TodoId::as_uuid);
        let tag_ids = edit.tag_ids.clone().map(|ids| {
            let mut ids = ids.into_iter().map(TagId::as_uuid).collect::<Vec<_>>();
            ids.sort_unstable();
            ids.dedup();
            ids
        });
        let dependency_ids = edit.dependency_ids.clone().map(|ids| {
            let mut ids = ids.into_iter().map(TodoId::as_uuid).collect::<Vec<_>>();
            ids.sort_unstable();
            ids.dedup();
            ids
        });
        let current = transaction
            .query_opt(
                "SELECT version, trashed_at, deleted_at FROM todos WHERE id = $1 FOR UPDATE",
                &[&id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(current) = current else {
            return Err(StorageError::TodoNotFound { todo_id: id });
        };
        if current
            .get::<_, Option<chrono::DateTime<chrono::Utc>>>(1)
            .is_some()
            || current
                .get::<_, Option<chrono::DateTime<chrono::Utc>>>(2)
                .is_some()
        {
            return Err(StorageError::TodoNotFound { todo_id: id });
        }
        let actual_version = current.get::<_, i64>(0);
        if actual_version != expected_version {
            return Err(StorageError::TodoVersionConflict {
                todo_id: id,
                expected_version,
                actual_version,
            });
        }
        if let Some(parent_id) = parent_id {
            if parent_id == id.as_uuid() {
                return Err(StorageError::SelfParent { todo_id: id });
            }
            let parent = transaction
                .query_opt(
                    "SELECT parent_id, trashed_at, deleted_at FROM todos WHERE id = $1 FOR UPDATE",
                    &[&parent_id],
                )
                .await
                .map_err(StorageError::Query)?;
            let Some(parent) = parent else {
                return Err(StorageError::ParentNotFound {
                    todo_id: TodoId::from_uuid(parent_id),
                });
            };
            if parent
                .get::<_, Option<chrono::DateTime<chrono::Utc>>>(1)
                .is_some()
                || parent
                    .get::<_, Option<chrono::DateTime<chrono::Utc>>>(2)
                    .is_some()
            {
                return Err(StorageError::ParentNotFound {
                    todo_id: TodoId::from_uuid(parent_id),
                });
            }
            let mut ancestor = parent.get::<_, Option<Uuid>>(0);
            let mut visited = HashSet::new();
            while let Some(ancestor_id) = ancestor {
                if !visited.insert(ancestor_id) {
                    return Err(StorageError::Cycle { todo_id: id });
                }
                if ancestor_id == id.as_uuid() {
                    return Err(StorageError::Cycle { todo_id: id });
                }
                ancestor = transaction
                    .query_opt(
                        "SELECT parent_id FROM todos WHERE id = $1 FOR UPDATE",
                        &[&ancestor_id],
                    )
                    .await
                    .map_err(StorageError::Query)?
                    .and_then(|row| row.get::<_, Option<Uuid>>(0));
            }
        }
        if let Some(ids) = &tag_ids {
            for tag_id in ids {
                if transaction
                    .query_opt("SELECT id FROM tags WHERE id = $1 FOR UPDATE", &[tag_id])
                    .await
                    .map_err(StorageError::Query)?
                    .is_none()
                {
                    return Err(StorageError::TagNotFound {
                        tag_id: TagId::from_uuid(*tag_id),
                    });
                }
            }
        }
        if let Some(project_id) = project_id {
            let project_is_live = transaction
                .query_opt(
                    "SELECT id FROM projects WHERE id = $1 AND archived_at IS NULL FOR UPDATE",
                    &[&project_id],
                )
                .await
                .map_err(StorageError::Query)?
                .is_some();
            if !project_is_live {
                return Err(StorageError::ProjectNotFound {
                    project_id: ProjectId::from_uuid(project_id),
                });
            }
        }
        if let Some(ids) = &dependency_ids {
            if ids
                .iter()
                .any(|dependency_id| *dependency_id == id.as_uuid())
            {
                return Err(StorageError::SelfDependency { todo_id: id });
            }
            let live_ids = transaction
                .query("SELECT id FROM todos WHERE id = ANY($1::uuid[]) AND trashed_at IS NULL AND deleted_at IS NULL", &[ids])
                .await
                .map_err(StorageError::Query)?
                .into_iter()
                .map(|row| row.get::<_, Uuid>(0))
                .collect::<HashSet<_>>();
            if let Some(missing) = ids
                .iter()
                .find(|dependency_id| !live_ids.contains(dependency_id))
            {
                return Err(StorageError::DependencyNotFound {
                    todo_id: TodoId::from_uuid(*missing),
                });
            }
            let cycle = transaction
                .query_one("WITH RECURSIVE edges AS (SELECT dependent_id, prerequisite_id FROM todo_dependencies WHERE dependent_id <> $1 UNION ALL SELECT $1::uuid, unnest($2::uuid[])), reachable(todo_id) AS (SELECT prerequisite_id FROM edges WHERE dependent_id = $1 UNION SELECT edges.prerequisite_id FROM edges JOIN reachable ON edges.dependent_id = reachable.todo_id) SELECT EXISTS (SELECT 1 FROM reachable WHERE todo_id = $1)", &[&id.as_uuid(), ids])
                .await
                .map_err(StorageError::Query)?
                .get::<_, bool>(0);
            if cycle {
                return Err(StorageError::DependencyCycle { todo_id: id });
            }
        }
        if due_changed
            && due_date.is_none()
            && due_at.is_none()
            && edit.reminders.is_none()
            && transaction
                .query_opt(
                    "SELECT 1 FROM todo_reminders WHERE todo_id = $1 LIMIT 1",
                    &[&id.as_uuid()],
                )
                .await
                .map_err(StorageError::Query)?
                .is_some()
        {
            return Err(StorageError::InvalidReminder {
                reason: "reminders require a due value".to_owned(),
            });
        }
        let row = transaction
            .query_opt(
                "UPDATE todos SET title = COALESCE($3, title), priority = COALESCE($4, priority), due_date = CASE WHEN $5 THEN $6 ELSE due_date END, due_at = CASE WHEN $5 THEN $7 ELSE due_at END, timezone = CASE WHEN $5 THEN $8 ELSE timezone END, notes = CASE WHEN $9 THEN $10 ELSE notes END, project_id = CASE WHEN $11 THEN $12 ELSE project_id END, parent_id = CASE WHEN $13 THEN $14 ELSE parent_id END, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND trashed_at IS NULL AND deleted_at IS NULL AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, recurrence_rule, COALESCE((SELECT jsonb_agg(jsonb_build_object('minutes_before', tr.minutes_before, 'repeatable', tr.repeatable) ORDER BY tr.minutes_before, tr.repeatable) FROM todo_reminders tr WHERE tr.todo_id = todos.id), '[]'::jsonb), COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[]), COALESCE(ARRAY(SELECT td.prerequisite_id FROM todo_dependencies td WHERE td.dependent_id = todos.id ORDER BY td.prerequisite_id), ARRAY[]::uuid[])",
                &[&id.as_uuid(), &expected_version, &title, &priority, &due_changed, &due_date, &due_at, &timezone, &notes_changed, &notes, &project_changed, &project_id, &parent_changed, &parent_id],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = row else {
            return todo_edit_conflict(&transaction, id, expected_version).await;
        };
        let mut todo = todo_from_row(&row).map_err(|error| match error {
            StorageError::InvalidStoredData(reason) if reason.contains("recurrence") => {
                StorageError::InvalidRecurrence { reason }
            }
            other => other,
        })?;
        if let Some(recurrence) = edit.recurrence {
            let recurrence_rule = recurrence
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
            transaction
                .execute(
                    "UPDATE todos SET recurrence_rule = $2 WHERE id = $1",
                    &[&id.as_uuid(), &recurrence_rule],
                )
                .await
                .map_err(StorageError::Query)?;
            todo.recurrence = recurrence;
            todo.clone()
                .rehydrate()
                .map_err(|error| recurrence_error(&error))?;
        }
        if let Some(ids) = tag_ids {
            transaction
                .execute("DELETE FROM todo_tags WHERE todo_id = $1", &[&id.as_uuid()])
                .await
                .map_err(StorageError::Query)?;
            for tag_id in &ids {
                transaction
                    .execute(
                        "INSERT INTO todo_tags (todo_id, tag_id) VALUES ($1, $2)",
                        &[&id.as_uuid(), tag_id],
                    )
                    .await
                    .map_err(StorageError::Query)?;
            }
            todo.tag_ids = ids.into_iter().map(TagId::from_uuid).collect();
        }
        if let Some(ids) = dependency_ids {
            transaction
                .execute(
                    "DELETE FROM todo_dependencies WHERE dependent_id = $1",
                    &[&id.as_uuid()],
                )
                .await
                .map_err(StorageError::Query)?;
            for prerequisite_id in &ids {
                transaction
                    .execute("INSERT INTO todo_dependencies (dependent_id, prerequisite_id) VALUES ($1, $2)", &[&id.as_uuid(), prerequisite_id])
                    .await
                    .map_err(StorageError::Query)?;
            }
            todo.dependency_ids = ids.into_iter().map(TodoId::from_uuid).collect();
        } else {
            todo.dependency_ids = transaction
                .query("SELECT prerequisite_id FROM todo_dependencies WHERE dependent_id = $1 ORDER BY prerequisite_id", &[&id.as_uuid()])
                .await
                .map_err(StorageError::Query)?
                .into_iter()
                .map(|row| TodoId::from_uuid(row.get(0)))
                .collect();
        }
        if let Some(reminders) = edit.reminders {
            todo.reminders = reminders;
            todo.clone()
                .rehydrate()
                .map_err(|error| StorageError::InvalidReminder {
                    reason: error.to_string(),
                })?;
            transaction
                .execute(
                    "DELETE FROM todo_reminders WHERE todo_id = $1",
                    &[&id.as_uuid()],
                )
                .await
                .map_err(StorageError::Query)?;
            for reminder in &todo.reminders {
                transaction
                    .execute(
                        "INSERT INTO todo_reminders (todo_id, minutes_before, repeatable) VALUES ($1, $2, $3)",
                        &[&id.as_uuid(), &i32::try_from(reminder.minutes_before).map_err(|_| StorageError::InvalidReminder { reason: "offset overflow".to_owned() })?, &reminder.repeatable],
                    )
                    .await
                    .map_err(StorageError::Query)?;
            }
        }
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(todo)
    }

    /// Query live, incomplete reminders due by an instant in stable order.
    pub async fn due_reminders(
        &self,
        at: DateTime<Utc>,
    ) -> Result<Vec<crate::application::Reminder>, StorageError> {
        let (client, _connection_task) = connect(&self.settings).await?;
        let rows = client
            .query(
                "SELECT t.id, t.title, COALESCE(t.due_at, (t.due_date::timestamp + time '09:00') AT TIME ZONE t.timezone) - (tr.minutes_before * INTERVAL '1 minute') AS trigger_at, tr.minutes_before, tr.repeatable FROM todos t JOIN todo_reminders tr ON tr.todo_id = t.id WHERE (t.due_date IS NOT NULL OR t.due_at IS NOT NULL) AND t.completed_at IS NULL AND t.trashed_at IS NULL AND t.deleted_at IS NULL AND NOT EXISTS (SELECT 1 FROM todo_dependencies dep JOIN todos prerequisite ON prerequisite.id = dep.prerequisite_id WHERE dep.dependent_id = t.id AND prerequisite.completed_at IS NULL AND prerequisite.trashed_at IS NULL AND prerequisite.deleted_at IS NULL) AND COALESCE(t.due_at, (t.due_date::timestamp + time '09:00') AT TIME ZONE t.timezone) - (tr.minutes_before * INTERVAL '1 minute') <= $1 ORDER BY trigger_at, lower(t.title), t.id, tr.minutes_before, tr.repeatable",
                &[&at],
            )
            .await
            .map_err(StorageError::Query)?;
        rows.into_iter()
            .map(|row| {
                Ok(crate::application::Reminder {
                    todo_id: TodoId::from_uuid(row.get(0)),
                    title: row.get(1),
                    trigger_at: row.get(2),
                    minutes_before: row.get::<_, i32>(3).try_into().map_err(|_| {
                        StorageError::InvalidReminder {
                            reason: "negative stored offset".to_owned(),
                        }
                    })?,
                    repeatable: row.get(4),
                })
            })
            .collect()
    }

    /// Record due todo reminders once. No notification transport is invoked.
    pub async fn scan_reminders(
        &self,
        at: DateTime<Utc>,
        dry_run: bool,
    ) -> Result<Vec<ReminderDelivery>, StorageError> {
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let query = format!(
            "{TODO_SELECT} WHERE (t.due_date IS NOT NULL OR t.due_at IS NOT NULL) AND t.completed_at IS NULL AND t.trashed_at IS NULL AND t.deleted_at IS NULL AND NOT EXISTS (SELECT 1 FROM todo_dependencies dep JOIN todos prerequisite ON prerequisite.id = dep.prerequisite_id WHERE dep.dependent_id = t.id AND prerequisite.completed_at IS NULL AND prerequisite.trashed_at IS NULL AND prerequisite.deleted_at IS NULL)"
        );
        let rows = transaction
            .query(&query, &[])
            .await
            .map_err(StorageError::Query)?;
        let mut candidates = Vec::new();
        for row in rows {
            let todo = todo_from_row(&row)?;
            let Some(due) = &todo.due else { continue };
            let timezone = match due {
                TodoDue::Date { timezone, .. } | TodoDue::Timed { timezone, .. } => timezone,
            };
            let zone = timezone.parse::<Tz>().map_err(|_| {
                StorageError::InvalidStoredData(format!("invalid todo timezone: {timezone}"))
            })?;
            // Reminder offsets are bounded to seven days (10,080 minutes), so
            // include that much local calendar time beyond the scan instant.
            // Expanding in the todo's timezone preserves civil-date and DST
            // semantics while remaining bounded by the domain recurrence cap.
            let through = at.with_timezone(&zone).date_naive() + chrono::Duration::days(7);
            let occurrences = todo
                .expand_due_instances(NaiveDate::MIN, through)
                .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
            for reminder in &todo.reminders {
                let selected = if reminder.repeatable {
                    occurrences.iter().collect::<Vec<_>>()
                } else {
                    occurrences.iter().take(1).collect::<Vec<_>>()
                };
                for occurrence in selected {
                    let scheduled_for = due_trigger_at(occurrence, reminder.minutes_before)?;
                    if scheduled_for <= at {
                        candidates.push((
                            todo.id,
                            todo.title.clone(),
                            reminder.minutes_before,
                            reminder.repeatable,
                            scheduled_for,
                        ));
                    }
                }
            }
        }
        candidates.sort_by_key(|candidate| {
            (
                candidate.4,
                candidate.1.to_lowercase(),
                candidate.0.as_uuid().as_u128(),
            )
        });
        let mut output = Vec::with_capacity(candidates.len());
        for (todo_id, title, minutes_before, repeatable, scheduled_for) in candidates {
            let reminder = transaction.query_opt(
                "SELECT id FROM reminders WHERE todo_id = $1 AND offset_seconds = $2 AND repeatable = $3",
                &[&todo_id.as_uuid(), &(i64::from(minutes_before) * 60), &repeatable],
            ).await.map_err(StorageError::Query)?;
            let reminder_id = match reminder {
                Some(row) => row.get(0),
                None if dry_run => Uuid::nil(),
                None => transaction.query_one(
                    "INSERT INTO reminders (id, todo_id, offset_seconds, repeatable) VALUES ($1, $2, $3, $4) ON CONFLICT (todo_id, offset_seconds, repeatable) WHERE todo_id IS NOT NULL AND offset_seconds IS NOT NULL DO UPDATE SET todo_id = EXCLUDED.todo_id RETURNING id",
                    &[&Uuid::now_v7(), &todo_id.as_uuid(), &(i64::from(minutes_before) * 60), &repeatable],
                ).await.map_err(StorageError::Query)?.get(0),
            };
            let delivery = if dry_run {
                transaction.query_opt(
                    "SELECT id FROM reminder_deliveries WHERE reminder_id = $1 AND scheduled_for = $2",
                    &[&reminder_id, &scheduled_for],
                ).await.map_err(StorageError::Query)?.is_some()
            } else {
                transaction.query_opt(
                    "INSERT INTO reminder_deliveries (id, reminder_id, scheduled_for) VALUES ($1, $2, $3) ON CONFLICT (reminder_id, scheduled_for) DO NOTHING RETURNING id",
                    &[&Uuid::now_v7(), &reminder_id, &scheduled_for],
                ).await.map_err(StorageError::Query)?.is_some()
            };
            output.push(ReminderDelivery {
                todo_id,
                title,
                minutes_before,
                repeatable,
                scheduled_for,
                status: if delivery {
                    "already_recorded"
                } else if dry_run {
                    "would_record"
                } else {
                    "recorded"
                }
                .to_owned(),
                transport: "none",
            });
        }
        if dry_run {
            transaction.rollback().await.map_err(StorageError::Query)?;
        } else {
            transaction.commit().await.map_err(StorageError::Query)?;
        }
        Ok(output)
    }
}

fn due_trigger_at(due: &TodoDue, minutes_before: u32) -> Result<DateTime<Utc>, StorageError> {
    let due_at = match due {
        TodoDue::Timed { at, .. } => at.with_timezone(&Utc),
        TodoDue::Date { date, timezone } => {
            let zone = timezone.parse::<Tz>().map_err(|_| {
                StorageError::InvalidStoredData(format!("invalid todo timezone: {timezone}"))
            })?;
            let local = date.and_time(NaiveTime::from_hms_opt(9, 0, 0).expect("valid time"));
            match zone.from_local_datetime(&local) {
                LocalResult::Single(value) | LocalResult::Ambiguous(value, _) => {
                    value.with_timezone(&Utc)
                }
                LocalResult::None => {
                    return Err(StorageError::InvalidStoredData(
                        "todo date falls in a nonexistent local time".to_owned(),
                    ));
                }
            }
        }
    };
    Ok(due_at - chrono::Duration::minutes(i64::from(minutes_before)))
}

async fn todo_edit_conflict(
    transaction: &tokio_postgres::Transaction<'_>,
    id: TodoId,
    expected_version: i64,
) -> Result<Todo, StorageError> {
    let state = transaction
        .query_opt(
            "SELECT version, trashed_at, deleted_at FROM todos WHERE id = $1",
            &[&id.as_uuid()],
        )
        .await
        .map_err(StorageError::Query)?;
    match state {
        Some(row)
            if row
                .get::<_, Option<chrono::DateTime<chrono::Utc>>>(1)
                .is_none()
                && row
                    .get::<_, Option<chrono::DateTime<chrono::Utc>>>(2)
                    .is_none() =>
        {
            Err(StorageError::TodoVersionConflict {
                todo_id: id,
                expected_version,
                actual_version: row.get(0),
            })
        }
        _ => Err(StorageError::TodoNotFound { todo_id: id }),
    }
}

async fn todo_lifecycle_conflict(
    transaction: &tokio_postgres::Transaction<'_>,
    id: TodoId,
    expected_version: i64,
    restoring: bool,
) -> Result<Todo, StorageError> {
    let state = transaction
        .query_opt(
            "SELECT version, trashed_at, deleted_at FROM todos WHERE id = $1",
            &[&id.as_uuid()],
        )
        .await
        .map_err(StorageError::Query)?;
    match state {
        None => Err(StorageError::TodoNotFound { todo_id: id }),
        Some(row)
            if !restoring
                && row
                    .get::<_, Option<chrono::DateTime<chrono::Utc>>>(2)
                    .is_some() =>
        {
            Err(StorageError::TodoNotFound { todo_id: id })
        }
        Some(row)
            if restoring
                && row
                    .get::<_, Option<chrono::DateTime<chrono::Utc>>>(1)
                    .is_none()
                && row
                    .get::<_, Option<chrono::DateTime<chrono::Utc>>>(2)
                    .is_none() =>
        {
            Err(StorageError::TodoNotFound { todo_id: id })
        }
        Some(row) => Err(StorageError::TodoVersionConflict {
            todo_id: id,
            expected_version,
            actual_version: row.get(0),
        }),
    }
}

impl crate::application::AsyncTodoRepository for PostgresTodoRepository {
    type Error = StorageError;
    fn save_todo<'a>(
        &'a self,
        todo: &'a Todo,
    ) -> crate::application::RepositoryFuture<'a, (), Self::Error> {
        Box::pin(async move { Self::save_todo(self, todo).await })
    }
    fn find_todo(
        &self,
        id: TodoId,
    ) -> crate::application::RepositoryFuture<'_, Option<Todo>, Self::Error> {
        Box::pin(async move { Self::find_todo(self, id).await })
    }
    fn list_todos(&self) -> crate::application::RepositoryFuture<'_, Vec<Todo>, Self::Error> {
        Box::pin(async move { Self::list_todos(self).await })
    }
    fn complete_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> crate::application::RepositoryFuture<'_, Todo, Self::Error> {
        Box::pin(async move { Self::complete_todo(self, id, expected_version).await })
    }
    fn trash_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> crate::application::RepositoryFuture<'_, Todo, Self::Error> {
        Box::pin(async move { Self::trash_todo(self, id, expected_version).await })
    }
    fn restore_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> crate::application::RepositoryFuture<'_, Todo, Self::Error> {
        Box::pin(async move { Self::restore_todo(self, id, expected_version).await })
    }
    fn purge_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> crate::application::RepositoryFuture<'_, TodoId, Self::Error> {
        Box::pin(async move { Self::purge_todo(self, id, expected_version).await })
    }
    fn edit_todo(
        &self,
        id: TodoId,
        expected_version: i64,
        edit: TodoEdit,
    ) -> crate::application::RepositoryFuture<'_, Todo, Self::Error> {
        Box::pin(async move { Self::edit_todo(self, id, expected_version, edit).await })
    }
    fn due_reminders(
        &self,
        at: DateTime<Utc>,
    ) -> crate::application::RepositoryFuture<'_, Vec<crate::application::Reminder>, Self::Error>
    {
        Box::pin(async move { Self::due_reminders(self, at).await })
    }
    fn scan_reminders(
        &self,
        at: DateTime<Utc>,
        dry_run: bool,
    ) -> crate::application::RepositoryFuture<'_, Vec<ReminderDelivery>, Self::Error> {
        Box::pin(async move { Self::scan_reminders(self, at, dry_run).await })
    }
}

impl crate::application::AsyncTagRepository for PostgresTagRepository {
    type Error = StorageError;
    fn save_tag<'a>(
        &'a self,
        tag: &'a Tag,
    ) -> crate::application::RepositoryFuture<'a, (), Self::Error> {
        Box::pin(async move { Self::save_tag(self, tag).await })
    }
    fn list_tags(&self) -> crate::application::RepositoryFuture<'_, Vec<Tag>, Self::Error> {
        Box::pin(async move { Self::list_tags(self).await })
    }
}

impl crate::application::AsyncProjectRepository for PostgresProjectRepository {
    type Error = StorageError;

    fn save_project<'a>(
        &'a self,
        project: &'a Project,
    ) -> crate::application::RepositoryFuture<'a, (), Self::Error> {
        Box::pin(async move { Self::save_project(self, project).await })
    }

    fn find_project(
        &self,
        id: ProjectId,
    ) -> crate::application::RepositoryFuture<'_, Option<Project>, Self::Error> {
        Box::pin(async move { Self::find_project(self, id).await })
    }

    fn list_projects(&self) -> crate::application::RepositoryFuture<'_, Vec<Project>, Self::Error> {
        Box::pin(async move { Self::list_projects(self).await })
    }
}

impl crate::application::AsyncCalendarEventRepository for PostgresCalendarEventRepository {
    type Error = StorageError;

    fn save_calendar<'a>(
        &'a self,
        calendar: &'a Calendar,
    ) -> crate::application::RepositoryFuture<'a, (), Self::Error> {
        Box::pin(async move { Self::save_calendar(self, calendar).await })
    }

    fn save_event<'a>(
        &'a self,
        event: &'a Event,
    ) -> crate::application::RepositoryFuture<'a, (), Self::Error> {
        Box::pin(async move { Self::save_event(self, event).await })
    }

    fn list_calendars(
        &self,
    ) -> crate::application::RepositoryFuture<'_, Vec<Calendar>, Self::Error> {
        Box::pin(async move { Self::list_calendars(self).await })
    }

    fn find_event(
        &self,
        id: EventId,
    ) -> crate::application::RepositoryFuture<'_, Option<Event>, Self::Error> {
        Box::pin(async move { Self::find_event(self, id).await })
    }

    fn list_events(
        &self,
        calendar_id: Option<CalendarId>,
    ) -> crate::application::RepositoryFuture<'_, Vec<Event>, Self::Error> {
        Box::pin(async move { Self::list_events(self, calendar_id).await })
    }

    fn cancel_event(
        &self,
        id: EventId,
        expected_version: i64,
    ) -> crate::application::RepositoryFuture<'_, Event, Self::Error> {
        Box::pin(async move { Self::cancel_event(self, id, expected_version).await })
    }

    fn restore_event(
        &self,
        id: EventId,
        expected_version: i64,
    ) -> crate::application::RepositoryFuture<'_, Event, Self::Error> {
        Box::pin(async move { Self::restore_event(self, id, expected_version).await })
    }

    fn edit_event<'a>(
        &'a self,
        id: EventId,
        expected_version: i64,
        edit: &'a EventEdit,
    ) -> crate::application::RepositoryFuture<'a, Event, Self::Error> {
        Box::pin(async move { Self::edit_event(self, id, expected_version, edit).await })
    }

    fn day_agenda(
        &self,
        date: NaiveDate,
        timezone: &str,
        starts_at: DateTime<chrono::FixedOffset>,
        ends_at: DateTime<chrono::FixedOffset>,
    ) -> crate::application::RepositoryFuture<'_, Vec<Event>, Self::Error> {
        let timezone = timezone.to_owned();
        Box::pin(async move { Self::day_agenda(self, date, &timezone, starts_at, ends_at).await })
    }
}

impl AsyncAgendaRepository for (PostgresCalendarEventRepository, PostgresTodoRepository) {
    type Error = StorageError;

    fn agenda_events(
        &self,
        include_trashed: bool,
    ) -> crate::application::RepositoryFuture<'_, Vec<Event>, Self::Error> {
        Box::pin(async move { self.0.list_events_with_trashed(None, include_trashed).await })
    }

    fn agenda_todos(
        &self,
        include_trashed: bool,
    ) -> crate::application::RepositoryFuture<'_, Vec<Todo>, Self::Error> {
        Box::pin(async move { self.1.list_todos_with_trashed(include_trashed).await })
    }
}

/// Export all todo-related state in deterministic order.
pub async fn export_todos(settings: &ConnectionSettings) -> Result<TodoExport, StorageError> {
    let (client, _) = connect(settings).await?;
    let projects = client.query("SELECT id, name, normalized_name, archived_at, version, created_at, updated_at FROM projects ORDER BY normalized_name, id", &[]).await.map_err(StorageError::Query)?.iter().map(project_from_row).collect::<Result<Vec<_>, _>>()?;
    let tags = client.query("SELECT id, name, normalized_name, created_at, updated_at FROM tags ORDER BY normalized_name, id", &[]).await.map_err(StorageError::Query)?.iter().map(tag_from_row).collect::<Result<Vec<_>, _>>()?;
    let todos = client
        .query(&format!("{TODO_SELECT} {TODO_ORDER}"), &[])
        .await
        .map_err(StorageError::Query)?
        .iter()
        .map(todo_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TodoExport {
        schema_version: 1,
        projects,
        tags,
        todos,
    })
}

/// Import a fully validated document atomically. No vault or other store is touched.
pub async fn import_todos(
    settings: &ConnectionSettings,
    payload: &TodoExport,
) -> Result<usize, StorageError> {
    payload.validate()?;
    let (mut client, _) = connect(settings).await?;
    let tx = client.transaction().await.map_err(StorageError::Query)?;
    for project in &payload.projects {
        if tx.query_opt("SELECT 1 FROM projects WHERE id = $1 OR (archived_at IS NULL AND normalized_name = $2)", &[&project.id.as_uuid(), &project.normalized_name]).await.map_err(StorageError::Query)?.is_some() { return Err(StorageError::ImportConflict { kind: "project", id: project.id.to_string() }); }
    }
    for tag in &payload.tags {
        if tx
            .query_opt(
                "SELECT 1 FROM tags WHERE id = $1 OR normalized_name = $2",
                &[&tag.id.as_uuid(), &tag.normalized_name],
            )
            .await
            .map_err(StorageError::Query)?
            .is_some()
        {
            return Err(StorageError::ImportConflict {
                kind: "tag",
                id: tag.id.to_string(),
            });
        }
    }
    for todo in &payload.todos {
        if tx
            .query_opt("SELECT 1 FROM todos WHERE id = $1", &[&todo.id.as_uuid()])
            .await
            .map_err(StorageError::Query)?
            .is_some()
        {
            return Err(StorageError::ImportConflict {
                kind: "todo",
                id: todo.id.to_string(),
            });
        }
    }
    for project in &payload.projects {
        tx.execute("INSERT INTO projects (id, name, normalized_name, archived_at, version, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7)", &[&project.id.as_uuid(), &project.name, &project.normalized_name, &project.archived_at, &project.version, &project.created_at, &project.updated_at]).await.map_err(StorageError::Query)?;
    }
    for tag in &payload.tags {
        tx.execute("INSERT INTO tags (id, name, normalized_name, created_at, updated_at) VALUES ($1,$2,$3,$4,$5)", &[&tag.id.as_uuid(), &tag.name, &tag.normalized_name, &tag.created_at, &tag.updated_at]).await.map_err(StorageError::Query)?;
    }
    for todo in &payload.todos {
        let (due_date, due_at, timezone) = match &todo.due {
            Some(TodoDue::Date { date, timezone }) => (Some(*date), None, Some(timezone.as_str())),
            Some(TodoDue::Timed { at, timezone }) => {
                (None, Some(at.with_timezone(&Utc)), Some(timezone.as_str()))
            }
            None => (None, None, None),
        };
        let recurrence = todo
            .recurrence
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| StorageError::ImportInvalid {
                reason: error.to_string(),
            })?;
        tx.execute("INSERT INTO todos (id,parent_id,title,notes,due_date,due_at,timezone,priority,project_id,completed_at,trashed_at,version,created_at,updated_at,recurrence_rule) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)", &[&todo.id.as_uuid(), &todo.parent_id.map(TodoId::as_uuid), &todo.title, &todo.notes, &due_date, &due_at, &timezone, &todo.priority.to_string(), &todo.project_id.map(ProjectId::as_uuid), &todo.completed_at, &todo.trashed_at, &todo.version, &todo.created_at, &todo.updated_at, &recurrence]).await.map_err(StorageError::Query)?;
    }
    for todo in &payload.todos {
        for tag in &todo.tag_ids {
            tx.execute(
                "INSERT INTO todo_tags (todo_id,tag_id) VALUES ($1,$2)",
                &[&todo.id.as_uuid(), &tag.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?;
        }
        for dependency in &todo.dependency_ids {
            tx.execute(
                "INSERT INTO todo_dependencies (dependent_id,prerequisite_id) VALUES ($1,$2)",
                &[&todo.id.as_uuid(), &dependency.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?;
        }
        for reminder in &todo.reminders {
            let minutes = i32::try_from(reminder.minutes_before).map_err(|_| {
                StorageError::ImportInvalid {
                    reason: "reminder offset overflow".to_owned(),
                }
            })?;
            tx.execute(
                "INSERT INTO todo_reminders (todo_id,minutes_before,repeatable) VALUES ($1,$2,$3)",
                &[&todo.id.as_uuid(), &minutes, &reminder.repeatable],
            )
            .await
            .map_err(StorageError::Query)?;
        }
    }
    tx.commit().await.map_err(StorageError::Query)?;
    Ok(payload.projects.len() + payload.tags.len() + payload.todos.len())
}

fn event_status(status: EventStatus) -> &'static str {
    match status {
        EventStatus::Tentative => "tentative",
        EventStatus::Confirmed => "confirmed",
        EventStatus::Cancelled => "cancelled",
    }
}

fn parse_event_status(value: Option<String>) -> Result<Option<EventStatus>, StorageError> {
    value
        .map(|status| match status.as_str() {
            "tentative" => Ok(EventStatus::Tentative),
            "confirmed" => Ok(EventStatus::Confirmed),
            "cancelled" => Ok(EventStatus::Cancelled),
            _ => Err(StorageError::InvalidStoredData(format!(
                "unknown event status '{status}'"
            ))),
        })
        .transpose()
}

#[derive(Debug, Default, Deserialize)]
struct ExtensionProperties {
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    alarms: Vec<crate::domain::Alarm>,
    organizer: Option<String>,
    #[serde(default)]
    attendees: Vec<String>,
}

fn tag_from_row(row: &Row) -> Result<Tag, StorageError> {
    Tag {
        id: TagId::from_uuid(row.get(0)),
        name: row.get(1),
        normalized_name: row.get(2),
        created_at: row.get(3),
        updated_at: row.get(4),
    }
    .rehydrate()
    .map_err(|e| StorageError::InvalidStoredData(e.to_string()))
}

fn project_from_row(row: &Row) -> Result<Project, StorageError> {
    let id = row.get::<_, Uuid>(0).to_string().parse().map_err(|error| {
        StorageError::InvalidStoredData(format!("invalid project identifier: {error}"))
    })?;
    Project {
        id,
        name: row.get(1),
        normalized_name: row.get(2),
        archived_at: row.get(3),
        version: row.get(4),
        created_at: row.get(5),
        updated_at: row.get(6),
    }
    .rehydrate()
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
}

fn recurrence_error(error: &TodoError) -> StorageError {
    StorageError::InvalidRecurrence {
        reason: error.to_string(),
    }
}

#[allow(clippy::too_many_lines)]
fn todo_from_row(row: &Row) -> Result<Todo, StorageError> {
    let parse_id = |value: Uuid, kind: &'static str| {
        value.to_string().parse().map_err(|error| {
            StorageError::InvalidStoredData(format!("invalid {kind} identifier: {error}"))
        })
    };
    let id = parse_id(row.get(0), "todo")?;
    let parent_id = row
        .get::<_, Option<Uuid>>(1)
        .map(|value| parse_id(value, "todo"))
        .transpose()?;
    let project_id = row
        .get::<_, Option<Uuid>>(8)
        .map(|value| {
            value.to_string().parse::<ProjectId>().map_err(|error| {
                StorageError::InvalidStoredData(format!("invalid project identifier: {error}"))
            })
        })
        .transpose()?;
    let timezone = row.get::<_, Option<String>>(6);
    let due_date = row.get::<_, Option<NaiveDate>>(4);
    let due_at = row.get::<_, Option<DateTime<Utc>>>(5);
    let due = match (due_date, due_at, timezone) {
        (None, None, None) => None,
        (Some(date), None, Some(zone)) => Some(TodoDue::date(date, zone)),
        (None, Some(at), Some(zone)) => {
            let parsed_zone = zone.parse::<Tz>().map_err(|_| {
                StorageError::InvalidStoredData(format!("invalid IANA timezone '{zone}'"))
            })?;
            Some(TodoDue::timed(
                at.with_timezone(&parsed_zone).fixed_offset(),
                zone,
            ))
        }
        _ => {
            return Err(StorageError::InvalidStoredData(
                "todo has mixed or incomplete due columns".to_owned(),
            ));
        }
    }
    .transpose()
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    let recurrence = if row.len() > 16 {
        row.get::<_, Option<serde_json::Value>>(14)
            .map(|value| {
                serde_json::from_value::<RecurrenceRule>(value).map_err(|error| {
                    StorageError::InvalidStoredData(format!("invalid recurrence rule: {error}"))
                })
            })
            .transpose()?
    } else {
        None
    };
    let (tag_index, dependency_index) = if row.len() > 17 {
        (16, 17)
    } else if row.len() > 16 {
        (15, 16)
    } else {
        (14, 15)
    };
    let tag_ids = if row.len() > tag_index {
        row.get::<_, Vec<Uuid>>(tag_index)
            .into_iter()
            .map(TagId::from_uuid)
            .collect()
    } else {
        Vec::new()
    };
    let dependency_ids = if row.len() > dependency_index {
        row.get::<_, Vec<Uuid>>(dependency_index)
            .into_iter()
            .map(TodoId::from_uuid)
            .collect()
    } else {
        Vec::new()
    };
    let reminders = if row.len() > 17 {
        serde_json::from_value::<Vec<TodoReminder>>(row.get(15)).map_err(|error| {
            StorageError::InvalidStoredData(format!("invalid reminders: {error}"))
        })?
    } else {
        Vec::new()
    };
    let priority = row
        .get::<_, String>(7)
        .parse::<Priority>()
        .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    Todo {
        id,
        title: row.get(2),
        due,
        recurrence,
        reminders,
        priority,
        project_id,
        tag_ids,
        dependency_ids,
        notes: row.get(3),
        parent_id,
        completed_at: row.get(9),
        trashed_at: row.get(10),
        version: row.get(11),
        created_at: row.get(12),
        updated_at: row.get(13),
    }
    .rehydrate()
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
}

fn calendar_from_row(row: &Row) -> Result<Calendar, StorageError> {
    let id = row.get::<_, Uuid>(0).to_string().parse().map_err(|error| {
        StorageError::InvalidStoredData(format!("invalid calendar identifier: {error}"))
    })?;
    Calendar::rehydrate(
        id,
        row.get::<_, String>(1),
        row.get(2),
        row.get(3),
        row.get(4),
        row.get(5),
        row.get(6),
    )
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
}

fn event_from_row(row: &Row) -> Result<Event, StorageError> {
    let id = row.get::<_, Uuid>(0).to_string().parse().map_err(|error| {
        StorageError::InvalidStoredData(format!("invalid event identifier: {error}"))
    })?;
    let calendar_id = row.get::<_, Uuid>(1).to_string().parse().map_err(|error| {
        StorageError::InvalidStoredData(format!("invalid calendar identifier: {error}"))
    })?;
    let timezone = row.get::<_, Option<String>>(9);
    let starts_at = row.get::<_, Option<DateTime<Utc>>>(10);
    let ends_at = row.get::<_, Option<DateTime<Utc>>>(11);
    let all_day_start = row.get::<_, Option<NaiveDate>>(12);
    let all_day_end = row.get::<_, Option<NaiveDate>>(13);
    let time = match (timezone, starts_at, ends_at, all_day_start, all_day_end) {
        (Some(timezone), Some(start), Some(end), None, None) => {
            let zone = timezone.parse::<Tz>().map_err(|_| {
                StorageError::InvalidStoredData(format!("invalid IANA timezone '{timezone}'"))
            })?;
            EventTime::timed(
                start.with_timezone(&zone).fixed_offset(),
                end.with_timezone(&zone).fixed_offset(),
                timezone,
            )
        }
        (None, None, None, Some(start), Some(end)) => EventTime::all_day(start, end),
        _ => {
            return Err(StorageError::InvalidStoredData(
                "event has mixed or incomplete temporal columns".to_owned(),
            ));
        }
    }
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    let extension: ExtensionProperties = serde_json::from_value(row.get(15))
        .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    let metadata = EventMetadata {
        description: row.get(4),
        location: row.get(5),
        url: row.get(6),
        status: parse_event_status(row.get(7))?,
        busy: row.get(8),
        categories: extension.categories,
        recurrence_rule: row.get(14),
        alarms: extension.alarms,
        organizer: extension.organizer,
        attendees: extension.attendees,
    };
    Event::rehydrate(
        id,
        calendar_id,
        RfcUid::new(row.get::<_, String>(2))
            .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?,
        row.get::<_, String>(3),
        time,
        metadata,
        row.get(16),
        row.get(17),
        row.get(18),
        row.get(19),
        row.get(20),
    )
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio_postgres::config::Host;

    use super::{TODO_ORDER, TODO_SELECT, postgres_config};
    use crate::application::TodoQueryProjection;
    use crate::config::{ConfigSource, ConnectionSettings};
    use crate::domain::todo::Todo;

    #[test]
    fn libpq_local_uri_uses_the_postgresql_socket() {
        let settings = ConnectionSettings::Url {
            url: "postgresql:///mg_calr_test".to_owned(),
            source: ConfigSource::Environment,
        };

        let config = postgres_config(&settings).expect("valid local URI");

        assert_eq!(config.get_dbname(), Some("mg_calr_test"));
        assert_eq!(
            config.get_hosts(),
            &[Host::Unix(Path::new("/run/postgresql").to_path_buf())]
        );
    }

    #[test]
    fn todo_sql_contract_is_parameterized_and_stably_ordered() {
        assert!(!TODO_SELECT.contains("{title}"));
        assert!(TODO_SELECT.contains("COALESCE(t.trashed_at, t.deleted_at)"));
        assert!(TODO_ORDER.contains("lower(t.title)"));
        assert!(TODO_ORDER.contains("t.id"));
    }

    #[test]
    fn todo_query_projection_serializes_core_fields_without_tags() {
        let todo = Todo::new("Stable output").expect("valid todo");
        let value = serde_json::to_value(TodoQueryProjection::from(todo)).expect("serializable");
        assert_eq!(value["title"], "Stable output");
        assert!(value["tag_ids"].is_array());
        assert!(value.get("version").is_some());
    }
}
