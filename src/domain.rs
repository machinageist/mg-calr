use std::fmt;
use std::str::FromStr;

use chrono::{
    DateTime, Datelike, Days, FixedOffset, Months, NaiveDate, Offset, TimeZone, Utc, Weekday,
};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub mod reminder;
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
    #[error("recurrence interval must be between 1 and 366")]
    InvalidRecurrenceInterval,
    #[error("recurrence count must be between 1 and {max}")]
    InvalidRecurrenceCount { max: u32 },
    #[error("a recurrence rule must state a count or an until date")]
    UnboundedRecurrence,
    #[error("recurrence until must be after the event start")]
    RecurrenceUntilNotAfterStart,
    #[error("a weekday set is only meaningful for a weekly recurrence")]
    WeekdaySetWithoutWeekly,
    #[error("a weekday set must not be empty or repeat a day")]
    InvalidWeekdaySet,
    #[error("recurrence range start must not be after its end")]
    InvalidRecurrenceRange,
    #[error("a recurring occurrence has no valid local time in '{timezone}'")]
    UnrepresentableOccurrence { timezone: String },
    #[error("'{value}' is not a repeat frequency; use daily, weekly, or monthly")]
    UnknownFrequency { value: String },
    #[error("{field} must be at most {max} characters")]
    TooLong { field: &'static str, max: usize },
    #[error("event URL must start with http://, https://, or mailto: and contain no whitespace")]
    InvalidUrl,
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

/// How often an event repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EventFrequency {
    Daily,
    Weekly,
    Monthly,
}

impl FromStr for EventFrequency {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "daily" | "day" => Ok(Self::Daily),
            "weekly" | "week" => Ok(Self::Weekly),
            "monthly" | "month" => Ok(Self::Monthly),
            _ => Err(DomainError::UnknownFrequency {
                value: value.to_owned(),
            }),
        }
    }
}

/// A bounded repeat rule owned by this application.
///
/// Deliberately separate from the todo `RecurrenceRule`, which is part of the
/// `mg-remindr` projection contract and cannot state a weekday set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecurrence {
    pub frequency: EventFrequency,
    pub interval: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<NaiveDate>,
    /// Weekdays a weekly rule lands on. Empty means the day the event starts on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_weekday: Vec<Weekday>,
}

impl EventRecurrence {
    /// Build a validated repeat rule.
    ///
    /// # Errors
    /// Rejects an interval or count outside its bound, a rule with neither a
    /// count nor an until date, and a weekday set that is empty, repeats a day,
    /// or is attached to a frequency other than weekly.
    pub fn new(
        frequency: EventFrequency,
        interval: u32,
        count: Option<u32>,
        until: Option<NaiveDate>,
        by_weekday: Vec<Weekday>,
    ) -> Result<Self, DomainError> {
        let rule = Self {
            frequency,
            interval,
            count,
            until,
            by_weekday,
        };
        rule.validate()?;
        Ok(rule)
    }

    /// # Errors
    /// Returns the same rejections as [`EventRecurrence::new`].
    pub fn validate(&self) -> Result<(), DomainError> {
        if !(1..=MAX_RECURRENCE_INTERVAL).contains(&self.interval) {
            return Err(DomainError::InvalidRecurrenceInterval);
        }
        if let Some(count) = self.count
            && !(1..=MAX_RECURRENCE_COUNT).contains(&count)
        {
            return Err(DomainError::InvalidRecurrenceCount {
                max: MAX_RECURRENCE_COUNT,
            });
        }
        if self.count.is_none() && self.until.is_none() {
            return Err(DomainError::UnboundedRecurrence);
        }
        if !self.by_weekday.is_empty() {
            if self.frequency != EventFrequency::Weekly {
                return Err(DomainError::WeekdaySetWithoutWeekly);
            }
            let mut seen = self.by_weekday.clone();
            seen.sort_by_key(Weekday::num_days_from_monday);
            seen.dedup();
            if seen.len() != self.by_weekday.len() {
                return Err(DomainError::InvalidWeekdaySet);
            }
        }
        Ok(())
    }

