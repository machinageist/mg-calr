use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, FixedOffset, NaiveDate, Offset, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub mod todo;
pub use todo::{Project, ProjectId, TodoId};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
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
    #[error("timed events require an IANA timezone")]
    MissingTimezone,
    #[error("'{timezone}' is not a valid IANA timezone")]
    InvalidTimezone { timezone: String },
    #[error(
        "timed event {boundary} offset does not match IANA timezone '{timezone}' at that instant"
    )]
    OffsetTimezoneMismatch {
        boundary: &'static str,
        timezone: String,
    },
    #[error("timed event end must be after start")]
    EndNotAfterStart,
    #[error("all-day event end must be after start and is exclusive")]
    InvalidAllDayRange,
    #[error("RFC UID must be stable and contain no whitespace")]
    InvalidRfcUid,
    #[error("event version must be at least 1")]
    InvalidEventVersion,
}

macro_rules! domain_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(serde::de::Error::custom)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(value)
                    .map(Self)
                    .map_err(|error| DomainError::InvalidIdentifier {
                        kind: $kind,
                        value: value.to_owned(),
                        reason: error.to_string(),
                    })
            }
        }
    };
}

domain_id!(CalendarId, "calendar");
domain_id!(EventId, "event");
domain_id!(ReminderId, "reminder");
domain_id!(DeliveryId, "reminder delivery");
domain_id!(AuditId, "audit record");

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RfcUid(String);

impl RfcUid {
    /// Derive the UID once from the immutable event ID; edits never regenerate it.
    #[must_use]
    pub fn for_event(id: EventId) -> Self {
        Self(format!("{id}@mg-calr.local"))
    }

    /// # Errors
    /// Returns an error when the UID is empty or contains whitespace.
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() || value.chars().any(char::is_whitespace) {
            return Err(DomainError::InvalidRfcUid);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RfcUid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    pub id: CalendarId,
    pub name: String,
    pub color: Option<String>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

impl Calendar {
    /// Create a live calendar with standard lifecycle metadata.
    ///
    /// # Errors
    /// Returns an error for an empty or control-character name.
    pub fn new(name: impl Into<String>) -> Result<Self, DomainError> {
        let name = validate_text("calendar name", name.into())?;
        let now = Utc::now();
        Ok(Self {
            id: CalendarId::new(),
            name,
            color: None,
            is_default: false,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }

    /// Rehydrate a persisted calendar while re-running domain validation.
    ///
    /// # Errors
    /// Returns an error when persisted calendar fields violate domain validation.
    pub fn rehydrate(
        id: CalendarId,
        name: impl Into<String>,
        color: Option<String>,
        is_default: bool,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            id,
            name: validate_text("calendar name", name.into())?,
            color,
            is_default,
            created_at,
            updated_at,
            deleted_at,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventTime {
    Timed {
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
        timezone: String,
    },
    AllDay {
        start: NaiveDate,
        /// Exclusive end date, matching RFC 5545 DATE ranges.
        end_exclusive: NaiveDate,
    },
}

impl EventTime {
    /// # Errors
    /// Rejects missing/unknown IANA zones and non-positive timed ranges.
    pub fn timed(
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
        timezone: impl Into<String>,
    ) -> Result<Self, DomainError> {
        if end <= start {
            return Err(DomainError::EndNotAfterStart);
        }
        let timezone = timezone.into();
        if timezone.trim().is_empty() {
            return Err(DomainError::MissingTimezone);
        }
        let zone = timezone
            .parse::<Tz>()
            .map_err(|_| DomainError::InvalidTimezone {
                timezone: timezone.clone(),
            })?;
        for (boundary, value) in [("start", start), ("end", end)] {
            let expected = value.with_timezone(&zone).offset().fix();
            if expected != *value.offset() {
                return Err(DomainError::OffsetTimezoneMismatch { boundary, timezone });
            }
        }
        Ok(Self::Timed {
            start,
            end,
            timezone,
        })
    }

    /// # Errors
    /// Rejects an empty or backwards all-day half-open range.
    pub fn all_day(start: NaiveDate, end_exclusive: NaiveDate) -> Result<Self, DomainError> {
        if end_exclusive <= start {
            return Err(DomainError::InvalidAllDayRange);
        }
        Ok(Self::AllDay {
            start,
            end_exclusive,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventStatus {
    Tentative,
    Confirmed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alarm {
    pub offset_seconds: Option<i64>,
    pub absolute_at: Option<DateTime<FixedOffset>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventMetadata {
    pub description: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub status: Option<EventStatus>,
    pub busy: bool,
    pub categories: Vec<String>,
    pub recurrence_rule: Option<String>,
    pub alarms: Vec<Alarm>,
    pub organizer: Option<String>,
    pub attendees: Vec<String>,
}

impl Default for EventMetadata {
    fn default() -> Self {
        Self {
            description: None,
            location: None,
            url: None,
            status: None,
            busy: true,
            categories: Vec::new(),
            recurrence_rule: None,
            alarms: Vec::new(),
            organizer: None,
            attendees: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub calendar_id: CalendarId,
    pub rfc_uid: RfcUid,
    pub title: String,
    pub time: EventTime,
    #[serde(flatten)]
    pub metadata: EventMetadata,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub remote_tombstoned_at: Option<DateTime<Utc>>,
    pub version: i64,
}

impl Event {
    /// Create an event; the generated RFC UID is tied to its immutable ID.
    ///
    /// # Errors
    /// Returns an error for an empty/control-character title or invalid time.
    pub fn new(
        calendar_id: CalendarId,
        title: impl Into<String>,
        time: EventTime,
    ) -> Result<Self, DomainError> {
        let title = validate_text("event title", title.into())?;
        let id = EventId::new();
        let now = Utc::now();
        Ok(Self {
            id,
            calendar_id,
            rfc_uid: RfcUid::for_event(id),
            title,
            time,
            metadata: EventMetadata::default(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            remote_tombstoned_at: None,
            version: 1,
        })
    }

    /// Rehydrate a persisted event while re-running title validation.
    ///
    /// # Errors
    /// Returns an error when persisted event fields violate domain validation.
    #[allow(clippy::too_many_arguments)]
    pub fn rehydrate(
        id: EventId,
        calendar_id: CalendarId,
        rfc_uid: RfcUid,
        title: impl Into<String>,
        time: EventTime,
        metadata: EventMetadata,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
        remote_tombstoned_at: Option<DateTime<Utc>>,
        version: i64,
    ) -> Result<Self, DomainError> {
        if version < 1 {
            return Err(DomainError::InvalidEventVersion);
        }
        Ok(Self {
            id,
            calendar_id,
            rfc_uid,
            title: validate_text("event title", title.into())?,
            time,
            metadata,
            created_at,
            updated_at,
            deleted_at,
            remote_tombstoned_at,
            version,
        })
    }
}

fn validate_text(field: &'static str, value: String) -> Result<String, DomainError> {
    if value.trim().is_empty() {
        return Err(DomainError::EmptyField { field });
    }
    if value.chars().any(char::is_control) {
        return Err(DomainError::ControlCharacter { field });
    }
    Ok(value)
}
