use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Complete identity of one presentation intent.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DeliveryKey {
    pub schedule_ref: ScheduleRef,
    pub occurrence_key: OccurrenceKey,
    pub scheduled_for: DateTime<Utc>,
    pub channel: DeliveryChannel,
}

impl DeliveryKey {
    #[must_use]
    pub const fn new(
        schedule_ref: ScheduleRef,
        occurrence_key: OccurrenceKey,
        scheduled_for: DateTime<Utc>,
        channel: DeliveryChannel,
    ) -> Self {
        Self {
            schedule_ref,
            occurrence_key,
            scheduled_for,
            channel,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScheduleRef(String);

impl ScheduleRef {
    /// Constructs a validated reminder schedule reference.
    ///
    /// # Errors
    ///
    /// Returns an error when the value has an unsupported prefix, trailing
    /// separator, or whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, ReminderDomainError> {
        let value = value.into();
        let valid_prefix =
            value.starts_with("mg-calr:reminder:") || value.starts_with("mg-todo:reminder:");
        if !valid_prefix || value.ends_with(':') || value.chars().any(char::is_whitespace) {
            return Err(ReminderDomainError::InvalidScheduleRef);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OccurrenceKey(String);

impl OccurrenceKey {
    /// Constructs a non-empty occurrence key without whitespace.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is empty or contains whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, ReminderDomainError> {
        let value = value.into();
        if value.is_empty() || value.chars().any(char::is_whitespace) {
            return Err(ReminderDomainError::InvalidOccurrenceKey);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn singleton() -> Self {
        Self("singleton".to_owned())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryChannel {
    Freedesktop,
    Log,
    Null,
}

impl DeliveryChannel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Freedesktop => "freedesktop",
            Self::Log => "log",
            Self::Null => "null",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Claimed,
    Presented,
    Deferred,
    Snoozed,
    Dismissed,
    Folded,
    Expired,
    Failed,
    Revoked,
    UnconfirmedLost,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledReminder {
    pub key: DeliveryKey,
    pub suppressed: bool,
}

/// Pure, repeatable materialization plan. Suppressed schedules never acquire a
/// ledger identity; duplicate source records collapse to one ordered key.
#[must_use]
pub fn plan_deliveries(schedules: impl IntoIterator<Item = ScheduledReminder>) -> Vec<DeliveryKey> {
    let mut keys = schedules
        .into_iter()
        .filter(|schedule| !schedule.suppressed)
        .map(|schedule| schedule.key)
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    keys
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ReminderDomainError {
    #[error("schedule reference must use the mg-calr or mg-todo reminder namespace")]
    InvalidScheduleRef,
    #[error("occurrence key must be non-empty and contain no whitespace")]
    InvalidOccurrenceKey,
}