    /// Weekdays this rule lands on, falling back to the day the event starts.
    fn weekdays(&self, start: NaiveDate) -> Vec<Weekday> {
        if self.by_weekday.is_empty() {
            return vec![start.weekday()];
        }
        let mut days = self.by_weekday.clone();
        days.sort_by_key(Weekday::num_days_from_monday);
        days
    }

    /// Expand a base time into the occurrences overlapping one civil-date window.
    ///
    /// Each occurrence keeps the base event's duration, and a timed occurrence
    /// keeps its wall time in its own zone, so a daylight-saving transition moves
    /// the instant rather than the time a person reads.
    ///
    /// # Errors
    /// Rejects an invalid rule, a backwards window, and an occurrence whose local
    /// time does not exist in its zone.
    pub fn expand(
        &self,
        base: &EventTime,
        from: NaiveDate,
        through: NaiveDate,
    ) -> Result<Vec<(u32, EventTime)>, DomainError> {
        if from > through {
            return Err(DomainError::InvalidRecurrenceRange);
        }
        self.validate()?;
        let start = base_start_date(base);
        if self.until.is_some_and(|until| until < start) {
            return Err(DomainError::RecurrenceUntilNotAfterStart);
        }
        let mut found = Vec::new();
        for (index, date) in self.occurrence_dates(start).into_iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            if index >= MAX_RECURRENCE_STEPS {
                break;
            }
            if date > through || self.until.is_some_and(|until| date > until) {
                break;
            }
            if date >= from {
                found.push((index, shift_event_time(base, date)?));
            }
            if self.count.is_some_and(|count| index + 1 >= count) {
                break;
            }
        }
        Ok(found)
    }

    /// Every date this rule lands on, in order, starting at the base date.
    fn occurrence_dates(&self, start: NaiveDate) -> Vec<NaiveDate> {
        let mut dates = Vec::new();
        match self.frequency {
            EventFrequency::Weekly => {
                let weekdays = self.weekdays(start);
                let Some(mut week) = start
                    .checked_sub_days(Days::new(u64::from(start.weekday().num_days_from_monday())))
                else {
                    return dates;
                };
                // A weekday set can name days earlier in the starting week than
                // the event itself; those are not occurrences and take no index.
                while dates.len() < MAX_RECURRENCE_STEPS as usize {
                    let before = dates.len();
                    for weekday in &weekdays {
                        let Some(date) = week
                            .checked_add_days(Days::new(u64::from(weekday.num_days_from_monday())))
                        else {
                            return dates;
                        };
                        if date >= start {
                            dates.push(date);
                        }
                    }
                    let Some(next) = week.checked_add_days(Days::new(u64::from(self.interval) * 7))
                    else {
                        return dates;
                    };
                    if next <= week && before == dates.len() {
                        return dates;
                    }
                    week = next;
                }
            }
            EventFrequency::Daily => {
                let mut date = start;
                while dates.len() < MAX_RECURRENCE_STEPS as usize {
                    dates.push(date);
                    let Some(next) = date.checked_add_days(Days::new(u64::from(self.interval)))
                    else {
                        return dates;
                    };
                    date = next;
                }
            }
            EventFrequency::Monthly => {
                let mut step = 0_u32;
                while dates.len() < MAX_RECURRENCE_STEPS as usize {
                    let Some(date) = start.checked_add_months(Months::new(self.interval * step))
                    else {
                        return dates;
                    };
                    dates.push(date);
                    let Some(next) = step.checked_add(1) else {
                        return dates;
                    };
                    step = next;
                }
            }
        }
        dates
    }
}

/// The civil date an event's own time starts on.
fn base_start_date(base: &EventTime) -> NaiveDate {
    match base {
        EventTime::Timed { start, .. } => start.date_naive(),
        EventTime::AllDay { start, .. } => *start,
    }
}

