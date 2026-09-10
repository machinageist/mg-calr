#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]
use std::{collections::HashSet, path::PathBuf};

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls, Row};
use uuid::Uuid;

use crate::application::{
    AgendaTodoSnapshot, AsyncAgendaRepository, EventEdit, EventLifecycleError,
    EventLifecycleErrorMapping,
};
use crate::config::ConnectionSettings;
use crate::domain::{
    Calendar, CalendarId, Event, EventId, EventMetadata, EventRecurrence, EventStatus, EventTime,
    RfcUid,
    todo::{Project, ProjectId, Tag, TagId, TodoId},
};

pub const FOUNDATION_MIGRATION: &str = include_str!("../migrations/0001_foundation.sql");
pub const TODO_CORE_MIGRATION: &str = include_str!("../migrations/0002_todo_core.sql");
pub const TODO_RECURRENCE_MIGRATION: &str = include_str!("../migrations/0003_todo_recurrence.sql");
pub const TODO_REMINDERS_MIGRATION: &str = include_str!("../migrations/0004_todo_reminders.sql");
pub const EVENT_LIFECYCLE_MIGRATION: &str = include_str!("../migrations/0005_event_lifecycle.sql");
pub const REPAIR_TODO_RECURRENCE_MIGRATION: &str =
    include_str!("../migrations/0006_repair_todo_recurrence.sql");
pub const REMINDER_DELIVERY_LEDGER_MIGRATION: &str =
    include_str!("../migrations/0007_reminder_delivery_ledger.sql");
pub const EVENT_RECURRENCE_MIGRATION: &str =
    include_str!("../migrations/0008_event_recurrence.sql");
pub const REMOVE_LEGACY_TODO_AUTHORITY_MIGRATION: &str =
    include_str!("../migrations/0009_remove_legacy_todo_authority.sql");

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
    Migration {
        version: 6,
        name: "repair_todo_recurrence",
        sql: REPAIR_TODO_RECURRENCE_MIGRATION,
    },
    Migration {
        version: 7,
        name: "reminder_delivery_ledger",
        sql: REMINDER_DELIVERY_LEDGER_MIGRATION,
    },
    Migration {
        version: 8,
        name: "event_recurrence",
        sql: EVENT_RECURRENCE_MIGRATION,
    },
    Migration {
        version: 9,
        name: "remove_legacy_todo_authority",
        sql: REMOVE_LEGACY_TODO_AUTHORITY_MIGRATION,
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

/// Versioned, lossless interchange document for local calendars and events.
/// Versioned, lossless interchange document for local calendars and events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventExport {
    pub schema_version: u8,
    pub calendars: Vec<Calendar>,
    pub events: Vec<Event>,
}

impl EventExport {
    /// Parse and validate the complete document without opening a database.
    pub fn parse(json: &str) -> Result<Self, StorageError> {
        let mut payload: Self =
            serde_json::from_str(json).map_err(|error| StorageError::ImportInvalid {
                reason: error.to_string(),
            })?;
        payload.validate()?;
        payload
            .calendars
            .sort_by_key(|calendar| (calendar.name.to_lowercase(), calendar.id.as_uuid()));
        payload
            .events
            .sort_by_key(|event| (event.calendar_id.as_uuid(), event.id.as_uuid()));
        Ok(payload)
    }

