use serde::Serialize;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use crate::config::ConnectionSettings;
use crate::domain::{Calendar, CalendarId, Event, EventStatus, EventTime};

pub const FOUNDATION_MIGRATION: &str = include_str!("../migrations/0001_foundation.sql");

#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "foundation",
    sql: FOUNDATION_MIGRATION,
}];

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

/// PostgreSQL-backed repository for calendar and event creation.
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
}

impl crate::application::AsyncCalendarEventRepository for PostgresCalendarEventRepository {
    type Error = StorageError;

    fn save_calendar<'a>(
        &'a self,
        calendar: &'a Calendar,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Self::Error>> + Send + 'a>>
    {
        Box::pin(async move { Self::save_calendar(self, calendar).await })
    }

    fn save_event<'a>(
        &'a self,
        event: &'a Event,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), Self::Error>> + Send + 'a>>
    {
        Box::pin(async move { Self::save_event(self, event).await })
    }
}

fn event_status(status: EventStatus) -> &'static str {
    match status {
        EventStatus::Tentative => "tentative",
        EventStatus::Confirmed => "confirmed",
        EventStatus::Cancelled => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tokio_postgres::config::Host;

    use super::postgres_config;
    use crate::config::{ConfigSource, ConnectionSettings};

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
}
