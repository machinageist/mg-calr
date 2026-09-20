// Author: Jeff
// Date: 2026-09-19
// Description: mg-calr's records — calendars, events, projects and tags in one SQLite file
// Notes: One file, $MG_CALR_DB or $XDG_DATA_HOME/mg-calr/calr.sqlite, WAL, foreign keys on.
//        Timestamps are RFC 3339 UTC truncated to microseconds, which is both what PostgreSQL
//        stored and a fixed width, so TEXT comparison is chronological comparison. Dates are
//        plain YYYY-MM-DD for the same reason. Migrations are append-only, recorded with the
//        checksum of the SQL that ran, and never edited once applied.
//        Opening never migrates: `doctor`, `status` and `init` promise to diagnose only

#![allow(clippy::missing_errors_doc, clippy::must_use_candidate)]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use chrono_tz::Tz;
use rusqlite::types::FromSql;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::application::{
    AgendaRepository, AgendaTodoSnapshot, CalendarEventRepository, EventEdit, EventLifecycleError,
    EventLifecycleErrorMapping, ProjectRepository, TagRepository,
};
use crate::domain::{
    Calendar, CalendarId, Event, EventId, EventMetadata, EventRecurrence, EventStatus, EventTime,
    RfcUid,
    todo::{Project, ProjectId, Tag, TagId},
};

// ── Where the store lives ──

/// Environment override for the one SQLite file.
pub const DB_PATH_ENV: &str = "MG_CALR_DB";
/// File name under the XDG data directory when nothing overrides it.
pub const DEFAULT_DB_FILE: &str = "calr.sqlite";
/// How long a writer waits for another process to finish before giving up.
const BUSY_TIMEOUT_SECONDS: u64 = 5;

// Resolve the store path the way every command resolves it
#[must_use]
pub fn default_path(data_dir: &Path) -> PathBuf {
    std::env::var_os(DB_PATH_ENV).map_or_else(|| data_dir.join(DEFAULT_DB_FILE), PathBuf::from)
}

// ── Schema ──

// Calendars and events, the authority this application owns. The CHECK pair keeps a
// row either wholly timed or wholly all-day, which is what the domain's EventTime is.
pub const FOUNDATION_SQL: &str = "\
CREATE TABLE calendars (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    color TEXT,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT
);
CREATE UNIQUE INDEX calendars_one_default ON calendars (is_default)
    WHERE is_default = 1 AND deleted_at IS NULL;
CREATE TABLE events (
    id TEXT PRIMARY KEY,
    calendar_id TEXT NOT NULL REFERENCES calendars(id),
    rfc_uid TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    description TEXT,
    location TEXT,
    url TEXT,
    status TEXT,
    busy INTEGER NOT NULL DEFAULT 1,
    timezone TEXT,
    starts_at TEXT,
    ends_at TEXT,
    all_day_start TEXT,
    all_day_end TEXT,
    recurrence_rule TEXT,
    extension_properties TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    remote_tombstoned_at TEXT,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version >= 1),
    CHECK ((timezone IS NOT NULL AND starts_at IS NOT NULL AND ends_at IS NOT NULL
            AND all_day_start IS NULL AND all_day_end IS NULL)
        OR (timezone IS NULL AND starts_at IS NULL AND ends_at IS NULL
            AND all_day_start IS NOT NULL AND all_day_end IS NOT NULL)),
    CHECK (ends_at IS NULL OR ends_at > starts_at),
    CHECK (all_day_end IS NULL OR all_day_end > all_day_start),
    CHECK (recurrence_rule IS NULL OR json_valid(recurrence_rule)),
    CHECK (json_valid(extension_properties))
);
CREATE INDEX events_calendar_idx ON events (calendar_id);
CREATE INDEX events_recurrence_idx ON events (starts_at, all_day_start)
    WHERE recurrence_rule IS NOT NULL AND deleted_at IS NULL;";

// Project and tag metadata. Both are keyed by a normalized name so two spellings of
// one name cannot become two records; an archived project releases its name again.
pub const PROJECTS_AND_TAGS_SQL: &str = "\
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    archived_at TEXT,
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX projects_normalized_name_unique ON projects (normalized_name)
    WHERE archived_at IS NULL;
CREATE TABLE tags (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX tags_normalized_name_unique ON tags (normalized_name);";

/// One embedded schema migration.
///
/// `tables` names what the migration creates, so a ledger row whose tables are
/// absent is caught rather than trusted.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
    pub tables: &'static [&'static str],
}

pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "foundation",
        sql: FOUNDATION_SQL,
        tables: &["calendars", "events"],
    },
    Migration {
        version: 2,
        name: "projects_and_tags",
        sql: PROJECTS_AND_TAGS_SQL,
        tables: &["projects", "tags"],
    },
];

/// The append-only ledger. Every applied migration leaves one row here and no row is ever changed.
const LEDGER_SQL: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (\
     version INTEGER PRIMARY KEY, name TEXT NOT NULL, checksum TEXT NOT NULL, applied_at TEXT NOT NULL)";

/// State of one embedded migration against a live store.
#[derive(Debug, Clone, Serialize)]
pub struct MigrationState {
    pub version: i64,
    pub name: String,
    pub applied: bool,
}

