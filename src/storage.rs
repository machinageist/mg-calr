#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls, Row};
use uuid::Uuid;

use crate::application::TodoEdit;
use crate::config::ConnectionSettings;
use crate::domain::{
    Calendar, CalendarId, Event, EventId, EventMetadata, EventStatus, EventTime, RfcUid,
    todo::{Priority, Project, ProjectId, Tag, TagId, Todo, TodoDue, TodoId},
};

pub const FOUNDATION_MIGRATION: &str = include_str!("../migrations/0001_foundation.sql");
pub const TODO_CORE_MIGRATION: &str = include_str!("../migrations/0002_todo_core.sql");

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
    #[error("todo {todo_id} has child todos and cannot be purged")]
    TodoHasChildren { todo_id: TodoId },
    #[error("todo {todo_id} is not trashed")]
    TodoNotTrashed { todo_id: TodoId },
    #[error("project {project_id} does not exist or is archived")]
    ProjectNotFound { project_id: ProjectId },
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
    #[error("stored calendar/event data is invalid: {0}")]
    InvalidStoredData(String),
    #[error("migration version {version} is recorded as '{actual}', expected '{expected}'")]
    MigrationDrift {
        version: i64,
        actual: String,
        expected: &'static str,
    },
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
    e.deleted_at, e.remote_tombstoned_at FROM events e JOIN calendars c ON c.id = e.calendar_id";
const EVENT_ORDER: &str = "ORDER BY CASE WHEN e.all_day_start IS NOT NULL THEN 0 ELSE 1 END, \
    COALESCE(e.all_day_start, (e.starts_at AT TIME ZONE 'UTC')::date), \
    e.starts_at NULLS FIRST, lower(e.title), e.id";

const TODO_SELECT: &str = "SELECT t.id, t.parent_id, t.title, t.notes, t.due_date, t.due_at, \
    t.timezone, t.priority, t.project_id, t.completed_at, COALESCE(t.trashed_at, t.deleted_at) AS trashed_at, t.version, \
    t.created_at, t.updated_at, COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = t.id ORDER BY tt.tag_id), ARRAY[]::uuid[]) AS tag_ids FROM todos t";
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
                "INSERT INTO events (id, calendar_id, rfc_uid, title, description, location, url, status, busy, timezone, starts_at, ends_at, all_day_start, all_day_end, recurrence_rule, extension_properties, created_at, updated_at, deleted_at, remote_tombstoned_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)",
                &[
                    &event.id.as_uuid(), &event.calendar_id.as_uuid(), &event.rfc_uid.as_str(),
                    &event.title, &event.metadata.description, &event.metadata.location,
                    &event.metadata.url, &status, &event.metadata.busy, &timezone, &starts_at,
                    &ends_at, &all_day_start, &all_day_end, &event.metadata.recurrence_rule,
                    &extension_properties, &event.created_at, &event.updated_at,
                    &event.deleted_at, &event.remote_tombstoned_at,
                ],
            )
            .await
            .map_err(StorageError::Query)?;
        transaction.commit().await.map_err(StorageError::Query)
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
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
        let (due_date, due_at, timezone) = match &todo.due {
            Some(TodoDue::Date { date, timezone }) => (Some(*date), None, Some(timezone.as_str())),
            Some(TodoDue::Timed { at, timezone }) => {
                (None, Some(at.with_timezone(&Utc)), Some(timezone.as_str()))
            }
            None => (None, None, None),
        };
        transaction.execute(
            "INSERT INTO todos (id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, trashed_at, version, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
            &[&todo.id.as_uuid(), &todo.parent_id.map(TodoId::as_uuid), &todo.title,
              &todo.notes, &due_date, &due_at, &timezone, &todo.priority.to_string(),
              &todo.project_id.map(ProjectId::as_uuid), &todo.completed_at, &todo.trashed_at,
              &todo.version, &todo.created_at, &todo.updated_at],
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
        let (client, _connection_task) = connect(&self.settings).await?;
        let rows = client
            .query(&format!("{TODO_SELECT} {TODO_ORDER}"), &[])
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
                "UPDATE todos SET completed_at = CURRENT_TIMESTAMP, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND trashed_at IS NULL AND deleted_at IS NULL AND completed_at IS NULL AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[])",
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
                "UPDATE todos SET trashed_at = CURRENT_TIMESTAMP, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND trashed_at IS NULL AND deleted_at IS NULL AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[])",
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
                "UPDATE todos SET trashed_at = NULL, deleted_at = NULL, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND (trashed_at IS NOT NULL OR deleted_at IS NOT NULL) AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[])",
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
        let (mut client, _connection_task) = connect(&self.settings).await?;
        let transaction = client.transaction().await.map_err(StorageError::Query)?;
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
        let tag_ids = edit.tag_ids.clone().map(|ids| {
            let mut ids = ids.into_iter().map(TagId::as_uuid).collect::<Vec<_>>();
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
        let row = transaction
            .query_opt(
                "UPDATE todos SET title = COALESCE($3, title), priority = COALESCE($4, priority), due_date = CASE WHEN $5 THEN $6 ELSE due_date END, due_at = CASE WHEN $5 THEN $7 ELSE due_at END, timezone = CASE WHEN $5 THEN $8 ELSE timezone END, notes = CASE WHEN $9 THEN $10 ELSE notes END, project_id = CASE WHEN $11 THEN $12 ELSE project_id END, version = version + 1, updated_at = CURRENT_TIMESTAMP WHERE id = $1 AND trashed_at IS NULL AND deleted_at IS NULL AND version = $2 RETURNING id, parent_id, title, notes, due_date, due_at, timezone, priority, project_id, completed_at, COALESCE(trashed_at, deleted_at) AS trashed_at, version, created_at, updated_at, COALESCE(ARRAY(SELECT tt.tag_id FROM todo_tags tt WHERE tt.todo_id = todos.id ORDER BY tt.tag_id), ARRAY[]::uuid[])",
                &[&id.as_uuid(), &expected_version, &title, &priority, &due_changed, &due_date, &due_at, &timezone, &notes_changed, &notes, &project_changed, &project_id],
            )
            .await
            .map_err(StorageError::Query)?;
        let Some(row) = row else {
            return todo_edit_conflict(&transaction, id, expected_version).await;
        };
        let mut todo = todo_from_row(&row)?;
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
        transaction.commit().await.map_err(StorageError::Query)?;
        Ok(todo)
    }
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
    let tag_ids = if row.len() > 14 {
        row.get::<_, Vec<Uuid>>(14)
            .into_iter()
            .map(TagId::from_uuid)
            .collect()
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
        priority,
        project_id,
        tag_ids,
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
