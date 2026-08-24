use std::cmp::Ordering;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, FixedOffset, LocalResult, NaiveDate, TimeZone};
use chrono_tz::Tz;
use serde::Serialize;
use thiserror::Error;

use crate::domain::{Calendar, CalendarId, DomainError, Event, EventId, EventTime};

/// Boxed asynchronous repository operation used at the transport boundary.
pub type RepositoryFuture<'a, T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send + 'a>>;

/// Transport-independent persistence boundary used by small synchronous tests.
pub trait CalendarEventRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_calendar(&mut self, calendar: Calendar) -> Result<(), Self::Error>;
    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_event(&mut self, event: Event) -> Result<(), Self::Error>;
}

/// Asynchronous persistence/query boundary implemented by PostgreSQL.
pub trait AsyncCalendarEventRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_calendar<'a>(&'a self, calendar: &'a Calendar)
    -> RepositoryFuture<'a, (), Self::Error>;
    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_event<'a>(&'a self, event: &'a Event) -> RepositoryFuture<'a, (), Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_calendars(&self) -> RepositoryFuture<'_, Vec<Calendar>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn find_event(&self, id: EventId) -> RepositoryFuture<'_, Option<Event>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_events(
        &self,
        calendar_id: Option<CalendarId>,
    ) -> RepositoryFuture<'_, Vec<Event>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn day_agenda(
        &self,
        date: NaiveDate,
        timezone: &str,
        starts_at: DateTime<FixedOffset>,
        ends_at: DateTime<FixedOffset>,
    ) -> RepositoryFuture<'_, Vec<Event>, Self::Error>;
}

#[derive(Debug, Error)]
pub enum ApplicationError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("repository operation failed: {0}")]
    Repository(E),
}

#[derive(Debug, Error)]
pub enum QueryError<E: std::error::Error + 'static> {
    #[error("event {event_id} was not found")]
    EventNotFound { event_id: EventId },
    #[error("'{timezone}' is not a valid IANA timezone")]
    InvalidTimezone { timezone: String },
    #[error("the local day boundary for {date} is not representable in {timezone}")]
    InvalidDayBoundary { date: NaiveDate, timezone: String },
    #[error("repository operation failed: {0}")]
    Repository(E),
}

/// Stable query projection consumed by both human and JSON renderers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarProjection {
    pub id: CalendarId,
    pub name: String,
    pub color: Option<String>,
    pub is_default: bool,
}

impl From<Calendar> for CalendarProjection {
    fn from(calendar: Calendar) -> Self {
        Self {
            id: calendar.id,
            name: calendar.name,
            color: calendar.color,
            is_default: calendar.is_default,
        }
    }
}

impl fmt::Display for CalendarProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}\t{}", self.id, self.name)?;
        if self.is_default {
            formatter.write_str("\t(default)")?;
        }
        Ok(())
    }
}

/// Stable event query projection consumed by both human and JSON renderers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventProjection {
    pub id: EventId,
    pub calendar_id: CalendarId,
    pub title: String,
    pub time: EventTime,
}

impl From<Event> for EventProjection {
    fn from(event: Event) -> Self {
        Self {
            id: event.id,
            calendar_id: event.calendar_id,
            title: event.title,
            time: event.time,
        }
    }
}

impl fmt::Display for EventProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.time {
            EventTime::Timed {
                start,
                end,
                timezone,
            } => write!(
                formatter,
                "{}\t{}\t{}..{}\t{}",
                self.id, self.title, start, end, timezone
            ),
            EventTime::AllDay {
                start,
                end_exclusive,
            } => write!(
                formatter,
                "{}\t{}\t{}..{}\tall-day",
                self.id, self.title, start, end_exclusive
            ),
        }
    }
}

pub struct EventUseCases<R> {
    repository: R,
}

impl<R> EventUseCases<R> {
    #[must_use]
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn into_repository(self) -> R {
        self.repository
    }
}

impl<R> EventUseCases<R>
where
    R: CalendarEventRepository,
    R::Error: std::error::Error + 'static,
{
    /// # Errors
    /// Returns domain validation or synchronous repository persistence errors.
    pub fn create_calendar(
        &mut self,
        name: impl Into<String>,
    ) -> Result<Calendar, ApplicationError<R::Error>> {
        let calendar = Calendar::new(name)?;
        self.repository
            .save_calendar(calendar.clone())
            .map_err(ApplicationError::Repository)?;
        Ok(calendar)
    }

    /// # Errors
    /// Returns domain validation or synchronous repository persistence errors.
    pub fn create_event(
        &mut self,
        calendar_id: CalendarId,
        title: impl Into<String>,
        time: EventTime,
    ) -> Result<Event, ApplicationError<R::Error>> {
        let event = Event::new(calendar_id, title, time)?;
        self.repository
            .save_event(event.clone())
            .map_err(ApplicationError::Repository)?;
        Ok(event)
    }
}