    fn validate(&self) -> Result<(), StorageError> {
        if self.schema_version != 1 {
            return Err(StorageError::ImportInvalid {
                reason: format!("unsupported schema_version {}", self.schema_version),
            });
        }
        let mut calendar_ids = HashSet::new();
        let mut default_count = 0;
        for calendar in &self.calendars {
            Calendar::rehydrate(
                calendar.id,
                calendar.name.clone(),
                calendar.color.clone(),
                calendar.is_default,
                calendar.created_at,
                calendar.updated_at,
                calendar.deleted_at,
            )
            .map_err(|error| StorageError::ImportInvalid {
                reason: error.to_string(),
            })?;
            if calendar.deleted_at.is_none() && calendar.is_default {
                default_count += 1;
            }
            if !calendar_ids.insert(calendar.id.as_uuid()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("duplicate calendar {}", calendar.id),
                });
            }
        }
        if default_count > 1 {
            return Err(StorageError::ImportInvalid {
                reason: "multiple live default calendars".to_owned(),
            });
        }
        let mut event_ids = HashSet::new();
        let mut rfc_uids = HashSet::new();
        for event in &self.events {
            if !calendar_ids.contains(&event.calendar_id.as_uuid()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("event {} references missing calendar", event.id),
                });
            }
            if !event_ids.insert(event.id.as_uuid()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("duplicate event {}", event.id),
                });
            }
            if !rfc_uids.insert(event.rfc_uid.as_str().to_owned()) {
                return Err(StorageError::ImportInvalid {
                    reason: format!("duplicate RFC UID for event {}", event.id),
                });
            }
            match &event.time {
                EventTime::Timed {
                    start,
                    end,
                    timezone,
                } => EventTime::timed(*start, *end, timezone.clone()),
                EventTime::AllDay {
                    start,
                    end_exclusive,
                } => EventTime::all_day(*start, *end_exclusive),
            }
            .map_err(|error| StorageError::ImportInvalid {
                reason: error.to_string(),
            })?;
            Event::rehydrate(
                event.id,
                event.calendar_id,
                event.rfc_uid.clone(),
                event.title.clone(),
                event.time.clone(),
                event.metadata.clone(),
                event.created_at,
                event.updated_at,
                event.deleted_at,
                event.remote_tombstoned_at,
                event.version,
            )
            .map_err(|error| StorageError::ImportInvalid {
                reason: error.to_string(),
            })?;
        }
        Ok(())
    }
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

fn migration_checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

