use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, FixedOffset, NaiveDate, Offset, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum TodoError {
    #[error("invalid {kind} identifier '{value}': {reason}")]
    InvalidIdentifier {
        kind: &'static str,
        value: String,
        reason: String,
    },
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("{field} must not contain control characters")]
    ControlCharacter { field: &'static str },
    #[error("invalid priority '{value}'; expected none, low, medium, high, or urgent")]
    InvalidPriority { value: String },
    #[error("due value requires an IANA timezone")]
    MissingTimezone,
    #[error("'{timezone}' is not a valid IANA timezone")]
    InvalidTimezone { timezone: String },
    #[error("due value offset does not match IANA timezone '{timezone}' at that instant")]
    OffsetTimezoneMismatch { timezone: String },
}

macro_rules! todo_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl FromStr for $name {
            type Err = TodoError;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value)
                    .map(Self)
                    .map_err(|error| TodoError::InvalidIdentifier {
                        kind: $kind,
                        value: value.to_owned(),
                        reason: error.to_string(),
                    })
            }
        }
    };
}

todo_id!(TodoId, "todo");
todo_id!(ProjectId, "project");
todo_id!(TagId, "tag");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    None,
    Low,
    Medium,
    High,
    Urgent,
}

impl FromStr for Priority {
    type Err = TodoError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "urgent" => Ok(Self::Urgent),
            _ => Err(TodoError::InvalidPriority {
                value: value.to_owned(),
            }),
        }
    }
}
impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Urgent => "urgent",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TodoDue {
    Date {
        date: NaiveDate,
        timezone: String,
    },
    Timed {
        at: DateTime<FixedOffset>,
        timezone: String,
    },
}

impl TodoDue {
    /// Construct a civil-date due value in an IANA timezone.
    ///
    /// # Errors
    /// Returns an error when the timezone is missing or invalid.
    pub fn date(date: NaiveDate, timezone: impl Into<String>) -> Result<Self, TodoError> {
        let timezone = valid_timezone(timezone.into())?;
        Ok(Self::Date { date, timezone })
    }
    /// Construct a zoned timed due value, validating its RFC3339 offset.
    ///
    /// # Errors
    /// Returns an error when the timezone is missing, invalid, or disagrees
    /// with the offset at the supplied instant.
    pub fn timed(
        at: DateTime<FixedOffset>,
        timezone: impl Into<String>,
    ) -> Result<Self, TodoError> {
        let timezone = valid_timezone(timezone.into())?;
        let zone = timezone
            .parse::<Tz>()
            .map_err(|_| TodoError::InvalidTimezone {
                timezone: timezone.clone(),
            })?;
        if at.with_timezone(&zone).offset().fix() != *at.offset() {
            return Err(TodoError::OffsetTimezoneMismatch { timezone });
        }
        Ok(Self::Timed { at, timezone })
    }
    #[must_use]
    pub fn is_all_day(&self) -> bool {
        matches!(self, Self::Date { .. })
    }
}

fn valid_timezone(timezone: String) -> Result<String, TodoError> {
    if timezone.trim().is_empty() {
        return Err(TodoError::MissingTimezone);
    }
    timezone
        .parse::<Tz>()
        .map_err(|_| TodoError::InvalidTimezone {
            timezone: timezone.clone(),
        })?;
    Ok(timezone)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub id: TodoId,
    pub title: String,
    pub due: Option<TodoDue>,
    pub priority: Priority,
    pub project_id: Option<ProjectId>,
    pub tag_ids: Vec<TagId>,
    pub notes: Option<String>,
    pub parent_id: Option<TodoId>,
    pub completed_at: Option<DateTime<Utc>>,
    pub trashed_at: Option<DateTime<Utc>>,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Todo {
    /// Construct a new open todo with no due value or relationships.
    ///
    /// # Errors
    /// Returns an error when the title is empty or contains control characters.
    pub fn new(title: impl Into<String>) -> Result<Self, TodoError> {
        let title = valid_text("todo title", title.into())?;
        let now = Utc::now();
        Ok(Self {
            id: TodoId::new(),
            title,
            due: None,
            priority: Priority::None,
            project_id: None,
            tag_ids: Vec::new(),
            notes: None,
            parent_id: None,
            completed_at: None,
            trashed_at: None,
            version: 1,
            created_at: now,
            updated_at: now,
        })
    }
    /// Revalidate persisted fields before returning a usable todo.
    ///
    /// # Errors
    /// Returns an error when the title is empty or contains control characters.
    pub fn rehydrate(mut self) -> Result<Self, TodoError> {
        self.title = valid_text("todo title", self.title)?;
        if self.version < 1 {
            self.version = 1;
        }
        Ok(self)
    }
    #[must_use]
    pub fn projection(&self, blocked: bool, unmet_prerequisite_count: usize) -> TodoProjection {
        TodoProjection {
            todo: self.clone(),
            blocked,
            unmet_prerequisite_count,
        }
    }
}

fn valid_text(field: &'static str, value: String) -> Result<String, TodoError> {
    if value.trim().is_empty() {
        return Err(TodoError::EmptyField { field });
    }
    if value.chars().any(char::is_control) {
        return Err(TodoError::ControlCharacter { field });
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoProjection {
    pub todo: Todo,
    pub blocked: bool,
    pub unmet_prerequisite_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoDependency {
    pub dependent_id: TodoId,
    pub prerequisite_id: TodoId,
}