// ── Errors ──

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid database configuration: {0}")]
    InvalidConfiguration(String),
    #[error("could not open the mg-calr database: {0}")]
    Open(String),
    #[error("database operation failed: {0}")]
    Query(#[source] rusqlite::Error),
    #[error("calendar {calendar_id} does not exist or is deleted")]
    CalendarNotLive { calendar_id: CalendarId },
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
    #[error("project {project_id} does not exist or is archived")]
    ProjectNotFound { project_id: ProjectId },
    #[error("tag '{normalized_name}' already exists")]
    TagAlreadyExists { normalized_name: String },
    #[error("tag {tag_id} does not exist")]
    TagNotFound { tag_id: TagId },
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

// ── Interchange document ──

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

    // Refuse a document that would not survive its own round trip
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

// ── Reading one event back ──

const EVENT_SELECT: &str = "SELECT e.id, e.calendar_id, e.rfc_uid, e.title, e.description, e.location, \
    e.url, e.status, e.busy, e.timezone, e.starts_at, e.ends_at, e.all_day_start, \
    e.all_day_end, e.recurrence_rule, e.extension_properties, e.created_at, e.updated_at, \
    e.deleted_at, e.remote_tombstoned_at, e.version FROM events e JOIN calendars c ON c.id = e.calendar_id";
// All-day rows lead their day; the rest follow the clock. Fixed-width stamps make
// substr(starts_at, 1, 10) the event's UTC date without a date function.
const EVENT_ORDER: &str = "ORDER BY CASE WHEN e.all_day_start IS NOT NULL THEN 0 ELSE 1 END, \
    COALESCE(e.all_day_start, substr(e.starts_at, 1, 10)), \
    e.starts_at, lower(e.title), e.id";

// ── The store ──

/// One SQLite file holding every calendar, event, project and tag mg-calr owns.
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    /// Open (creating) the file without applying migrations.
    ///
    /// `doctor`, `database status` and `init` all promise to diagnose only, so a
    /// migration never rides along with an open.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let path = path.into();
        if path.as_os_str().is_empty() {
            return Err(StorageError::InvalidConfiguration(
                "the database path is empty".to_owned(),
            ));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|error| StorageError::Open(format!("{}: {error}", parent.display())))?;
        }
        let store = Self { path };
        let connection = store.conn()?;
        // WAL lets the shell read while a command writes, and the mode lives in the file
        let mode: String = connection
            .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
            .map_err(StorageError::Query)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(StorageError::Open(format!(
                "the store could not switch to WAL (journal mode {mode})"
            )));
        }
        Ok(store)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    // Open one connection with the settings every connection needs
    fn conn(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open(&self.path)
            .map_err(|error| StorageError::Open(format!("{}: {error}", self.path.display())))?;
        connection
            .busy_timeout(Duration::from_secs(BUSY_TIMEOUT_SECONDS))
            .map_err(StorageError::Query)?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(StorageError::Query)?;
        Ok(connection)
    }

    // ── Migrations ──

    /// Apply every pending migration in one immediate transaction.
    pub fn migrate(&self) -> Result<Vec<MigrationState>, StorageError> {
        let mut connection = self.conn()?;
        // Take the write lock before reading the ledger, so two first-time opens
        // serialize rather than both finding it absent
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::Query)?;
        transaction
            .execute_batch(LEDGER_SQL)
            .map_err(StorageError::Query)?;
        let recorded = read_ledger(&transaction)?;
        verify_recorded_migrations(&transaction, &recorded)?;
        let applied_at = stamp(Utc::now());
        for migration in MIGRATIONS {
            if recorded
                .iter()
                .any(|(version, _)| *version == migration.version)
            {
                continue;
            }
            transaction
                .execute_batch(migration.sql)
                .map_err(StorageError::Query)?;
            transaction
                .execute(
                    "INSERT INTO schema_migrations (version, name, checksum, applied_at) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        migration.version,
                        migration.name,
                        migration_checksum(migration.sql),
                        applied_at
                    ],
                )
                .map_err(StorageError::Query)?;
        }
        transaction.commit().map_err(StorageError::Query)?;
        self.migration_status()
    }

    /// Read migration state without changing anything.
    pub fn migration_status(&self) -> Result<Vec<MigrationState>, StorageError> {
        let connection = self.conn()?;
        let ledger_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_migrations')",
                [],
                |row| row.get(0),
            )
            .map_err(StorageError::Query)?;
        if !ledger_exists {
            return Ok(MIGRATIONS
                .iter()
                .map(|migration| MigrationState {
                    version: migration.version,
                    name: migration.name.to_owned(),
                    applied: false,
                })
                .collect());
        }
        let recorded = read_ledger(&connection)?;
        verify_recorded_migrations(&connection, &recorded)?;
        Ok(MIGRATIONS
            .iter()
            .map(|migration| MigrationState {
                version: migration.version,
                name: migration.name.to_owned(),
                applied: recorded
                    .iter()
                    .any(|(version, _)| *version == migration.version),
            })
            .collect())
    }

    // ── Calendars and events ──

    /// Insert a calendar in a transaction.
    pub fn save_calendar(&self, calendar: &Calendar) -> Result<(), StorageError> {
        let connection = self.conn()?;
        connection
            .execute(
                "INSERT INTO calendars (id, name, color, is_default, created_at, updated_at, deleted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    calendar.id.to_string(),
                    calendar.name,
                    calendar.color,
                    calendar.is_default,
                    stamp(calendar.created_at),
                    stamp(calendar.updated_at),
                    optional_stamp(calendar.deleted_at),
                ],
            )
            .map_err(StorageError::Query)?;
        Ok(())
    }

    /// Insert an event only when its calendar is live.
    pub fn save_event(&self, event: &Event) -> Result<(), StorageError> {
        let mut connection = self.conn()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::Query)?;
        let live: Option<bool> = transaction
            .query_row(
                "SELECT deleted_at IS NULL FROM calendars WHERE id = ?1",
                params![event.calendar_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::Query)?;
        if live != Some(true) {
            return Err(StorageError::CalendarNotLive {
                calendar_id: event.calendar_id,
            });
        }
        insert_event_row(&transaction, event)?;
        transaction.commit().map_err(StorageError::Query)
    }

    /// Cancel a live event with an atomic optimistic-version check.
    pub fn cancel_event(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<Event, StorageError> {
        self.set_event_deletion(event_id, expected_version, true)
    }

    /// Restore a cancelled event with an atomic optimistic-version check.
    pub fn restore_event(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<Event, StorageError> {
        self.set_event_deletion(event_id, expected_version, false)
    }

    // Cancel and restore differ only in which lifecycle state they demand and leave
    fn set_event_deletion(
        &self,
        event_id: EventId,
        expected_version: i64,
        cancelling: bool,
    ) -> Result<Event, StorageError> {
        let mut connection = self.conn()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::Query)?;
        let current: Option<(i64, Option<String>)> = transaction
            .query_row(
                "SELECT version, deleted_at FROM events WHERE id = ?1",
                params![event_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StorageError::Query)?;
        let Some((actual_version, deleted_at)) = current else {
            return Err(StorageError::EventNotFound { event_id });
        };
        // Cancel wants a live row; restore wants a cancelled one. Lifecycle first,
        // so a caller holding a stale version is told what is actually wrong.
        if deleted_at.is_some() == cancelling {
            return Err(StorageError::EventNotFound { event_id });
        }
        if actual_version != expected_version {
            return Err(StorageError::EventVersionConflict {
                event_id,
                expected_version,
                actual_version,
            });
        }
        let now = stamp(Utc::now());
        let deleted = if cancelling { Some(now.clone()) } else { None };
        transaction
            .execute(
                "UPDATE events SET deleted_at = ?3, version = version + 1, updated_at = ?4 \
                 WHERE id = ?1 AND version = ?2",
                params![event_id.to_string(), expected_version, deleted, now],
            )
            .map_err(StorageError::Query)?;
        let event = read_one_event(&transaction, event_id)?;
        transaction.commit().map_err(StorageError::Query)?;
        Ok(event)
    }

    /// Edit title and/or temporal columns atomically with an optimistic version check.
    pub fn edit_event(
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
                Some(stamp(start.with_timezone(&Utc))),
                Some(stamp(end.with_timezone(&Utc))),
                None,
                None,
            ),
            Some(EventTime::AllDay {
                start,
                end_exclusive,
            }) => (
                None,
                None,
                None,
                Some(start.to_string()),
                Some(end_exclusive.to_string()),
            ),
            None => (None, None, None, None, None),
        };
        let mut connection = self.conn()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::Query)?;
        let current: Option<(i64, Option<String>)> = transaction
            .query_row(
                "SELECT version, deleted_at FROM events WHERE id = ?1",
                params![event_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(StorageError::Query)?;
        let Some((actual_version, deleted_at)) = current else {
            return Err(StorageError::EventNotFound { event_id });
        };
        if deleted_at.is_some() {
            return Err(StorageError::EventNotFound { event_id });
        }
        if actual_version != expected_version {
            return Err(StorageError::EventVersionConflict {
                event_id,
                expected_version,
                actual_version,
            });
        }
        transaction
            .execute(
                "UPDATE events SET title = COALESCE(?3, title), \
                 timezone = CASE WHEN ?4 THEN ?5 ELSE timezone END, \
                 starts_at = CASE WHEN ?4 THEN ?6 ELSE starts_at END, \
                 ends_at = CASE WHEN ?4 THEN ?7 ELSE ends_at END, \
                 all_day_start = CASE WHEN ?4 THEN ?8 ELSE all_day_start END, \
                 all_day_end = CASE WHEN ?4 THEN ?9 ELSE all_day_end END, \
                 version = version + 1, updated_at = ?10 \
                 WHERE id = ?1 AND deleted_at IS NULL AND version = ?2",
                params![
                    event_id.to_string(),
                    expected_version,
                    edit.title,
                    edit.time.is_some(),
                    timezone,
                    starts_at,
                    ends_at,
                    all_day_start,
                    all_day_end,
                    stamp(Utc::now()),
                ],
            )
            .map_err(StorageError::Query)?;
        let event = read_one_event(&transaction, event_id)?;
        transaction.commit().map_err(StorageError::Query)?;
        Ok(event)
    }

    /// List live calendars in deterministic name/ID order.
    pub fn list_calendars(&self) -> Result<Vec<Calendar>, StorageError> {
        let connection = self.conn()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, color, is_default, created_at, updated_at, deleted_at \
                 FROM calendars WHERE deleted_at IS NULL ORDER BY lower(name), id",
            )
            .map_err(StorageError::Query)?;
        collect(&mut statement, [], calendar_from_row)
    }

    /// Find one live event; absence is reported rather than raised.
    pub fn find_event(&self, event_id: EventId) -> Result<Option<Event>, StorageError> {
        let connection = self.conn()?;
        connection
            .query_row(
                &format!(
                    "{EVENT_SELECT} WHERE e.id = ?1 AND e.deleted_at IS NULL AND c.deleted_at IS NULL"
                ),
                params![event_id.to_string()],
                |row| Ok(event_from_row(row)),
            )
            .optional()
            .map_err(StorageError::Query)?
            .transpose()
    }

    /// List live events, optionally scoped to one live calendar.
    pub fn list_events(&self, calendar_id: Option<CalendarId>) -> Result<Vec<Event>, StorageError> {
        let connection = self.conn()?;
        let mut statement = connection
            .prepare(&format!(
                "{EVENT_SELECT} WHERE e.deleted_at IS NULL AND c.deleted_at IS NULL \
                 AND (?1 IS NULL OR e.calendar_id = ?1) {EVENT_ORDER}"
            ))
            .map_err(StorageError::Query)?;
        collect(
            &mut statement,
            params![calendar_id.map(|id| id.to_string())],
            event_from_row,
        )
    }

    /// List events overlapping a local calendar day in deterministic agenda order.
    pub fn day_agenda(
        &self,
        date: NaiveDate,
        starts_at: DateTime<chrono::FixedOffset>,
        ends_at: DateTime<chrono::FixedOffset>,
    ) -> Result<Vec<Event>, StorageError> {
        let next_date = date.succ_opt().ok_or_else(|| {
            StorageError::InvalidStoredData("day agenda date overflow".to_owned())
        })?;
        let connection = self.conn()?;
        let mut statement = connection
            .prepare(&format!(
                "{EVENT_SELECT} WHERE e.deleted_at IS NULL AND c.deleted_at IS NULL AND (\
                 (e.all_day_start IS NOT NULL AND e.all_day_start < ?2 AND e.all_day_end > ?1) OR \
                 (e.starts_at IS NOT NULL AND e.starts_at < ?4 AND e.ends_at > ?3)) {EVENT_ORDER}"
            ))
            .map_err(StorageError::Query)?;
        collect(
            &mut statement,
            params![
                date.to_string(),
                next_date.to_string(),
                stamp(starts_at.with_timezone(&Utc)),
                stamp(ends_at.with_timezone(&Utc)),
            ],
            event_from_row,
        )
    }

    // ── Interchange ──

    /// Export every calendar and event, cancelled ones included, in deterministic order.
    pub fn export_events(&self) -> Result<EventExport, StorageError> {
        let connection = self.conn()?;
        let calendars = {
            let mut statement = connection
                .prepare(
                    "SELECT id, name, color, is_default, created_at, updated_at, deleted_at \
                     FROM calendars ORDER BY lower(name), id",
                )
                .map_err(StorageError::Query)?;
            collect(&mut statement, [], calendar_from_row)?
        };
        let events = {
            let mut statement = connection
                .prepare(&format!("{EVENT_SELECT} {EVENT_ORDER}"))
                .map_err(StorageError::Query)?;
            collect(&mut statement, [], event_from_row)?
        };
        Ok(EventExport {
            schema_version: 1,
            calendars,
            events,
        })
    }

    /// Add events read from an iCalendar file to one existing calendar, atomically.
    ///
    /// A UID already present is a conflict: re-importing one file must not double a schedule.
    pub fn import_ics_events(
        &self,
        calendar_id: CalendarId,
        events: &[Event],
    ) -> Result<usize, StorageError> {
        let mut connection = self.conn()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::Query)?;
        let live: Option<bool> = transaction
            .query_row(
                "SELECT deleted_at IS NULL FROM calendars WHERE id = ?1",
                params![calendar_id.to_string()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::Query)?;
        if live != Some(true) {
            return Err(StorageError::ImportInvalid {
                reason: format!("calendar {calendar_id} does not exist"),
            });
        }
        for event in events {
            if exists(
                &transaction,
                "SELECT 1 FROM events WHERE rfc_uid = ?1",
                params![event.rfc_uid.as_str()],
            )? {
                return Err(StorageError::ImportConflict {
                    kind: "event",
                    id: event.rfc_uid.as_str().to_owned(),
                });
            }
            insert_event_row(&transaction, event)?;
        }
        transaction.commit().map_err(StorageError::Query)?;
        Ok(events.len())
    }

    /// Restore a document this application previously exported, atomically and without overwriting.
    pub fn import_events(&self, payload: &EventExport) -> Result<usize, StorageError> {
        payload.validate()?;
        let mut connection = self.conn()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StorageError::Query)?;
        for calendar in &payload.calendars {
            if exists(
                &transaction,
                "SELECT 1 FROM calendars WHERE id = ?1",
                params![calendar.id.to_string()],
            )? {
                return Err(StorageError::ImportConflict {
                    kind: "calendar",
                    id: calendar.id.to_string(),
                });
            }
            if calendar.is_default
                && calendar.deleted_at.is_none()
                && exists(
                    &transaction,
                    "SELECT 1 FROM calendars WHERE is_default = 1 AND deleted_at IS NULL",
                    params![],
                )?
            {
                return Err(StorageError::ImportConflict {
                    kind: "calendar",
                    id: calendar.id.to_string(),
                });
            }
        }
        for event in &payload.events {
            if exists(
                &transaction,
                "SELECT 1 FROM events WHERE id = ?1 OR rfc_uid = ?2",
                params![event.id.to_string(), event.rfc_uid.as_str()],
            )? {
                return Err(StorageError::ImportConflict {
                    kind: "event",
                    id: event.id.to_string(),
                });
            }
        }
        for calendar in &payload.calendars {
            transaction
                .execute(
                    "INSERT INTO calendars (id, name, color, is_default, created_at, updated_at, deleted_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        calendar.id.to_string(),
                        calendar.name,
                        calendar.color,
                        calendar.is_default,
                        stamp(calendar.created_at),
                        stamp(calendar.updated_at),
                        optional_stamp(calendar.deleted_at),
                    ],
                )
                .map_err(StorageError::Query)?;
        }
        for event in &payload.events {
            insert_event_row(&transaction, event)?;
        }
        transaction.commit().map_err(StorageError::Query)?;
        Ok(payload.calendars.len() + payload.events.len())
    }

    // ── Projects and tags ──

    pub fn save_project(&self, project: &Project) -> Result<(), StorageError> {
        let connection = self.conn()?;
        connection
            .execute(
                "INSERT INTO projects (id, name, normalized_name, archived_at, version, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    project.id.to_string(),
                    project.name,
                    project.normalized_name,
                    optional_stamp(project.archived_at),
                    project.version,
                    stamp(project.created_at),
                    stamp(project.updated_at),
                ],
            )
            .map_err(StorageError::Query)?;
        Ok(())
    }

    pub fn find_project(&self, id: ProjectId) -> Result<Option<Project>, StorageError> {
        let connection = self.conn()?;
        connection
            .query_row(
                "SELECT id, name, normalized_name, archived_at, version, created_at, updated_at \
                 FROM projects WHERE id = ?1",
                params![id.to_string()],
                |row| Ok(project_from_row(row)),
            )
            .optional()
            .map_err(StorageError::Query)?
            .transpose()
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, StorageError> {
        let connection = self.conn()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, normalized_name, archived_at, version, created_at, updated_at \
                 FROM projects WHERE archived_at IS NULL ORDER BY normalized_name, id",
            )
            .map_err(StorageError::Query)?;
        collect(&mut statement, [], project_from_row)
    }

    pub fn save_tag(&self, tag: &Tag) -> Result<(), StorageError> {
        let connection = self.conn()?;
        connection
            .execute(
                "INSERT INTO tags (id, name, normalized_name, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    tag.id.to_string(),
                    tag.name,
                    tag.normalized_name,
                    stamp(tag.created_at),
                    stamp(tag.updated_at),
                ],
            )
            .map_err(|error| {
                // A duplicate normalized name is the one insert failure a person caused
                if is_unique_violation(&error) {
                    StorageError::TagAlreadyExists {
                        normalized_name: tag.normalized_name.clone(),
                    }
                } else {
                    StorageError::Query(error)
                }
            })?;
        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<Tag>, StorageError> {
        let connection = self.conn()?;
        let mut statement = connection
            .prepare(
                "SELECT id, name, normalized_name, created_at, updated_at \
                 FROM tags ORDER BY normalized_name, id",
            )
            .map_err(StorageError::Query)?;
        collect(&mut statement, [], tag_from_row)
    }
}