// Takes any client so the caller can pass a transaction. Concurrent
// CREATE TABLE IF NOT EXISTS is not race-safe in PostgreSQL — two sessions can
// both find the table absent and both attempt it, and the loser gets 42P07 —
// so this must run under the advisory lock, never on a bare connection.
async fn ensure_migration_table<C: tokio_postgres::GenericClient + Sync>(
    client: &C,
) -> Result<(), StorageError> {
    client
        .batch_execute(
            "CREATE TABLE IF NOT EXISTS mg_calr_schema_migrations (\
             version bigint PRIMARY KEY, \
             name text NOT NULL, \
             checksum text, \
             applied_at timestamptz NOT NULL DEFAULT CURRENT_TIMESTAMP); \
             ALTER TABLE mg_calr_schema_migrations ADD COLUMN IF NOT EXISTS checksum text",
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
            "SELECT version, name, to_jsonb(migration)->>'checksum' \
             FROM mg_calr_schema_migrations migration ORDER BY version",
            &[],
        )
        .await
        .map_err(StorageError::Query)?;
    let applied = rows
        .into_iter()
        .map(|row| {
            (
                row.get::<_, i64>(0),
                (row.get::<_, String>(1), row.get::<_, Option<String>>(2)),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    if let Some(max_version) = applied.keys().max().copied() {
        verify_live_schema(&client, max_version).await?;
    }

    MIGRATIONS
        .iter()
        .map(|migration| {
            if let Some((actual_name, actual_checksum)) = applied.get(&migration.version) {
                let expected_checksum = migration_checksum(migration.sql);
                if actual_name != migration.name
                    || actual_checksum
                        .as_ref()
                        .is_some_and(|actual| actual != &expected_checksum)
                {
                    return Err(StorageError::MigrationDrift {
                        version: migration.version,
                        actual: format!(
                            "{actual_name}@{}",
                            actual_checksum.as_deref().unwrap_or("unchecksummed")
                        ),
                        expected: migration.name,
                    });
                }
            }
            Ok(MigrationState {
                version: migration.version,
                name: migration.name.to_owned(),
                applied: applied.contains_key(&migration.version),
            })
        })
        .collect()
}

async fn verify_live_schema(client: &Client, max_version: i64) -> Result<(), StorageError> {
    const TABLES: &[(i64, &str)] = &[
        (1, "calendars"),
        (1, "events"),
        (1, "todos"),
        (1, "todo_dependencies"),
        (1, "reminders"),
        (1, "reminder_deliveries"),
        (1, "audit_log"),
        (2, "projects"),
        (2, "tags"),
        (2, "todo_tags"),
        (4, "todo_reminders"),
        (7, "reminder_digests"),
        (7, "reminder_dnd_windows"),
        (7, "reminder_scanner_runs"),
    ];
    for (version, table) in TABLES {
        if *version > max_version {
            continue;
        }
        if max_version >= 9
            && matches!(
                *table,
                "todos" | "todo_dependencies" | "todo_tags" | "todo_reminders"
            )
        {
            continue;
        }
        let exists = client
            .query_one(
                "SELECT to_regclass(current_schema() || '.' || $1) IS NOT NULL",
                &[table],
            )
            .await
            .map_err(StorageError::Query)?
            .get::<_, bool>(0);
        if !exists {
            return Err(StorageError::MigrationDrift {
                version: max_version,
                actual: format!("missing live table {table}"),
                expected: "embedded migration schema",
            });
        }
    }
    Ok(())
}

/// Apply pending embedded migrations in one advisory-locked transaction.
///
/// # Errors
///
/// Returns an error for invalid configuration, connection/query failures, or
/// migration drift. SQL failures roll back the migration transaction.
pub async fn migrate(settings: &ConnectionSettings) -> Result<Vec<MigrationState>, StorageError> {
    let (mut client, _connection_task) = connect(settings).await?;
    let transaction = client.transaction().await.map_err(StorageError::Query)?;
    transaction
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&6_851_863_988_i64])
        .await
        .map_err(StorageError::Query)?;
    // Inside the lock, so two processes migrating one fresh database serialize
    // rather than racing to create the ledger. mg-remindr already does this.
    ensure_migration_table(&transaction).await?;

    for migration in MIGRATIONS {
        let expected_checksum = migration_checksum(migration.sql);
        let existing = transaction
            .query_opt(
                "SELECT name, checksum FROM mg_calr_schema_migrations WHERE version = $1",
                &[&migration.version],
            )
            .await
            .map_err(StorageError::Query)?;
        if let Some(row) = existing {
            let actual = row.get::<_, String>(0);
            let checksum = row.get::<_, Option<String>>(1);
            if actual != migration.name
                || checksum
                    .as_ref()
                    .is_some_and(|value| value != &expected_checksum)
            {
                return Err(StorageError::MigrationDrift {
                    version: migration.version,
                    actual,
                    expected: migration.name,
                });
            }
            if checksum.is_none() {
                transaction
                    .execute(
                        "UPDATE mg_calr_schema_migrations SET checksum = $2 WHERE version = $1 AND checksum IS NULL",
                        &[&migration.version, &expected_checksum],
                    )
                    .await
                    .map_err(StorageError::Query)?;
            }
            continue;
        }
        // The immutable v1 migration created `todos.recurrence_rule` as text,
        // while immutable v3 used `ADD COLUMN IF NOT EXISTS ... jsonb` before
        // adding jsonb constraints. Prepare that exact historical transition
        // transactionally without rewriting either checksummed migration.
        if migration.version == 3 {
            let recurrence_type = transaction
                .query_opt(
                    "SELECT data_type FROM information_schema.columns WHERE table_schema = current_schema() AND table_name = 'todos' AND column_name = 'recurrence_rule'",
                    &[],
                )
                .await
                .map_err(StorageError::Query)?
                .map(|row| row.get::<_, String>(0));
            if recurrence_type.as_deref() == Some("text") {
                transaction
                    .batch_execute(
                        "ALTER TABLE todos ALTER COLUMN recurrence_rule TYPE jsonb USING CASE WHEN recurrence_rule IS NULL THEN NULL ELSE recurrence_rule::jsonb END",
                    )
                    .await
                    .map_err(StorageError::Query)?;
            }
        }
        transaction
            .batch_execute(migration.sql)
            .await
            .map_err(StorageError::Query)?;
        transaction
            .execute(
                "INSERT INTO mg_calr_schema_migrations (version, name, checksum) VALUES ($1, $2, $3)",
                &[&migration.version, &migration.name, &expected_checksum],
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
        let recurrence = recurrence_value(event.metadata.recurrence_rule.as_ref())?;
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
                    &ends_at, &all_day_start, &all_day_end, &recurrence,
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

/// Read-only agenda boundary that keeps events in PostgreSQL and todos in the
/// validated immutable mg-remindr projection.
pub struct ProjectionAgendaRepository {
    events: PostgresCalendarEventRepository,
    todo_projection: PathBuf,
}

impl ProjectionAgendaRepository {
    pub fn new(events: PostgresCalendarEventRepository, todo_projection: PathBuf) -> Self {
        Self {
            events,
            todo_projection,
        }
    }
}

#[derive(Debug, Error)]
pub enum AgendaRepositoryError {
    #[error("calendar repository operation failed")]
    Calendar(#[source] StorageError),
    #[error(transparent)]
    TodoProjection(#[from] crate::interop::ProjectionError),
}

impl AsyncAgendaRepository for ProjectionAgendaRepository {
    type Error = AgendaRepositoryError;

    fn agenda_events(
        &self,
        include_trashed: bool,
    ) -> crate::application::RepositoryFuture<'_, Vec<Event>, Self::Error> {
        Box::pin(async move {
            self.events
                .list_events_with_trashed(None, include_trashed)
                .await
                .map_err(AgendaRepositoryError::Calendar)
        })
    }

    fn agenda_todos(
        &self,
        include_trashed: bool,
    ) -> crate::application::RepositoryFuture<'_, AgendaTodoSnapshot, Self::Error> {
        Box::pin(async move {
            let projection = crate::interop::TodoProjectionSnapshot::load(&self.todo_projection)?;
            let mut snapshot = projection.agenda_todos()?;
            if !include_trashed {
                snapshot.todos.retain(|todo| todo.trashed_at.is_none());
            }
            Ok(snapshot)
        })
    }
}

/// Export all todo-related state in deterministic order.
async fn export_events_from<C: tokio_postgres::GenericClient + Sync>(
    client: &C,
) -> Result<EventExport, StorageError> {
    let calendars = client
        .query(
            "SELECT id, name, color, is_default, created_at, updated_at, deleted_at FROM calendars ORDER BY lower(name), id",
            &[],
        )
        .await
        .map_err(StorageError::Query)?
        .iter()
        .map(calendar_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    let events = client
        .query(&format!("{EVENT_SELECT} {EVENT_ORDER}"), &[])
        .await
        .map_err(StorageError::Query)?
        .iter()
        .map(event_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventExport {
        schema_version: 1,
        calendars,
        events,
    })
}

pub async fn export_events(settings: &ConnectionSettings) -> Result<EventExport, StorageError> {
    let (client, _) = connect(settings).await?;
    export_events_from(&client).await
}

/// Import a fully validated calendar/event document atomically without overwriting.
/// Write one complete event row inside an open transaction.
async fn insert_event_row(
    tx: &tokio_postgres::Transaction<'_>,
    event: &Event,
) -> Result<(), StorageError> {
    let (timezone, starts_at, ends_at, all_day_start, all_day_end) = match &event.time {
        EventTime::Timed {
            start,
            end,
            timezone,
        } => (
            Some(timezone.as_str()),
            Some(start.with_timezone(&Utc)),
            Some(end.with_timezone(&Utc)),
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
    tx.execute(
        "INSERT INTO events (id,calendar_id,rfc_uid,title,description,location,url,status,busy,timezone,starts_at,ends_at,all_day_start,all_day_end,recurrence_rule,extension_properties,created_at,updated_at,deleted_at,remote_tombstoned_at,version) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)",
        &[&event.id.as_uuid(), &event.calendar_id.as_uuid(), &event.rfc_uid.as_str(), &event.title, &event.metadata.description, &event.metadata.location, &event.metadata.url, &status, &event.metadata.busy, &timezone, &starts_at, &ends_at, &all_day_start, &all_day_end, &recurrence_value(event.metadata.recurrence_rule.as_ref())?, &extension_properties, &event.created_at, &event.updated_at, &event.deleted_at, &event.remote_tombstoned_at, &event.version],
    ).await.map_err(StorageError::Query)?;
    Ok(())
}

/// Add events read from an iCalendar file to one existing calendar, atomically.
///
/// A UID already present is an import conflict: re-importing the same file must
/// not silently double a schedule.
///
/// # Errors
/// Returns an error for a missing calendar, a UID already stored, or a query failure.
pub async fn import_ics_events(
    settings: &ConnectionSettings,
    calendar_id: CalendarId,
    events: &[Event],
) -> Result<usize, StorageError> {
    let (mut client, _) = connect(settings).await?;
    let tx = client.transaction().await.map_err(StorageError::Query)?;
    if tx
        .query_opt(
            "SELECT 1 FROM calendars WHERE id = $1 AND deleted_at IS NULL",
            &[&calendar_id.as_uuid()],
        )
        .await
        .map_err(StorageError::Query)?
        .is_none()
    {
        return Err(StorageError::ImportInvalid {
            reason: format!("calendar {calendar_id} does not exist"),
        });
    }
    for event in events {
        if tx
            .query_opt(
                "SELECT 1 FROM events WHERE rfc_uid = $1",
                &[&event.rfc_uid.as_str()],
            )
            .await
            .map_err(StorageError::Query)?
            .is_some()
        {
            return Err(StorageError::ImportConflict {
                kind: "event",
                id: event.rfc_uid.as_str().to_owned(),
            });
        }
        insert_event_row(&tx, event).await?;
    }
    tx.commit().await.map_err(StorageError::Query)?;
    Ok(events.len())
}

pub async fn import_events(
    settings: &ConnectionSettings,
    payload: &EventExport,
) -> Result<usize, StorageError> {
    payload.validate()?;
    let (mut client, _) = connect(settings).await?;
    let tx = client.transaction().await.map_err(StorageError::Query)?;
    for calendar in &payload.calendars {
        if tx
            .query_opt(
                "SELECT 1 FROM calendars WHERE id = $1",
                &[&calendar.id.as_uuid()],
            )
            .await
            .map_err(StorageError::Query)?
            .is_some()
        {
            return Err(StorageError::ImportConflict {
                kind: "calendar",
                id: calendar.id.to_string(),
            });
        }
        if calendar.is_default
            && calendar.deleted_at.is_none()
            && tx
                .query_opt(
                    "SELECT 1 FROM calendars WHERE is_default AND deleted_at IS NULL",
                    &[],
                )
                .await
                .map_err(StorageError::Query)?
                .is_some()
        {
            return Err(StorageError::ImportConflict {
                kind: "calendar",
                id: calendar.id.to_string(),
            });
        }
    }
    for event in &payload.events {
        if tx
            .query_opt(
                "SELECT 1 FROM events WHERE id = $1 OR rfc_uid = $2",
                &[&event.id.as_uuid(), &event.rfc_uid.as_str()],
            )
            .await
            .map_err(StorageError::Query)?
            .is_some()
        {
            return Err(StorageError::ImportConflict {
                kind: "event",
                id: event.id.to_string(),
            });
        }
    }
    for calendar in &payload.calendars {
        tx.execute(
            "INSERT INTO calendars (id,name,color,is_default,created_at,updated_at,deleted_at) VALUES ($1,$2,$3,$4,$5,$6,$7)",
            &[&calendar.id.as_uuid(), &calendar.name, &calendar.color, &calendar.is_default, &calendar.created_at, &calendar.updated_at, &calendar.deleted_at],
        ).await.map_err(StorageError::Query)?;
    }
    for event in &payload.events {
        insert_event_row(&tx, event).await?;
    }
    tx.commit().await.map_err(StorageError::Query)?;
    Ok(payload.calendars.len() + payload.events.len())
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

/// Encode a validated rule for the jsonb column.
fn recurrence_value(
    rule: Option<&EventRecurrence>,
) -> Result<Option<serde_json::Value>, StorageError> {
    rule.map(|rule| {
        rule.validate()
            .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
        serde_json::to_value(rule)
            .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
    })
    .transpose()
}

/// Read a stored rule back, revalidating rather than trusting the column.
fn recurrence_from_row(
    value: Option<serde_json::Value>,
) -> Result<Option<EventRecurrence>, StorageError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let rule: EventRecurrence = serde_json::from_value(value)
        .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    rule.validate()
        .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    Ok(Some(rule))
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
        recurrence_rule: recurrence_from_row(row.get(14))?,
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

    use super::postgres_config;
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
    fn todo_query_projection_serializes_core_fields_without_tags() {
        let todo = Todo::new("Stable output").expect("valid todo");
        let value = serde_json::to_value(TodoQueryProjection::from(todo)).expect("serializable");
        assert_eq!(value["title"], "Stable output");
        assert!(value["tag_ids"].is_array());
        assert!(value.get("version").is_some());
    }
}
