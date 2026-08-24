#![allow(clippy::missing_errors_doc)]
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Duration, FixedOffset, Months, NaiveDate, Offset, Utc};
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
    #[error("invalid persisted project data: {reason}")]
    InvalidStoredProject { reason: String },
    #[error("invalid persisted tag data: {reason}")]
    InvalidStoredTag { reason: String },
    #[error("invalid priority '{value}'; expected none, low, medium, high, or urgent")]
    InvalidPriority { value: String },
    #[error("due value requires an IANA timezone")]
    MissingTimezone,
    #[error("'{timezone}' is not a valid IANA timezone")]
    InvalidTimezone { timezone: String },
    #[error("due value offset does not match IANA timezone '{timezone}' at that instant")]
    OffsetTimezoneMismatch { timezone: String },
    #[error("recurrence interval must be between 1 and 366")]
    InvalidRecurrenceInterval,
    #[error("recurrence count must be between 1 and 1000")]
    InvalidRecurrenceCount,
    #[error("recurrence until must be after the todo due value")]
    RecurrenceUntilNotAfterDue,
    #[error("recurrence rule requires a due value")]
    RecurrenceWithoutDue,
    #[error("recurrence expansion range is invalid")]
    InvalidRecurrenceRange,
    #[error("stored recurrence rule is invalid: {reason}")]
    InvalidStoredRecurrence { reason: String },
    #[error("reminder offset must be between 1 and 10080 minutes")]
    InvalidReminderOffset,
    #[error("reminders require a due value")]
    ReminderWithoutDue,
    #[error("stored reminder data is invalid: {reason}")]
    InvalidStoredReminder { reason: String },
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
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: TagId,
    pub name: String,
    pub normalized_name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Tag {
    pub fn new(name: impl Into<String>) -> Result<Self, TodoError> {
        let name = valid_text("tag name", name.into())?;
        let now = Utc::now();
        Ok(Self {
            id: TagId::new(),
            normalized_name: name.to_lowercase(),
            name,
            created_at: now,
            updated_at: now,
        })
    }
    pub fn rehydrate(mut self) -> Result<Self, TodoError> {
        self.name = valid_text("tag name", self.name)?;
        if self.normalized_name != self.name.to_lowercase() {
            return Err(TodoError::InvalidStoredTag {
                reason: "tag normalized name does not match name".to_owned(),
            });
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub normalized_name: String,
    pub archived_at: Option<DateTime<Utc>>,
    pub version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Project {
    /// Create a live project with a deterministic normalized-name key.
    ///
    /// # Errors
    /// Returns an error when the name is empty or contains control characters.
    pub fn new(name: impl Into<String>) -> Result<Self, TodoError> {
        let name = valid_text("project name", name.into())?;
        let now = Utc::now();
        Ok(Self {
            normalized_name: name.to_lowercase(),
            id: ProjectId::new(),
            name,
            archived_at: None,
            version: 1,
            created_at: now,
            updated_at: now,
        })
    }

    /// Revalidate a project loaded from persistence.
    ///
    /// # Errors
    /// Returns an error when the persisted name is invalid.
    pub fn rehydrate(mut self) -> Result<Self, TodoError> {
        self.name = valid_text("project name", self.name)?;
        let expected_normalized_name = self.name.to_lowercase();
        if self.normalized_name != expected_normalized_name {
            return Err(TodoError::InvalidStoredProject {
                reason: "normalized name does not match name".to_owned(),
            });
        }
        if self.version < 1 {
            return Err(TodoError::InvalidStoredProject {
                reason: "version must be at least 1".to_owned(),
            });
        }
        Ok(self)
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RecurrenceFrequency {
    Daily,
    Weekly,
    Monthly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecurrenceRule {
    pub frequency: RecurrenceFrequency,
    pub interval: u32,
    pub count: Option<u32>,
    pub until: Option<NaiveDate>,
}

impl RecurrenceRule {
    pub fn new(
        frequency: RecurrenceFrequency,
        interval: u32,
        count: Option<u32>,
        until: Option<NaiveDate>,
    ) -> Result<Self, TodoError> {
        let rule = Self {
            frequency,
            interval,
            count,
            until,
        };
        rule.validate()?;
        Ok(rule)
    }

    pub fn validate(&self) -> Result<(), TodoError> {
        if !(1..=366).contains(&self.interval) {
            return Err(TodoError::InvalidRecurrenceInterval);
        }
        if self.count.is_some_and(|count| !(1..=1000).contains(&count)) {
            return Err(TodoError::InvalidRecurrenceCount);
        }
        if self.count.is_none() && self.until.is_none() {
            return Err(TodoError::InvalidRecurrenceCount);
        }
        Ok(())
    }
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
    pub recurrence: Option<RecurrenceRule>,
    pub reminders: Vec<TodoReminder>,
    pub priority: Priority,
    pub project_id: Option<ProjectId>,
    pub tag_ids: Vec<TagId>,
    pub dependency_ids: Vec<TodoId>,
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
            recurrence: None,
            reminders: Vec::new(),
            priority: Priority::None,
            project_id: None,
            tag_ids: Vec::new(),
            dependency_ids: Vec::new(),
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
        if let Some(rule) = &self.recurrence {
            rule.validate()?;
            validate_recurrence_due(self.due.as_ref(), rule)?;
        }
        validate_reminders(self.due.as_ref(), &self.reminders)?;
        if self.version < 1 {
            self.version = 1;
        }
        Ok(self)
    }

    /// Expand due instances in a bounded inclusive date range. The stored base
    /// todo is never changed.
    pub fn expand_due_instances(
        &self,
        from: NaiveDate,
        through: NaiveDate,
    ) -> Result<Vec<TodoDue>, TodoError> {
        Ok(self
            .expand_due_instances_indexed(from, through)?
            .into_iter()
            .map(|(_, due)| due)
            .collect())
    }

    pub fn expand_due_instances_indexed(
        &self,
        from: NaiveDate,
        through: NaiveDate,
    ) -> Result<Vec<(u32, TodoDue)>, TodoError> {
        if from > through {
            return Err(TodoError::InvalidRecurrenceRange);
        }
        let Some(due) = &self.due else {
            return Ok(Vec::new());
        };
        let Some(rule) = &self.recurrence else {
            return Ok(if from <= due_date(due) && due_date(due) <= through {
                vec![(0, due.clone())]
            } else {
                Vec::new()
            });
        };
        rule.validate()?;
        validate_recurrence_due(Some(due), rule)?;
        let mut result = Vec::new();
        let mut current = due.clone();
        for occurrence in 0..=1000_u32 {
            let date = due_date(&current);
            if date > through || rule.until.is_some_and(|until| date > until) {
                break;
            }
            if date >= from {
                result.push((occurrence, current.clone()));
            }
            if rule.count.is_some_and(|count| occurrence + 1 >= count) {
                break;
            }
            current = add_recurrence_step(&current, rule)?;
        }
        Ok(result)
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

fn due_date(due: &TodoDue) -> NaiveDate {
    match due {
        TodoDue::Date { date, .. } => *date,
        TodoDue::Timed { at, .. } => at.date_naive(),
    }
}

fn validate_recurrence_due(due: Option<&TodoDue>, rule: &RecurrenceRule) -> Result<(), TodoError> {
    let Some(due) = due else {
        return Err(TodoError::RecurrenceWithoutDue);
    };
    if rule.until.is_some_and(|until| until <= due_date(due)) {
        return Err(TodoError::RecurrenceUntilNotAfterDue);
    }
    Ok(())
}

fn add_recurrence_step(due: &TodoDue, rule: &RecurrenceRule) -> Result<TodoDue, TodoError> {
    let amount = rule.interval;
    match due {
        TodoDue::Date { date, timezone } => {
            let next = match rule.frequency {
                RecurrenceFrequency::Daily => {
                    date.checked_add_signed(Duration::days(i64::from(amount)))
                }
                RecurrenceFrequency::Weekly => {
                    date.checked_add_signed(Duration::weeks(i64::from(amount)))
                }
                RecurrenceFrequency::Monthly => date.checked_add_months(Months::new(amount)),
            }
            .ok_or(TodoError::InvalidRecurrenceRange)?;
            TodoDue::date(next, timezone.clone())
        }
        TodoDue::Timed { at, timezone } => {
            let zone = timezone
                .parse::<Tz>()
                .map_err(|_| TodoError::InvalidTimezone {
                    timezone: timezone.clone(),
                })?;
            let local = at.with_timezone(&zone);
            let next_local = match rule.frequency {
                RecurrenceFrequency::Daily => {
                    local.checked_add_signed(Duration::days(i64::from(amount)))
                }
                RecurrenceFrequency::Weekly => {
                    local.checked_add_signed(Duration::weeks(i64::from(amount)))
                }
                RecurrenceFrequency::Monthly => local.checked_add_months(Months::new(amount)),
            }
            .ok_or(TodoError::InvalidRecurrenceRange)?;
            TodoDue::timed(
                next_local.with_timezone(&next_local.offset().fix()),
                timezone.clone(),
            )
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoReminder {
    pub minutes_before: u32,
    pub repeatable: bool,
}

impl TodoReminder {
    pub fn new(minutes_before: u32, repeatable: bool) -> Result<Self, TodoError> {
        if !(1..=10_080).contains(&minutes_before) {
            return Err(TodoError::InvalidReminderOffset);
        }
        Ok(Self {
            minutes_before,
            repeatable,
        })
    }
}

fn validate_reminders(due: Option<&TodoDue>, reminders: &[TodoReminder]) -> Result<(), TodoError> {
    if !reminders.is_empty() && due.is_none() {
        return Err(TodoError::ReminderWithoutDue);
    }
    let mut seen = HashSet::new();
    for reminder in reminders {
        TodoReminder::new(reminder.minutes_before, reminder.repeatable)?;
        if !seen.insert((reminder.minutes_before, reminder.repeatable)) {
            return Err(TodoError::InvalidStoredReminder {
                reason: "duplicate reminder".to_owned(),
            });
        }
    }
    Ok(())
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