// ── The application's boundaries, served by the one store ──

impl CalendarEventRepository for Store {
    type Error = StorageError;

    fn save_calendar(&self, calendar: &Calendar) -> Result<(), Self::Error> {
        Self::save_calendar(self, calendar)
    }

    fn save_event(&self, event: &Event) -> Result<(), Self::Error> {
        Self::save_event(self, event)
    }

    fn list_calendars(&self) -> Result<Vec<Calendar>, Self::Error> {
        Self::list_calendars(self)
    }

    fn find_event(&self, id: EventId) -> Result<Option<Event>, Self::Error> {
        Self::find_event(self, id)
    }

    fn list_events(&self, calendar_id: Option<CalendarId>) -> Result<Vec<Event>, Self::Error> {
        Self::list_events(self, calendar_id)
    }

    fn cancel_event(&self, id: EventId, expected_version: i64) -> Result<Event, Self::Error> {
        Self::cancel_event(self, id, expected_version)
    }

    fn restore_event(&self, id: EventId, expected_version: i64) -> Result<Event, Self::Error> {
        Self::restore_event(self, id, expected_version)
    }

    fn edit_event(
        &self,
        id: EventId,
        expected_version: i64,
        edit: &EventEdit,
    ) -> Result<Event, Self::Error> {
        Self::edit_event(self, id, expected_version, edit)
    }

