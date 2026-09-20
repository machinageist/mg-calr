#![allow(clippy::unnested_or_patterns)]
pub mod application;
pub mod config;
pub mod domain;
pub mod ics;
pub mod interop;
pub mod notify;
pub mod storage;
pub mod tui;

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    #[error(transparent)]
    Storage(#[from] storage::StorageError),
    #[error(transparent)]
    Domain(#[from] domain::DomainError),
    #[error(transparent)]
    Todo(#[from] domain::todo::TodoError),
    #[error(transparent)]
    Projection(#[from] interop::ProjectionError),
    #[error("required input is missing: {field}")]
    RequiredInput { field: &'static str },
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("event {event_id} was not found")]
    EventNotFound { event_id: domain::EventId },
    #[error(
        "event {event_id} version conflict: expected {expected_version}, actual {actual_version}"
    )]
    EventVersionConflict {
        event_id: domain::EventId,
        expected_version: i64,
        actual_version: i64,
    },
    #[error("todo {todo_id} was not found")]
    TodoNotFound { todo_id: domain::todo::TodoId },
    #[error("could not read interactive input: {0}")]
    Input(#[from] std::io::Error),
    #[error("could not serialize output: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl AppError {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Config(config::ConfigError::InvalidToml(_)) => "config_invalid",
            Self::Config(_) => "config_unavailable",
            Self::Storage(storage::StorageError::InvalidConfiguration(_)) => {
                "database_config_invalid"
            }
            Self::Storage(storage::StorageError::Open(_)) => "database_unavailable",
            Self::Storage(storage::StorageError::MigrationDrift { .. }) => "migration_drift",
            Self::Storage(storage::StorageError::CalendarNotLive { .. }) => "calendar_not_live",
            Self::TodoNotFound { .. } => "todo_not_found",
            Self::Storage(storage::StorageError::ProjectNotFound { .. }) => "project_not_found",
            Self::Storage(storage::StorageError::TagAlreadyExists { .. }) => "tag_already_exists",
            Self::Storage(storage::StorageError::TagNotFound { .. }) => "tag_not_found",
            Self::Storage(storage::StorageError::InvalidStoredData(_)) => "stored_data_invalid",
            Self::Storage(storage::StorageError::ImportInvalid { .. }) => "import_invalid",
            Self::Storage(storage::StorageError::ImportConflict { .. }) => "import_conflict",
            Self::Storage(storage::StorageError::Query(_)) => "database_error",
            Self::Projection(interop::ProjectionError::Missing) => "projection_missing",
            Self::Projection(interop::ProjectionError::Stale(_)) => "projection_stale",
            Self::Projection(interop::ProjectionError::Conflict(_)) => "projection_conflict",
            Self::Projection(interop::ProjectionError::Incomplete(_)) => "projection_incomplete",
            Self::Projection(interop::ProjectionError::Json(_))
            | Self::Projection(interop::ProjectionError::Invalid(_)) => "projection_invalid",
            Self::Projection(interop::ProjectionError::Read(_)) => "projection_unavailable",
            Self::Projection(interop::ProjectionError::Write(_)) => "projection_write_failed",
            Self::Domain(_) | Self::Todo(_) | Self::InvalidInput(_) => "invalid_input",
            Self::RequiredInput { .. } => "required_input_missing",
            Self::EventNotFound { .. }
            | Self::Storage(storage::StorageError::EventNotFound { .. }) => "event_not_found",
            Self::EventVersionConflict { .. }
            | Self::Storage(storage::StorageError::EventVersionConflict { .. }) => {
                "event_version_conflict"
            }

            Self::Input(_) => "input_unavailable",
            Self::Serialization(_) => "serialization_error",
        }
    }

    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::RequiredInput { .. } | Self::Input(_) => 64,
            Self::Domain(_)
            | Self::Todo(_)
            | Self::Projection(interop::ProjectionError::Stale(_))
            | Self::Projection(interop::ProjectionError::Conflict(_))
            | Self::Projection(interop::ProjectionError::Incomplete(_))
            | Self::Projection(interop::ProjectionError::Json(_))
            | Self::Projection(interop::ProjectionError::Invalid(_))
            | Self::InvalidInput(_)
            | Self::Storage(storage::StorageError::TagAlreadyExists { .. })
            | Self::Storage(storage::StorageError::ImportInvalid { .. }) => 65,
            Self::EventNotFound { .. }
            | Self::TodoNotFound { .. }
            | Self::Storage(storage::StorageError::TagNotFound { .. })
            | Self::Storage(storage::StorageError::ProjectNotFound { .. }) => 66,
            Self::Config(_) => 78,
            Self::Projection(interop::ProjectionError::Missing)
            | Self::Projection(interop::ProjectionError::Read(_))
            | Self::Projection(interop::ProjectionError::Write(_)) => 74,
            Self::EventVersionConflict { .. }
            | Self::Storage(storage::StorageError::EventVersionConflict { .. }) => 75,
            Self::Storage(_) => 69,
            Self::Serialization(_) => 70,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub schema_version: u8,
    pub command: &'static str,
    pub ok: bool,
    pub data: T,
}

impl<T: Serialize> Envelope<T> {
    #[must_use]
    pub const fn success(command: &'static str, data: T) -> Self {
        Self {
            schema_version: 1,
            command,
            ok: true,
            data,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorEnvelope<'a> {
    pub schema_version: u8,
    pub ok: bool,
    pub error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody<'a> {
    pub code: &'static str,
    pub message: &'a str,
}