impl<R> EventUseCases<R>
where
    R: AsyncCalendarEventRepository,
    R::Error: std::error::Error + 'static,
{
    /// # Errors
    /// Returns domain validation or asynchronous repository persistence errors.
    pub async fn create_calendar_async(
        &self,
        name: impl Into<String>,
    ) -> Result<Calendar, ApplicationError<R::Error>> {
        let calendar = Calendar::new(name)?;
        self.repository
            .save_calendar(&calendar)
            .await
            .map_err(ApplicationError::Repository)?;
        Ok(calendar)
    }

    /// # Errors
    /// Returns domain validation or asynchronous repository persistence errors.
    pub async fn create_event_async(
        &self,
        calendar_id: CalendarId,
        title: impl Into<String>,
        time: EventTime,
    ) -> Result<Event, ApplicationError<R::Error>> {
        let event = Event::new(calendar_id, title, time)?;
        self.repository
            .save_event(&event)
            .await
            .map_err(ApplicationError::Repository)?;
        Ok(event)
    }

    /// # Errors
    /// Returns a typed repository query error.
    pub async fn list_calendars_async(
        &self,
    ) -> Result<Vec<CalendarProjection>, QueryError<R::Error>> {
        let mut items = self
            .repository
            .list_calendars()
            .await
            .map_err(QueryError::Repository)?;
        items.sort_by_cached_key(|calendar| (calendar.name.to_lowercase(), calendar.id.as_uuid()));
        Ok(items.into_iter().map(CalendarProjection::from).collect())
    }

    /// # Errors
    /// Returns a repository query error or [`QueryError::EventNotFound`].
    pub async fn show_event_async(
        &self,
        event_id: EventId,
    ) -> Result<EventProjection, QueryError<R::Error>> {
        self.repository
            .find_event(event_id)
            .await
            .map_err(QueryError::Repository)?
            .map(EventProjection::from)
            .ok_or(QueryError::EventNotFound { event_id })
    }

    /// # Errors
    /// Returns a typed repository query error.
    pub async fn list_events_async(
        &self,
        calendar_id: Option<CalendarId>,
    ) -> Result<Vec<EventProjection>, QueryError<R::Error>> {
        let mut items = self
            .repository
            .list_events(calendar_id)
            .await
            .map_err(QueryError::Repository)?;
        items.sort_by(compare_events);
        Ok(items.into_iter().map(EventProjection::from).collect())
    }

    /// # Errors
    /// Returns a timezone/day-boundary validation error or repository query error.
    pub async fn day_agenda_async(
        &self,
        date: NaiveDate,
        timezone: &str,
    ) -> Result<Vec<EventProjection>, QueryError<R::Error>> {
        let zone = timezone
            .parse::<Tz>()
            .map_err(|_| QueryError::InvalidTimezone {
                timezone: timezone.to_owned(),
            })?;
        let next_date = date
            .succ_opt()
            .ok_or_else(|| QueryError::InvalidDayBoundary {
                date,
                timezone: timezone.to_owned(),
            })?;
        let starts_at = local_midnight(zone, date, timezone)?;
        let ends_at = local_midnight(zone, next_date, timezone)?;
        let mut items = self
            .repository
            .day_agenda(date, timezone, starts_at, ends_at)
            .await
            .map_err(QueryError::Repository)?;
        items.sort_by(compare_events);
        Ok(items.into_iter().map(EventProjection::from).collect())
    }
}

fn compare_events(left: &Event, right: &Event) -> Ordering {
    let temporal = match (&left.time, &right.time) {
        (EventTime::AllDay { start: left, .. }, EventTime::AllDay { start: right, .. }) => {
            left.cmp(right)
        }
        (EventTime::AllDay { .. }, EventTime::Timed { .. }) => Ordering::Less,
        (EventTime::Timed { .. }, EventTime::AllDay { .. }) => Ordering::Greater,
        (EventTime::Timed { start: left, .. }, EventTime::Timed { start: right, .. }) => {
            left.cmp(right)
        }
    };
    temporal
        .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
        .then_with(|| left.id.as_uuid().cmp(&right.id.as_uuid()))
}

fn local_midnight<E: std::error::Error + 'static>(
    zone: Tz,
    date: NaiveDate,
    timezone: &str,
) -> Result<DateTime<FixedOffset>, QueryError<E>> {
    let local = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| QueryError::InvalidDayBoundary {
            date,
            timezone: timezone.to_owned(),
        })?;
    match zone.from_local_datetime(&local) {
        LocalResult::Single(value) => Ok(value.fixed_offset()),
        LocalResult::Ambiguous(first, _) => Ok(first.fixed_offset()),
        LocalResult::None => Err(QueryError::InvalidDayBoundary {
            date,
            timezone: timezone.to_owned(),
        }),
    }
}