    fn day_agenda(
        &self,
        date: NaiveDate,
        _timezone: &str,
        starts_at: DateTime<chrono::FixedOffset>,
        ends_at: DateTime<chrono::FixedOffset>,
    ) -> Result<Vec<Event>, Self::Error> {
        Self::day_agenda(self, date, starts_at, ends_at)
    }
}

impl TagRepository for Store {
    type Error = StorageError;

    fn save_tag(&self, tag: &Tag) -> Result<(), Self::Error> {
        Self::save_tag(self, tag)
    }

    fn list_tags(&self) -> Result<Vec<Tag>, Self::Error> {
        Self::list_tags(self)
    }
}

impl ProjectRepository for Store {
    type Error = StorageError;

    fn save_project(&self, project: &Project) -> Result<(), Self::Error> {
        Self::save_project(self, project)
    }

    fn find_project(&self, id: ProjectId) -> Result<Option<Project>, Self::Error> {
        Self::find_project(self, id)
    }

    fn list_projects(&self) -> Result<Vec<Project>, Self::Error> {
        Self::list_projects(self)
    }
}

/// Read-only agenda boundary that keeps events in SQLite and todos in the
/// validated immutable mg-remindr projection.
///
/// It holds the store's path rather than an open store, because the todo projection is
/// read first: a missing projection is reported as a missing projection even when the
/// calendar store cannot be opened at all.
#[derive(Debug, Clone)]
pub struct ProjectionAgendaRepository {
    events: PathBuf,
    todo_projection: PathBuf,
}

