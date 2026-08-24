use serde::Serialize;
use thiserror::Error;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use crate::config::ConnectionSettings;

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
