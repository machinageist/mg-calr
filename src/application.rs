use crate::domain::{Calendar, CalendarId, DomainError, Event, EventTime};
use thiserror::Error;

/// Transport-independent persistence boundary for calendar/event use cases.
pub trait CalendarEventRepository {
    type Error;

    /// Persist a newly constructed calendar.
    ///
    /// # Errors
    /// Implementations return their transport/storage-specific error.
    fn save_calendar(&mut self, calendar: Calendar) -> Result<(), Self::Error>;
    /// Persist a newly constructed event.
    ///
    /// # Errors
    /// Implementations return their transport/storage-specific error.
    fn save_event(&mut self, event: Event) -> Result<(), Self::Error>;
}

#[derive(Debug, Error)]
pub enum ApplicationError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("repository operation failed: {0}")]
    Repository(E),
}

/// Small application boundary that owns construction and delegates persistence.
/// It has no CLI, SQL, or transport concerns and is straightforward to fake in tests.
pub struct EventUseCases<R> {
    repository: R,
}

impl<R> EventUseCases<R>
where
    R: CalendarEventRepository,
    R::Error: std::error::Error + 'static,
{
    #[must_use]
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }

    /// Construct and persist a calendar through the repository boundary.
    ///
    /// # Errors
    /// Returns a domain validation error or the repository's error.
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

    /// Construct and persist an event through the repository boundary.
    ///
    /// # Errors
    /// Returns a domain validation error or the repository's error.
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

    pub fn into_repository(self) -> R {
        self.repository
    }
}