impl ProjectionAgendaRepository {
    pub fn new(events: impl Into<PathBuf>, todo_projection: PathBuf) -> Self {
        Self {
            events: events.into(),
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

impl AgendaRepository for ProjectionAgendaRepository {
    type Error = AgendaRepositoryError;

    fn agenda_events(&self, _include_trashed: bool) -> Result<Vec<Event>, Self::Error> {
        Store::open(&self.events)
            .and_then(|store| store.list_events(None))
            .map_err(AgendaRepositoryError::Calendar)
    }

    fn agenda_todos(&self, include_trashed: bool) -> Result<AgendaTodoSnapshot, Self::Error> {
        let projection = crate::interop::TodoProjectionSnapshot::load(&self.todo_projection)?;
        let mut snapshot = projection.agenda_todos()?;
        if !include_trashed {
            snapshot.todos.retain(|todo| todo.trashed_at.is_none());
        }
        Ok(snapshot)
    }
}

// ── Row and value helpers ──

// Hash the SQL that ran, so a migration edited after the fact is caught rather than skipped
fn migration_checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}

// Read the ledger in version order
fn read_ledger(connection: &Connection) -> Result<Vec<(i64, Option<String>)>, StorageError> {
    let mut statement = connection
        .prepare("SELECT version, checksum FROM schema_migrations ORDER BY version")
        .map_err(StorageError::Query)?;
    let rows = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(StorageError::Query)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::Query)?;
    Ok(rows)
}

/// Refuse a store whose recorded migrations no longer match the embedded ones.
///
/// A migration edited after it was applied is invisible to a version-only ledger:
/// the runner skips it, the new statements never run, and the store reports itself
/// current while missing tables. Compare the checksum, then the live tables.
fn verify_recorded_migrations(
    connection: &Connection,
    recorded: &[(i64, Option<String>)],
) -> Result<(), StorageError> {
    for (version, checksum) in recorded {
        let Some(migration) = MIGRATIONS.iter().find(|m| m.version == *version) else {
            return Err(StorageError::MigrationDrift {
                version: *version,
                actual: "unknown migration".to_owned(),
                expected: "an embedded migration",
            });
        };
        let expected = migration_checksum(migration.sql);
        if checksum.as_deref().is_some_and(|actual| actual != expected) {
            return Err(StorageError::MigrationDrift {
                version: *version,
                actual: checksum.clone().unwrap_or_default(),
                expected: migration.name,
            });
        }
        for table in migration.tables {
            let present: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                    params![table],
                    |row| row.get(0),
                )
                .map_err(StorageError::Query)?;
            if !present {
                return Err(StorageError::MigrationDrift {
                    version: *version,
                    actual: format!("missing live table {table}"),
                    expected: migration.name,
                });
            }
        }
    }
    Ok(())
}