/// Move one event time onto another date, keeping its duration and wall time.
fn shift_event_time(base: &EventTime, date: NaiveDate) -> Result<EventTime, DomainError> {
    match base {
        EventTime::AllDay {
            start,
            end_exclusive,
        } => {
            let span = (*end_exclusive - *start).num_days().max(1);
            let end = date
                .checked_add_days(Days::new(u64::try_from(span).unwrap_or(1)))
                .ok_or(DomainError::InvalidAllDayRange)?;
            EventTime::all_day(date, end)
        }
        EventTime::Timed {
            start,
            end,
            timezone,
        } => {
            let zone = timezone
                .parse::<Tz>()
                .map_err(|_| DomainError::InvalidTimezone {
                    timezone: timezone.clone(),
                })?;
            let duration = *end - *start;
            let wall = start.with_timezone(&zone).naive_local();
            let local = date.and_time(wall.time());
            let shifted = zone.from_local_datetime(&local).single().ok_or_else(|| {
                DomainError::UnrepresentableOccurrence {
                    timezone: timezone.clone(),
                }
            })?;
            let occurrence_start = shifted.fixed_offset();
            let occurrence_end = occurrence_start + duration;
            EventTime::timed(
                occurrence_start,
                occurrence_end.with_timezone(&zone).fixed_offset(),
                timezone.clone(),
            )
        }
    }
}

const MAX_RECURRENCE_INTERVAL: u32 = 366;
const MAX_RECURRENCE_COUNT: u32 = 1000;
/// Occurrences examined before a rule is treated as runaway, independent of the
/// window asked for, so a far-future query cannot walk forever.
const MAX_RECURRENCE_STEPS: u32 = 4000;

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
    /// The validated repeat rule the agenda expands, if this event repeats.
    pub recurrence_rule: Option<EventRecurrence>,
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

/// Longest event description a person can store, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 10_000;
/// Longest event location, in characters.
pub const MAX_LOCATION_CHARS: usize = 500;
/// Longest event URL, in characters.
pub const MAX_URL_CHARS: usize = 2048;
const URL_SCHEMES: [&str; 3] = ["http://", "https://", "mailto:"];

/// Validate a multi-line event description: newlines and tabs are the only
/// control characters it may hold.
///
/// # Errors
/// Rejects an empty description, one over [`MAX_DESCRIPTION_CHARS`], and any
/// other control character.
pub fn validate_description(value: String) -> Result<String, DomainError> {
    const FIELD: &str = "event description";
    if value.trim().is_empty() {
        return Err(DomainError::EmptyField { field: FIELD });
    }
    if value.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(DomainError::TooLong {
            field: FIELD,
            max: MAX_DESCRIPTION_CHARS,
        });
    }
    if value
        .chars()
        .any(|character| character.is_control() && character != '\n' && character != '\t')
    {
        return Err(DomainError::ControlCharacter { field: FIELD });
    }
    Ok(value)
}

/// Validate a single-line event location.
///
/// # Errors
/// Rejects an empty location, one over [`MAX_LOCATION_CHARS`], and any control
/// character.
pub fn validate_location(value: String) -> Result<String, DomainError> {
    let value = validate_text("event location", value)?;
    if value.chars().count() > MAX_LOCATION_CHARS {
        return Err(DomainError::TooLong {
            field: "event location",
            max: MAX_LOCATION_CHARS,
        });
    }
    Ok(value)
}

/// Validate an event link: http, https, or mailto only, so a card can open it
/// without handing an arbitrary scheme to the desktop.
///
/// # Errors
/// Rejects an unsupported scheme, a bare scheme, whitespace or control
/// characters, and a URL over [`MAX_URL_CHARS`].
pub fn validate_url(value: String) -> Result<String, DomainError> {
    if value.chars().count() > MAX_URL_CHARS {
        return Err(DomainError::TooLong {
            field: "event URL",
            max: MAX_URL_CHARS,
        });
    }
    let lowered = value.to_ascii_lowercase();
    let scheme_ok = URL_SCHEMES
        .iter()
        .any(|scheme| lowered.starts_with(scheme) && lowered.len() > scheme.len());
    if !scheme_ok
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(DomainError::InvalidUrl);
    }
    Ok(value)
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