// Run one prepared statement and turn every row into a domain record
fn collect<T>(
    statement: &mut rusqlite::Statement<'_>,
    parameters: impl rusqlite::Params,
    read: fn(&Row<'_>) -> Result<T, StorageError>,
) -> Result<Vec<T>, StorageError> {
    let rows = statement
        .query_map(parameters, |row| Ok(read(row)))
        .map_err(StorageError::Query)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(StorageError::Query)?;
    rows.into_iter().collect()
}

// Whether any row matches, without reading one
fn exists(
    connection: &Connection,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<bool, StorageError> {
    connection
        .query_row(sql, parameters, |_| Ok(()))
        .optional()
        .map_err(StorageError::Query)
        .map(|row| row.is_some())
}

// Whether a failed insert lost a uniqueness race rather than the database
fn is_unique_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

// Read one column, naming it a storage failure rather than a bare SQLite one
fn column<T: FromSql>(row: &Row<'_>, index: usize) -> Result<T, StorageError> {
    row.get(index).map_err(StorageError::Query)
}

// One instant, fixed width and in UTC, so TEXT order is time order
fn stamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn optional_stamp(value: Option<DateTime<Utc>>) -> Option<String> {
    value.map(stamp)
}

// Read an instant back, refusing a column this application did not write
fn read_stamp(text: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(text)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| StorageError::InvalidStoredData(format!("invalid timestamp: {error}")))
}

fn read_optional_stamp(text: Option<String>) -> Result<Option<DateTime<Utc>>, StorageError> {
    text.map(|text| read_stamp(&text)).transpose()
}

fn read_date(text: &str) -> Result<NaiveDate, StorageError> {
    text.parse()
        .map_err(|error| StorageError::InvalidStoredData(format!("invalid date: {error}")))
}

// Read one identifier back, naming what it was meant to be
fn read_id<T: std::str::FromStr>(text: &str, kind: &str) -> Result<T, StorageError>
where
    T::Err: std::fmt::Display,
{
    text.parse().map_err(|error| {
        StorageError::InvalidStoredData(format!("invalid {kind} identifier: {error}"))
    })
}

fn calendar_from_row(row: &Row<'_>) -> Result<Calendar, StorageError> {
    Calendar::rehydrate(
        read_id(&column::<String>(row, 0)?, "calendar")?,
        column::<String>(row, 1)?,
        column(row, 2)?,
        column(row, 3)?,
        read_stamp(&column::<String>(row, 4)?)?,
        read_stamp(&column::<String>(row, 5)?)?,
        read_optional_stamp(column(row, 6)?)?,
    )
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
}

fn project_from_row(row: &Row<'_>) -> Result<Project, StorageError> {
    Project {
        id: read_id(&column::<String>(row, 0)?, "project")?,
        name: column(row, 1)?,
        normalized_name: column(row, 2)?,
        archived_at: read_optional_stamp(column(row, 3)?)?,
        version: column(row, 4)?,
        created_at: read_stamp(&column::<String>(row, 5)?)?,
        updated_at: read_stamp(&column::<String>(row, 6)?)?,
    }
    .rehydrate()
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
}

fn tag_from_row(row: &Row<'_>) -> Result<Tag, StorageError> {
    Tag {
        id: read_id(&column::<String>(row, 0)?, "tag")?,
        name: column(row, 1)?,
        normalized_name: column(row, 2)?,
        created_at: read_stamp(&column::<String>(row, 3)?)?,
        updated_at: read_stamp(&column::<String>(row, 4)?)?,
    }
    .rehydrate()
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
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

/// The metadata that has no column of its own, kept as one JSON document.
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

/// Encode a validated rule for its column.
fn recurrence_text(rule: Option<&EventRecurrence>) -> Result<Option<String>, StorageError> {
    rule.map(|rule| {
        rule.validate()
            .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
        serde_json::to_string(rule)
            .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
    })
    .transpose()
}

/// Read a stored rule back, revalidating rather than trusting the column.
fn recurrence_from_text(text: Option<String>) -> Result<Option<EventRecurrence>, StorageError> {
    let Some(text) = text else {
        return Ok(None);
    };
    let rule: EventRecurrence = serde_json::from_str(&text)
        .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    rule.validate()
        .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    Ok(Some(rule))
}

// Write one complete event row inside an open transaction
fn insert_event_row(
    transaction: &rusqlite::Transaction<'_>,
    event: &Event,
) -> Result<(), StorageError> {
    let (timezone, starts_at, ends_at, all_day_start, all_day_end) = match &event.time {
        EventTime::Timed {
            start,
            end,
            timezone,
        } => (
            Some(timezone.clone()),
            Some(stamp(start.with_timezone(&Utc))),
            Some(stamp(end.with_timezone(&Utc))),
            None,
            None,
        ),
        EventTime::AllDay {
            start,
            end_exclusive,
        } => (
            None,
            None,
            None,
            Some(start.to_string()),
            Some(end_exclusive.to_string()),
        ),
    };
    let extension_properties = serde_json::json!({
        "categories": event.metadata.categories,
        "alarms": event.metadata.alarms,
        "organizer": event.metadata.organizer,
        "attendees": event.metadata.attendees,
    })
    .to_string();
    transaction
        .execute(
            "INSERT INTO events (id, calendar_id, rfc_uid, title, description, location, url, status, busy, \
             timezone, starts_at, ends_at, all_day_start, all_day_end, recurrence_rule, extension_properties, \
             created_at, updated_at, deleted_at, remote_tombstoned_at, version) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                event.id.to_string(),
                event.calendar_id.to_string(),
                event.rfc_uid.as_str(),
                event.title,
                event.metadata.description,
                event.metadata.location,
                event.metadata.url,
                event.metadata.status.map(event_status),
                event.metadata.busy,
                timezone,
                starts_at,
                ends_at,
                all_day_start,
                all_day_end,
                recurrence_text(event.metadata.recurrence_rule.as_ref())?,
                extension_properties,
                stamp(event.created_at),
                stamp(event.updated_at),
                optional_stamp(event.deleted_at),
                optional_stamp(event.remote_tombstoned_at),
                event.version,
            ],
        )
        .map_err(StorageError::Query)?;
    Ok(())
}

// Read one event by id inside an open transaction, whatever its lifecycle state
fn read_one_event(
    transaction: &rusqlite::Transaction<'_>,
    event_id: EventId,
) -> Result<Event, StorageError> {
    transaction
        .query_row(
            &format!("{EVENT_SELECT} WHERE e.id = ?1"),
            params![event_id.to_string()],
            |row| Ok(event_from_row(row)),
        )
        .map_err(StorageError::Query)?
}

fn event_from_row(row: &Row<'_>) -> Result<Event, StorageError> {
    let timezone = column::<Option<String>>(row, 9)?;
    let starts_at = column::<Option<String>>(row, 10)?;
    let ends_at = column::<Option<String>>(row, 11)?;
    let all_day_start = column::<Option<String>>(row, 12)?;
    let all_day_end = column::<Option<String>>(row, 13)?;
    let time = match (timezone, starts_at, ends_at, all_day_start, all_day_end) {
        (Some(timezone), Some(start), Some(end), None, None) => {
            let zone = timezone.parse::<Tz>().map_err(|_| {
                StorageError::InvalidStoredData(format!("invalid IANA timezone '{timezone}'"))
            })?;
            EventTime::timed(
                read_stamp(&start)?.with_timezone(&zone).fixed_offset(),
                read_stamp(&end)?.with_timezone(&zone).fixed_offset(),
                timezone,
            )
        }
        (None, None, None, Some(start), Some(end)) => {
            EventTime::all_day(read_date(&start)?, read_date(&end)?)
        }
        _ => {
            return Err(StorageError::InvalidStoredData(
                "event has mixed or incomplete temporal columns".to_owned(),
            ));
        }
    }
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    let extension: ExtensionProperties = serde_json::from_str(&column::<String>(row, 15)?)
        .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?;
    let metadata = EventMetadata {
        description: column(row, 4)?,
        location: column(row, 5)?,
        url: column(row, 6)?,
        status: parse_event_status(column(row, 7)?)?,
        busy: column(row, 8)?,
        categories: extension.categories,
        recurrence_rule: recurrence_from_text(column(row, 14)?)?,
        alarms: extension.alarms,
        organizer: extension.organizer,
        attendees: extension.attendees,
    };
    Event::rehydrate(
        read_id(&column::<String>(row, 0)?, "event")?,
        read_id(&column::<String>(row, 1)?, "calendar")?,
        RfcUid::new(column::<String>(row, 2)?)
            .map_err(|error| StorageError::InvalidStoredData(error.to_string()))?,
        column::<String>(row, 3)?,
        time,
        metadata,
        read_stamp(&column::<String>(row, 16)?)?,
        read_stamp(&column::<String>(row, 17)?)?,
        read_optional_stamp(column(row, 18)?)?,
        read_optional_stamp(column(row, 19)?)?,
        column(row, 20)?,
    )
    .map_err(|error| StorageError::InvalidStoredData(error.to_string()))
}

#[cfg(test)]
#[allow(clippy::too_many_lines)]
mod tests {
    use super::*;
    use crate::domain::EventFrequency;

    // A store of its own, migrated, in a directory that disappears with the test
    fn store() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().expect("a temporary directory");
        let store = Store::open(directory.path().join("calr.sqlite")).expect("a fresh store");
        store.migrate().expect("migrations apply");
        (directory, store)
    }

    fn calendar(store: &Store, name: &str) -> Calendar {
        let calendar = Calendar::new(name).expect("a valid calendar");
        store.save_calendar(&calendar).expect("the calendar saves");
        calendar
    }

    fn instant(value: &str) -> DateTime<chrono::FixedOffset> {
        value.parse().expect("an RFC3339 fixture")
    }

    fn timed(calendar: &Calendar, title: &str, start: &str, end: &str) -> Event {
        Event::new(
            calendar.id,
            title,
            EventTime::timed(instant(start), instant(end), "America/Los_Angeles")
                .expect("a valid span"),
        )
        .expect("a valid event")
    }

    fn all_day(calendar: &Calendar, title: &str, start: &str, end: &str) -> Event {
        Event::new(
            calendar.id,
            title,
            EventTime::all_day(start.parse().expect("a date"), end.parse().expect("a date"))
                .expect("a valid span"),
        )
        .expect("a valid event")
    }

    #[test]
    fn an_empty_file_migrates_to_the_whole_schema_and_migrating_again_changes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path().join("calr.sqlite")).unwrap();

        let before = store.migration_status().unwrap();
        assert!(before.iter().all(|state| !state.applied));

        let first = store.migrate().unwrap();
        let second = store.migrate().unwrap();
        assert_eq!(first.len(), MIGRATIONS.len());
        assert_eq!(first.len(), second.len());
        assert!(second.iter().all(|state| state.applied));

        // One ledger row per migration, and no row written twice
        let connection = store.conn().unwrap();
        let versions: Vec<i64> = connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let applied = i64::try_from(MIGRATIONS.len()).unwrap();
        assert_eq!(versions, (1..=applied).collect::<Vec<_>>());
    }

    #[test]
    fn the_file_lands_where_the_environment_says() {
        assert_eq!(
            default_path(Path::new("/xdg/data/mg-calr")),
            PathBuf::from("/xdg/data/mg-calr/calr.sqlite")
        );
        assert!(Store::open("").is_err(), "an empty path is not a store");
    }

    #[test]
    fn a_rewritten_migration_is_refused_rather_than_skipped() {
        let (_directory, store) = store();
        store
            .conn()
            .unwrap()
            .execute(
                "UPDATE schema_migrations SET checksum = 'rewritten' WHERE version = 1",
                [],
            )
            .unwrap();

        let error = store.migration_status().unwrap_err();
        assert!(matches!(
            error,
            StorageError::MigrationDrift { version: 1, .. }
        ));
    }

    #[test]
    fn a_ledger_row_whose_tables_are_gone_is_refused() {
        let (_directory, store) = store();
        store
            .conn()
            .unwrap()
            .execute_batch("DROP INDEX events_recurrence_idx; DROP TABLE events;")
            .unwrap();

        assert!(matches!(
            store.migration_status().unwrap_err(),
            StorageError::MigrationDrift { .. }
        ));
    }

    #[test]
    fn an_event_round_trips_through_its_columns_with_every_field_intact() {
        let (_directory, store) = store();
        let work = calendar(&store, "Work");
        let mut event = timed(
            &work,
            "Standup",
            "2026-08-24T09:00:00-07:00",
            "2026-08-24T09:15:00-07:00",
        );
        event.metadata.description = Some("the daily one".to_owned());
        event.metadata.location = Some("the kitchen".to_owned());
        event.metadata.url = Some("https://example.invalid/standup".to_owned());
        event.metadata.status = Some(EventStatus::Confirmed);
        event.metadata.busy = false;
        event.metadata.categories = vec!["work".to_owned()];
        event.metadata.organizer = Some("jeff".to_owned());
        event.metadata.attendees = vec!["josh".to_owned()];
        store.save_event(&event).unwrap();

        let stored = store.find_event(event.id).unwrap().expect("it is stored");
        assert_eq!(stored.title, "Standup");
        assert_eq!(
            stored.metadata.description.as_deref(),
            Some("the daily one")
        );
        assert_eq!(stored.metadata.location.as_deref(), Some("the kitchen"));
        assert_eq!(stored.metadata.status, Some(EventStatus::Confirmed));
        assert!(!stored.metadata.busy);
        assert_eq!(stored.metadata.categories, ["work"]);
        assert_eq!(stored.metadata.organizer.as_deref(), Some("jeff"));
        assert_eq!(stored.metadata.attendees, ["josh"]);
        assert_eq!(stored.time, event.time);
        assert_eq!(stored.rfc_uid, event.rfc_uid);
        assert_eq!(stored.version, 1);
    }

    #[test]
    fn an_all_day_span_keeps_its_dates_and_an_event_needs_a_live_calendar() {
        let (_directory, store) = store();
        let work = calendar(&store, "Work");
        let span = all_day(&work, "Conference", "2026-08-24", "2026-08-27");
        store.save_event(&span).unwrap();

        let stored = store.find_event(span.id).unwrap().unwrap();
        assert_eq!(
            stored.time,
            EventTime::all_day("2026-08-24".parse().unwrap(), "2026-08-27".parse().unwrap())
                .unwrap()
        );

        // An event whose calendar was never created is refused by name
        let orphan = Event::new(
            CalendarId::new(),
            "Nowhere",
            EventTime::all_day("2026-08-24".parse().unwrap(), "2026-08-25".parse().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            store.save_event(&orphan).unwrap_err(),
            StorageError::CalendarNotLive { .. }
        ));
    }

    #[test]
    fn a_repeating_event_keeps_its_rule_and_the_rule_is_revalidated_on_the_way_back() {
        let (_directory, store) = store();
        let study = calendar(&store, "Study");
        let mut event = timed(
            &study,
            "Practice",
            "2026-09-07T10:00:00-07:00",
            "2026-09-07T12:00:00-07:00",
        );
        let rule = EventRecurrence::new(
            EventFrequency::Weekly,
            1,
            Some(13),
            None,
            vec![chrono::Weekday::Mon],
        )
        .unwrap();
        event.metadata.recurrence_rule = Some(rule.clone());
        store.save_event(&event).unwrap();

        let stored = store.find_event(event.id).unwrap().unwrap();
        assert_eq!(stored.metadata.recurrence_rule, Some(rule));

        // A column this application did not write is refused, not expanded
        store
            .conn()
            .unwrap()
            .execute(
                "UPDATE events SET recurrence_rule = '{\"frequency\":\"weekly\"}' WHERE id = ?1",
                params![event.id.to_string()],
            )
            .unwrap();
        assert!(matches!(
            store.find_event(event.id).unwrap_err(),
            StorageError::InvalidStoredData(_)
        ));
    }

    #[test]
    fn cancel_and_restore_move_one_version_at_a_time_and_refuse_a_stale_one() {
        let (_directory, store) = store();
        let work = calendar(&store, "Work");
        let event = timed(
            &work,
            "Standup",
            "2026-08-24T09:00:00-07:00",
            "2026-08-24T09:15:00-07:00",
        );
        store.save_event(&event).unwrap();

        assert!(matches!(
            store.cancel_event(event.id, 7).unwrap_err(),
            StorageError::EventVersionConflict {
                expected_version: 7,
                actual_version: 1,
                ..
            }
        ));
        let cancelled = store.cancel_event(event.id, 1).unwrap();
        assert_eq!(cancelled.version, 2);
        assert!(cancelled.deleted_at.is_some());
        assert!(store.find_event(event.id).unwrap().is_none());
        // Cancelling twice is not a version problem; the event is simply not live
        assert!(matches!(
            store.cancel_event(event.id, 2).unwrap_err(),
            StorageError::EventNotFound { .. }
        ));

        let restored = store.restore_event(event.id, 2).unwrap();
        assert_eq!(restored.version, 3);
        assert!(restored.deleted_at.is_none());
        assert!(matches!(
            store.restore_event(event.id, 3).unwrap_err(),
            StorageError::EventNotFound { .. }
        ));
        assert!(matches!(
            store.cancel_event(EventId::new(), 1).unwrap_err(),
            StorageError::EventNotFound { .. }
        ));
    }

    #[test]
    fn an_edit_takes_a_whole_temporal_form_and_leaves_the_rest_alone() {
        let (_directory, store) = store();
        let work = calendar(&store, "Work");
        let event = timed(
            &work,
            "Standup",
            "2026-08-24T09:00:00-07:00",
            "2026-08-24T09:15:00-07:00",
        );
        store.save_event(&event).unwrap();

        let titled = store
            .edit_event(
                event.id,
                1,
                &EventEdit {
                    title: Some("Renamed".to_owned()),
                    time: None,
                },
            )
            .unwrap();
        assert_eq!(titled.title, "Renamed");
        assert_eq!(titled.time, event.time, "an untouched time is untouched");
        assert_eq!(titled.version, 2);

        let moved = store
            .edit_event(
                event.id,
                2,
                &EventEdit {
                    title: None,
                    time: Some(
                        EventTime::all_day(
                            "2026-08-24".parse().unwrap(),
                            "2026-08-25".parse().unwrap(),
                        )
                        .unwrap(),
                    ),
                },
            )
            .unwrap();
        assert_eq!(moved.title, "Renamed", "an untouched title is untouched");
        assert!(matches!(moved.time, EventTime::AllDay { .. }));
        assert_eq!(moved.version, 3);

        assert!(matches!(
            store
                .edit_event(
                    event.id,
                    2,
                    &EventEdit {
                        title: Some("Stale".to_owned()),
                        time: None
                    }
                )
                .unwrap_err(),
            StorageError::EventVersionConflict { .. }
        ));
    }

    #[test]
    fn listing_is_scoped_deterministic_and_leaves_cancelled_rows_out() {
        let (_directory, store) = store();
        let work = calendar(&store, "Work");
        let home = calendar(&store, "Home");
        let late = timed(
            &work,
            "Late",
            "2026-08-24T10:00:00-07:00",
            "2026-08-24T11:00:00-07:00",
        );
        let early = timed(
            &work,
            "Early",
            "2026-08-24T08:00:00-07:00",
            "2026-08-24T09:00:00-07:00",
        );
        let span = all_day(&work, "All day", "2026-08-24", "2026-08-25");
        let elsewhere = all_day(&home, "Elsewhere", "2026-08-24", "2026-08-25");
        for event in [&late, &early, &span, &elsewhere] {
            store.save_event(event).unwrap();
        }

        let ordered: Vec<EventId> = store
            .list_events(None)
            .unwrap()
            .iter()
            .map(|event| event.id)
            .collect();
        assert_eq!(ordered.len(), 4);
        assert_eq!(
            &ordered[..2],
            // All-day rows lead the day, ordered by title
            &[span.id, elsewhere.id]
        );
        assert_eq!(&ordered[2..], &[early.id, late.id]);

        let scoped: Vec<EventId> = store
            .list_events(Some(home.id))
            .unwrap()
            .iter()
            .map(|event| event.id)
            .collect();
        assert_eq!(scoped, [elsewhere.id]);

        store.cancel_event(late.id, 1).unwrap();
        assert_eq!(store.list_events(None).unwrap().len(), 3);

        let names: Vec<String> = store
            .list_calendars()
            .unwrap()
            .into_iter()
            .map(|calendar| calendar.name)
            .collect();
        assert_eq!(names, ["Home", "Work"]);
    }

    #[test]
    fn a_day_agenda_holds_what_overlaps_that_local_day_and_nothing_else() {
        let (_directory, store) = store();
        let work = calendar(&store, "Work");
        let during = timed(
            &work,
            "During",
            "2026-08-24T09:00:00-07:00",
            "2026-08-24T09:15:00-07:00",
        );
        let before = timed(
            &work,
            "Before",
            "2026-08-23T09:00:00-07:00",
            "2026-08-23T09:15:00-07:00",
        );
        let spanning = all_day(&work, "Spanning", "2026-08-23", "2026-08-26");
        let after = all_day(&work, "After", "2026-08-25", "2026-08-26");
        for event in [&during, &before, &spanning, &after] {
            store.save_event(event).unwrap();
        }

        let date: NaiveDate = "2026-08-24".parse().unwrap();
        let found: Vec<EventId> = store
            .day_agenda(
                date,
                instant("2026-08-24T00:00:00-07:00"),
                instant("2026-08-25T00:00:00-07:00"),
            )
            .unwrap()
            .iter()
            .map(|event| event.id)
            .collect();
        assert_eq!(found, [spanning.id, during.id]);
    }

    #[test]
    fn an_export_round_trips_into_an_empty_store_and_refuses_to_land_twice() {
        let (_directory, store) = store();
        let work = calendar(&store, "Work");
        let event = timed(
            &work,
            "Standup",
            "2026-08-24T09:00:00-07:00",
            "2026-08-24T09:15:00-07:00",
        );
        store.save_event(&event).unwrap();
        store.cancel_event(event.id, 1).unwrap();

        let exported = store.export_events().unwrap();
        assert_eq!(exported.schema_version, 1);
        assert_eq!(exported.calendars.len(), 1);
        assert_eq!(
            exported.events.len(),
            1,
            "an export carries cancelled events too"
        );

        let document = serde_json::to_string(&exported).unwrap();
        let parsed = EventExport::parse(&document).unwrap();
        let (_other_directory, other) = self::tests::store();
        assert_eq!(other.import_events(&parsed).unwrap(), 2);
        assert_eq!(other.export_events().unwrap(), exported);

        // The same document a second time is a conflict, not a duplicate schedule
        assert!(matches!(
            other.import_events(&parsed).unwrap_err(),
            StorageError::ImportConflict {
                kind: "calendar",
                ..
            }
        ));
        assert!(
            EventExport::parse("{\"schema_version\":2,\"calendars\":[],\"events\":[]}").is_err()
        );
    }

    #[test]
    fn an_icalendar_file_lands_once_and_keeps_every_repeat_rule() {
        let (_directory, store) = store();
        let study = calendar(&store, "Study");
        let document = include_str!("../tests/fixtures/weekly-study-rhythm.ics");
        let parsed = crate::ics::read(document).expect("the fixture reads");
        let events: Vec<Event> = parsed
            .into_iter()
            .map(|source| {
                let mut event =
                    Event::new(study.id, source.title, source.time).expect("a valid event");
                event.rfc_uid = RfcUid::new(source.uid.expect("the fixture carries UIDs")).unwrap();
                event.metadata.description = source.description;
                event.metadata.recurrence_rule = source.recurrence;
                event
            })
            .collect();
        let expected = events.len();

        assert_eq!(
            store.import_ics_events(study.id, &events).unwrap(),
            expected
        );
        let stored = store.list_events(Some(study.id)).unwrap();
        assert_eq!(stored.len(), expected);
        assert!(
            stored
                .iter()
                .all(|event| event.metadata.recurrence_rule.is_some()),
            "every fixture event repeats"
        );

        // The same file again is a conflict on the first UID it carries
        assert!(matches!(
            store.import_ics_events(study.id, &events).unwrap_err(),
            StorageError::ImportConflict { kind: "event", .. }
        ));
        assert_eq!(store.list_events(Some(study.id)).unwrap().len(), expected);

        // A calendar that does not exist is named rather than created
        assert!(matches!(
            store
                .import_ics_events(CalendarId::new(), &events)
                .unwrap_err(),
            StorageError::ImportInvalid { .. }
        ));
    }

    #[test]
    fn projects_and_tags_keep_one_record_per_normalized_name() {
        let (_directory, store) = store();
        let project = Project::new("Bootcamp").unwrap();
        store.save_project(&project).unwrap();
        assert_eq!(
            store.find_project(project.id).unwrap().map(|p| p.name),
            Some("Bootcamp".to_owned())
        );
        assert!(store.find_project(ProjectId::new()).unwrap().is_none());
        assert_eq!(store.list_projects().unwrap().len(), 1);

        let tag = Tag::new("Deep work").unwrap();
        store.save_tag(&tag).unwrap();
        assert_eq!(store.list_tags().unwrap().len(), 1);

        // A second spelling of one name is refused by name, not by a raw SQLite code
        let same = Tag::new("deep WORK").unwrap();
        assert!(matches!(
            store.save_tag(&same).unwrap_err(),
            StorageError::TagAlreadyExists { .. }
        ));
    }

    #[test]
    fn a_store_that_was_never_migrated_fails_loudly_rather_than_silently() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path().join("calr.sqlite")).unwrap();

        assert!(store.list_calendars().is_err());
        assert!(store.export_events().is_err());
    }
}
