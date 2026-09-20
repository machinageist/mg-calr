#![allow(clippy::missing_errors_doc)]
use std::cmp::Ordering;
use std::convert::Infallible;
use std::fmt;

use chrono::{DateTime, Duration, FixedOffset, LocalResult, NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;
use serde::Serialize;
use thiserror::Error;

use crate::domain::{
    Alarm, Calendar, CalendarId, DomainError, Event, EventId, EventRecurrence, EventStatus,
    EventTime,
    todo::{Priority, Project, ProjectId, Tag, TagId, Todo, TodoDue, TodoId, TodoReminder},
    validate_description, validate_location, validate_url,
};

/// Persistence and query boundary for calendars and events, served by the store.
///
/// It was asynchronous while the transport was a PostgreSQL connection. A SQLite store is
/// a file this process opens itself, so every call here returns its answer directly.
pub trait CalendarEventRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_calendar(&self, calendar: &Calendar) -> Result<(), Self::Error>;
    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_event(&self, event: &Event) -> Result<(), Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_calendars(&self) -> Result<Vec<Calendar>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn find_event(&self, id: EventId) -> Result<Option<Event>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_events(&self, calendar_id: Option<CalendarId>) -> Result<Vec<Event>, Self::Error>;
    /// # Errors
    /// Returns a typed repository lifecycle error.
    fn cancel_event(&self, id: EventId, expected_version: i64) -> Result<Event, Self::Error>;
    /// # Errors
    /// Returns a typed repository lifecycle error.
    fn restore_event(&self, id: EventId, expected_version: i64) -> Result<Event, Self::Error>;
    /// # Errors
    /// Returns a typed repository lifecycle error.
    fn edit_event(
        &self,
        id: EventId,
        expected_version: i64,
        edit: &EventEdit,
    ) -> Result<Event, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn day_agenda(
        &self,
        date: NaiveDate,
        timezone: &str,
        starts_at: DateTime<FixedOffset>,
        ends_at: DateTime<FixedOffset>,
    ) -> Result<Vec<Event>, Self::Error>;
}

/// One optional field in an edit: leave it, set it, or clear it.
///
/// Clearing is a distinct intention from leaving a field alone, which is why
/// this is not an `Option`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Change<T> {
    #[default]
    Keep,
    Set(T),
    Clear,
}

impl<T: Clone> Change<T> {
    #[must_use]
    pub const fn is_keep(&self) -> bool {
        matches!(self, Self::Keep)
    }

    /// The value a field holds after this change is applied to `current`.
    #[must_use]
    pub fn resolve(&self, current: Option<&T>) -> Option<T> {
        match self {
            Self::Keep => current.cloned(),
            Self::Set(value) => Some(value.clone()),
            Self::Clear => None,
        }
    }
}

/// Explicitly supplied fields for one optimistic event edit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventEdit {
    pub title: Option<String>,
    pub time: Option<EventTime>,
    /// Move the event to another live calendar, keeping its identity and history.
    pub calendar_id: Option<CalendarId>,
    pub description: Change<String>,
    pub location: Change<String>,
    pub url: Change<String>,
    pub busy: Option<bool>,
}

impl EventEdit {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.time.is_none()
            && self.calendar_id.is_none()
            && self.description.is_keep()
            && self.location.is_keep()
            && self.url.is_keep()
            && self.busy.is_none()
    }

    /// Check every supplied value on its own, before any stored event is read.
    ///
    /// # Errors
    /// Returns the first field that fails domain validation.
    pub fn validate(&self) -> Result<(), DomainError> {
        if let Some(title) = &self.title {
            validate_title(title)?;
        }
        if let Some(time) = &self.time {
            revalidate_time(time)?;
        }
        if let Change::Set(value) = &self.description {
            validate_description(value.clone())?;
        }
        if let Change::Set(value) = &self.location {
            validate_location(value.clone())?;
        }
        if let Change::Set(value) = &self.url {
            validate_url(value.clone())?;
        }
        Ok(())
    }

    /// The event this edit produces from `current`, fully validated.
    ///
    /// A repeat rule is re-checked against the resulting time, so changing an
    /// event's start cannot leave a rule that no longer produces its first
    /// occurrence.
    ///
    /// # Errors
    /// Returns a domain error for any invalid value or combination.
    pub fn apply_to(&self, current: &Event) -> Result<Event, DomainError> {
        self.validate()?;
        let mut next = current.clone();
        if let Some(title) = &self.title {
            next.title.clone_from(title);
        }
        if let Some(time) = &self.time {
            next.time = time.clone();
        }
        if let Some(calendar_id) = self.calendar_id {
            next.calendar_id = calendar_id;
        }
        next.metadata.description = self
            .description
            .resolve(current.metadata.description.as_ref());
        next.metadata.location = self.location.resolve(current.metadata.location.as_ref());
        next.metadata.url = self.url.resolve(current.metadata.url.as_ref());
        if let Some(busy) = self.busy {
            next.metadata.busy = busy;
        }
        if let Some(rule) = next.metadata.recurrence_rule.take() {
            next.metadata.recurrence_rule = Some(validated_rule(rule, &next.time)?);
        }
        Ok(next)
    }
}

/// Optional detail supplied when an event is created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDetails {
    pub description: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub busy: bool,
    pub recurrence: Option<EventRecurrence>,
}

impl Default for EventDetails {
    fn default() -> Self {
        Self {
            description: None,
            location: None,
            url: None,
            busy: true,
            recurrence: None,
        }
    }
}

impl EventDetails {
    /// Write validated detail onto a freshly created event.
    ///
    /// # Errors
    /// Returns a domain error for any invalid value.
    pub fn apply_to(self, event: &mut Event) -> Result<(), DomainError> {
        event.metadata.description = self.description.map(validate_description).transpose()?;
        event.metadata.location = self.location.map(validate_location).transpose()?;
        event.metadata.url = self.url.map(validate_url).transpose()?;
        event.metadata.busy = self.busy;
        if let Some(rule) = self.recurrence {
            event.metadata.recurrence_rule = Some(validated_rule(rule, &event.time)?);
        }
        Ok(())
    }
}

/// Run the same title check event creation runs.
fn validate_title(title: &str) -> Result<(), DomainError> {
    Event::new(
        CalendarId::new(),
        title.to_owned(),
        EventTime::all_day(
            NaiveDate::MIN,
            NaiveDate::MIN.succ_opt().unwrap_or(NaiveDate::MAX),
        )?,
    )
    .map(|_| ())
}

/// Re-run the constructor checks on a time that may have been deserialized.
fn revalidate_time(time: &EventTime) -> Result<(), DomainError> {
    match time {
        EventTime::Timed {
            start,
            end,
            timezone,
        } => EventTime::timed(*start, *end, timezone.clone()).map(|_| ()),
        EventTime::AllDay {
            start,
            end_exclusive,
        } => EventTime::all_day(*start, *end_exclusive).map(|_| ()),
    }
}

/// Read-only persistence boundary for one combined agenda snapshot.
pub trait AgendaRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed query error.
    fn agenda_events(&self, include_trashed: bool) -> Result<Vec<Event>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn agenda_todos(&self, include_trashed: bool) -> Result<AgendaTodoSnapshot, Self::Error>;
}

/// Persistence boundary for tag metadata.
pub trait TagRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_tag(&self, tag: &Tag) -> Result<(), Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_tags(&self) -> Result<Vec<Tag>, Self::Error>;
}

/// Persistence boundary for project metadata.
pub trait ProjectRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_project(&self, project: &Project) -> Result<(), Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn find_project(&self, id: ProjectId) -> Result<Option<Project>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_projects(&self) -> Result<Vec<Project>, Self::Error>;
}

#[derive(Debug, Error)]
pub enum ApplicationError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error(transparent)]
    Todo(#[from] crate::domain::todo::TodoError),
    #[error("repository operation failed: {0}")]
    Repository(E),
}

#[derive(Debug, Error)]
pub enum QueryError<E: std::error::Error + 'static> {
    #[error("event {event_id} was not found")]
    EventNotFound { event_id: EventId },
    #[error("todo {todo_id} was not found")]
    TodoNotFound { todo_id: TodoId },
    #[error("project {project_id} was not found")]
    ProjectNotFound { project_id: ProjectId },
    #[error("'{timezone}' is not a valid IANA timezone")]
    InvalidTimezone { timezone: String },
    #[error("the local day boundary for {date} is not representable in {timezone}")]
    InvalidDayBoundary { date: NaiveDate, timezone: String },
    #[error("repository operation failed: {0}")]
    Repository(E),
    #[error(transparent)]
    Domain(#[from] crate::domain::todo::TodoError),
    #[error(transparent)]
    EventDomain(#[from] crate::domain::DomainError),
}

#[derive(Debug, Error)]
pub enum EventLifecycleError<E: std::error::Error + 'static> {
    #[error("event {event_id} was not found")]
    NotFound { event_id: EventId },
    #[error(
        "event {event_id} version conflict: expected {expected_version}, actual {actual_version}"
    )]
    VersionConflict {
        event_id: EventId,
        expected_version: i64,
        actual_version: i64,
    },
    #[error("repository operation failed: {0}")]
    Repository(E),
}

/// Map repository lifecycle failures to stable application outcomes.
pub trait EventLifecycleErrorMapping: std::error::Error + Sized + 'static {
    fn map_event_lifecycle_error(
        self,
        event_id: EventId,
        expected_version: i64,
    ) -> EventLifecycleError<Self>;
}

/// Stable project query projection consumed by both human and JSON renderers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectProjection {
    pub id: ProjectId,
    pub name: String,
    pub archived_at: Option<DateTime<chrono::Utc>>,
    pub version: i64,
    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}

impl From<Project> for ProjectProjection {
    fn from(project: Project) -> Self {
        Self {
            id: project.id,
            name: project.name,
            archived_at: project.archived_at,
            version: project.version,
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

impl fmt::Display for ProjectProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}\t{}", self.id, self.name)
    }
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
///
/// Carries every field an editing surface needs, so a card can read one event
/// and send back only what changed with the version it read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventProjection {
    pub id: EventId,
    pub calendar_id: CalendarId,
    pub title: String,
    pub time: EventTime,
    pub version: i64,
    pub cancelled: bool,
    pub description: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
    pub status: Option<EventStatus>,
    pub busy: bool,
    pub recurrence: Option<EventRecurrence>,
    pub alarms: Vec<Alarm>,
    pub categories: Vec<String>,
    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}

impl From<Event> for EventProjection {
    fn from(event: Event) -> Self {
        Self {
            id: event.id,
            calendar_id: event.calendar_id,
            title: event.title,
            time: event.time,
            version: event.version,
            cancelled: event.deleted_at.is_some(),
            description: event.metadata.description,
            location: event.metadata.location,
            url: event.metadata.url,
            status: event.metadata.status,
            busy: event.metadata.busy,
            recurrence: event.metadata.recurrence_rule,
            alarms: event.metadata.alarms,
            categories: event.metadata.categories,
            created_at: event.created_at,
            updated_at: event.updated_at,
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

/// Stable query projection for todo output. Recurrence, dependency, tag, and
/// hierarchy state are included when present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TodoQueryProjection {
    pub id: TodoId,
    pub title: String,
    pub due: Option<TodoDue>,
    pub recurrence: Option<crate::domain::todo::RecurrenceRule>,
    pub reminders: Vec<TodoReminder>,
    pub priority: crate::domain::todo::Priority,
    pub project_id: Option<crate::domain::todo::ProjectId>,
    pub tag_ids: Vec<TagId>,
    pub dependency_ids: Vec<TodoId>,
    pub notes: Option<String>,
    pub parent_id: Option<TodoId>,
    pub completed_at: Option<DateTime<chrono::Utc>>,
    pub trashed_at: Option<DateTime<chrono::Utc>>,
    pub version: i64,
    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}

/// The source kind of one agenda row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AgendaKind {
    Event,
    Todo,
}

/// Producer diagnostic retained on every projection-backed agenda response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectionDiagnostic {
    pub severity: String,
    pub code: String,
    pub message: String,
}

/// Truthful coverage facts for the validated projection content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectionCompleteness {
    pub complete: bool,
    pub record_count: usize,
    pub todo_count: usize,
    pub link_count: usize,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Immutable projection identity attached to combined agenda output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TodoProjectionMetadata {
    pub producer: String,
    pub producer_version: String,
    pub producer_revision: u64,
    pub source_revision: String,
    pub content_revision: String,
    pub created_at: DateTime<chrono::Utc>,
    pub completeness: ProjectionCompleteness,
}

/// Projection-backed todo rows and the exact envelope that authorized them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaTodoSnapshot {
    pub todos: Vec<Todo>,
    pub metadata: TodoProjectionMetadata,
}

/// A normalized, stable row in a combined agenda result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgendaItem {
    pub kind: AgendaKind,
    pub id: String,
    pub title: String,
    pub due: Option<TodoDue>,
    pub event_time: Option<EventTime>,
    pub priority: Option<Priority>,
    pub occurrence_index: Option<u32>,
    pub completed: bool,
    pub trashed: bool,
    pub blocked: bool,
}

impl AgendaItem {
    /// When this item falls, expressed in the zone the agenda was queried in.
    #[must_use]
    pub fn when(&self, zone: Tz) -> String {
        if let Some(due) = &self.due {
            return match due {
                TodoDue::Date { .. } => "all-day".to_owned(),
                TodoDue::Timed { at, .. } => at.with_timezone(&zone).format("%H:%M").to_string(),
            };
        }
        match &self.event_time {
            None => "unscheduled".to_owned(),
            Some(EventTime::AllDay { .. }) => "all-day".to_owned(),
            Some(EventTime::Timed { start, end, .. }) => format!(
                "{}-{}",
                start.with_timezone(&zone).format("%H:%M"),
                end.with_timezone(&zone).format("%H:%M")
            ),
        }
    }

    /// The civil date this item belongs to in one zone.
    #[must_use]
    pub fn on(&self, zone: Tz) -> Option<NaiveDate> {
        if let Some(due) = &self.due {
            return Some(match due {
                TodoDue::Date { date, .. } => *date,
                TodoDue::Timed { at, .. } => at.with_timezone(&zone).date_naive(),
            });
        }
        match &self.event_time {
            None => None,
            Some(EventTime::AllDay { start, .. }) => Some(*start),
            Some(EventTime::Timed { start, .. }) => Some(start.with_timezone(&zone).date_naive()),
        }
    }

    /// Lifecycle notes worth showing beside the title.
    #[must_use]
    pub fn notes(&self) -> String {
        let mut notes = Vec::new();
        if self.completed {
            notes.push("done");
        }
        if self.trashed {
            notes.push("trashed");
        }
        if self.blocked {
            notes.push("blocked");
        }
        if notes.is_empty() {
            String::new()
        } else {
            format!("  ({})", notes.join(", "))
        }
    }
}

/// Half-open civil-date bounds and lifecycle filters for an agenda query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgendaQuery {
    pub start: NaiveDate,
    pub end_exclusive: NaiveDate,
    pub timezone: String,
    pub include_completed: bool,
    pub include_trashed: bool,
    pub include_blocked: bool,
}

impl AgendaQuery {
    #[must_use]
    pub fn new(start: NaiveDate, end_exclusive: NaiveDate) -> Self {
        Self {
            start,
            end_exclusive,
            timezone: "UTC".to_owned(),
            include_completed: false,
            include_trashed: false,
            include_blocked: true,
        }
    }

    pub fn with_timezone(
        mut self,
        timezone: impl Into<String>,
    ) -> Result<Self, crate::domain::todo::TodoError> {
        let timezone = timezone.into();
        timezone
            .parse::<Tz>()
            .map_err(|_| crate::domain::todo::TodoError::InvalidTimezone {
                timezone: timezone.clone(),
            })?;
        self.timezone = timezone;
        Ok(self)
    }

    pub fn try_new(
        start: NaiveDate,
        end_exclusive: NaiveDate,
        timezone: impl Into<String>,
    ) -> Result<Self, crate::domain::todo::TodoError> {
        Self::new(start, end_exclusive).with_timezone(timezone)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgendaOutput {
    pub start: NaiveDate,
    pub end_exclusive: NaiveDate,
    /// The IANA zone the window and every rendered time is expressed in
    pub timezone: String,
    pub todo_projection: Option<TodoProjectionMetadata>,
    pub items: Vec<AgendaItem>,
}

impl AgendaOutput {
    /// # Errors
    /// Returns an invalid query, timezone, day-boundary, or recurrence error.
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_snapshot(
        query: AgendaQuery,
        events: Vec<Event>,
        todos: Vec<Todo>,
    ) -> Result<Self, QueryError<Infallible>> {
        Self::from_sources(&query, events, todos, None)
    }

    /// Build an agenda while retaining the immutable todo projection identity.
    #[allow(clippy::needless_pass_by_value)]
    pub fn from_projection(
        query: AgendaQuery,
        events: Vec<Event>,
        todos: AgendaTodoSnapshot,
    ) -> Result<Self, QueryError<Infallible>> {
        Self::from_sources(&query, events, todos.todos, Some(todos.metadata))
    }

    fn from_sources(
        query: &AgendaQuery,
        events: Vec<Event>,
        todos: Vec<Todo>,
        todo_projection: Option<TodoProjectionMetadata>,
    ) -> Result<Self, QueryError<Infallible>> {
        if query.start >= query.end_exclusive {
            return Err(QueryError::Domain(
                crate::domain::todo::TodoError::InvalidRecurrenceRange,
            ));
        }
        let zone = query
            .timezone
            .parse::<Tz>()
            .map_err(|_| QueryError::InvalidTimezone {
                timezone: query.timezone.clone(),
            })?;
        let window_start = local_day_boundary(zone, query.start, &query.timezone)?;
        let window_end = local_day_boundary(zone, query.end_exclusive, &query.timezone)?;
        let live_ids: std::collections::HashSet<TodoId> = todos
            .iter()
            .filter(|todo| todo.trashed_at.is_none() && todo.completed_at.is_none())
            .map(|todo| todo.id)
            .collect();
        let through = query.end_exclusive.pred_opt().ok_or(QueryError::Domain(
            crate::domain::todo::TodoError::InvalidRecurrenceRange,
        ))?;
        let mut items = Vec::new();
        for event in events {
            if event.deleted_at.is_some() {
                continue;
            }
            // A rule replaces the base time with the occurrences it implies; an
            // event without one keeps the single-instance path it always had.
            let occurrences = match &event.metadata.recurrence_rule {
                None => overlapping_base(&event.time, query, zone, window_start, window_end),
                Some(rule) => rule
                    .expand(&event.time, query.start, through)
                    .map_err(QueryError::EventDomain)?
                    .into_iter()
                    .map(|(index, time)| (Some(index), time))
                    .collect(),
            };
            for (occurrence_index, time) in occurrences {
                items.push(AgendaItem {
                    kind: AgendaKind::Event,
                    id: event.id.to_string(),
                    title: event.title.clone(),
                    due: None,
                    event_time: Some(time),
                    priority: None,
                    occurrence_index,
                    completed: false,
                    trashed: false,
                    blocked: false,
                });
            }
        }
        for todo in todos {
            let completed = todo.completed_at.is_some();
            let trashed = todo.trashed_at.is_some();
            let blocked = todo.dependency_ids.iter().any(|id| live_ids.contains(id));
            if (!query.include_completed && completed)
                || (!query.include_trashed && trashed)
                || (!query.include_blocked && blocked)
            {
                continue;
            }
            for (index, due) in
                agenda_due_instances(&todo, query.start, through, zone, window_start, window_end)
                    .map_err(QueryError::Domain)?
            {
                items.push(AgendaItem {
                    kind: AgendaKind::Todo,
                    id: todo.id.to_string(),
                    title: todo.title.clone(),
                    due: Some(due),
                    event_time: None,
                    priority: Some(todo.priority),
                    occurrence_index: Some(index),
                    completed,
                    trashed,
                    blocked,
                });
            }
        }
        items.sort_by_key(|item| agenda_order(item, zone));
        Ok(Self {
            start: query.start,
            end_exclusive: query.end_exclusive,
            timezone: query.timezone.clone(),
            todo_projection,
            items,
        })
    }
}

fn agenda_due_instances(
    todo: &Todo,
    from: NaiveDate,
    through: NaiveDate,
    query_zone: Tz,
    window_start: chrono::DateTime<Tz>,
    window_end: chrono::DateTime<Tz>,
) -> Result<Vec<(u32, TodoDue)>, crate::domain::todo::TodoError> {
    let Some(due) = &todo.due else {
        return Ok(Vec::new());
    };
    let (expansion_start, expansion_end) = match due {
        TodoDue::Date { .. } => (from, through),
        TodoDue::Timed { timezone, .. } => {
            let stored_zone = timezone.parse::<Tz>().map_err(|_| {
                crate::domain::todo::TodoError::InvalidTimezone {
                    timezone: timezone.clone(),
                }
            })?;
            // Expand a small civil-date cushion in the stored recurrence zone,
            // then apply the actual instant window in the query zone.
            (
                window_start
                    .with_timezone(&stored_zone)
                    .date_naive()
                    .checked_sub_signed(Duration::days(2))
                    .ok_or(crate::domain::todo::TodoError::InvalidRecurrenceRange)?,
                window_end
                    .with_timezone(&stored_zone)
                    .date_naive()
                    .checked_add_signed(Duration::days(2))
                    .ok_or(crate::domain::todo::TodoError::InvalidRecurrenceRange)?,
            )
        }
    };
    let instances = todo.expand_due_instances_indexed(expansion_start, expansion_end)?;
    Ok(instances
        .into_iter()
        .filter(|(_, due)| match due {
            TodoDue::Date { date, .. } => from <= *date && *date <= through,
            TodoDue::Timed { at, .. } => {
                let instant = at.with_timezone(&query_zone);
                instant >= window_start && instant < window_end
            }
        })
        .collect())
}

fn map_agenda_error<E: std::error::Error + 'static>(
    error: QueryError<Infallible>,
) -> QueryError<E> {
    match error {
        QueryError::InvalidTimezone { timezone } => QueryError::InvalidTimezone { timezone },
        QueryError::InvalidDayBoundary { date, timezone } => {
            QueryError::InvalidDayBoundary { date, timezone }
        }
        QueryError::Domain(error) => QueryError::Domain(error),
        QueryError::EventDomain(error) => QueryError::EventDomain(error),
        QueryError::Repository(error) => match error {},
        QueryError::EventNotFound { event_id } => QueryError::EventNotFound { event_id },
        QueryError::TodoNotFound { todo_id } => QueryError::TodoNotFound { todo_id },
        QueryError::ProjectNotFound { project_id } => QueryError::ProjectNotFound { project_id },
    }
}

fn local_day_boundary(
    zone: Tz,
    date: NaiveDate,
    timezone: &str,
) -> Result<chrono::DateTime<Tz>, QueryError<Infallible>> {
    match zone.from_local_datetime(&date.and_time(chrono::NaiveTime::MIN)) {
        LocalResult::Single(value) => Ok(value),
        LocalResult::Ambiguous(_, _) | LocalResult::None => Err(QueryError::InvalidDayBoundary {
            date,
            timezone: timezone.to_owned(),
        }),
    }
}

/// The base time of a non-recurring event, when it overlaps the queried window.
fn overlapping_base(
    time: &EventTime,
    query: &AgendaQuery,
    zone: Tz,
    window_start: DateTime<Tz>,
    window_end: DateTime<Tz>,
) -> Vec<(Option<u32>, EventTime)> {
    let overlaps = match time {
        EventTime::AllDay {
            start,
            end_exclusive,
        } => *start < query.end_exclusive && *end_exclusive > query.start,
        EventTime::Timed { start, end, .. } => {
            start.with_timezone(&zone) < window_end && end.with_timezone(&zone) > window_start
        }
    };
    if overlaps {
        vec![(None, time.clone())]
    } else {
        Vec::new()
    }
}

/// Order a day the way it is lived: all-day first, then by the clock.
///
/// Title is only a tie-break. Sorting a day by title alone put a 23:20 shutdown
/// ahead of an 08:00 wake, which is not a day anyone can read.
fn agenda_order(item: &AgendaItem, zone: Tz) -> (NaiveDate, u8, u32, u8, String, String, u32) {
    let date = item
        .due
        .as_ref()
        .map(|due| todo_due_date(due, zone))
        .or_else(|| {
            item.event_time
                .as_ref()
                .map(|time| event_time_date(time, zone))
        })
        .unwrap_or(NaiveDate::MAX);
    let (timed, minute) = agenda_start_minute(item, zone);
    (
        date,
        timed,
        minute,
        u8::from(item.kind != AgendaKind::Event),
        item.title.to_lowercase(),
        item.id.clone(),
        item.occurrence_index.unwrap_or(0),
    )
}

/// Whether a row is timed, and how far into its local day it starts.
fn agenda_start_minute(item: &AgendaItem, zone: Tz) -> (u8, u32) {
    let local = match (&item.due, &item.event_time) {
        (Some(TodoDue::Timed { at, .. }), _) => Some(at.with_timezone(&zone).time()),
        (None, Some(EventTime::Timed { start, .. })) => Some(start.with_timezone(&zone).time()),
        // An all-day due value, an all-day event, or a row carrying no time at all
        _ => None,
    };
    local.map_or((0, 0), |time| (1, time.hour() * 60 + time.minute()))
}

fn todo_due_date(due: &TodoDue, zone: Tz) -> NaiveDate {
    match due {
        TodoDue::Date { date, .. } => *date,
        TodoDue::Timed { at, .. } => at.with_timezone(&zone).date_naive(),
    }
}

fn event_time_date(time: &EventTime, zone: Tz) -> NaiveDate {
    match time {
        EventTime::AllDay { start, .. } => *start,
        EventTime::Timed { start, .. } => start.with_timezone(&zone).date_naive(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reminder {
    pub todo_id: TodoId,
    pub title: String,
    pub trigger_at: DateTime<chrono::Utc>,
    pub minutes_before: u32,
    pub repeatable: bool,
}

/// One deterministic result from legacy reminder materialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReminderDelivery {
    pub todo_id: TodoId,
    pub title: String,
    pub minutes_before: u32,
    pub repeatable: bool,
    pub scheduled_for: DateTime<chrono::Utc>,
    pub status: String,
    pub channel: &'static str,
}

impl From<Todo> for TodoQueryProjection {
    fn from(todo: Todo) -> Self {
        let Todo {
            id,
            title,
            due,
            recurrence,
            mut reminders,
            priority,
            project_id,
            tag_ids,
            dependency_ids,
            notes,
            parent_id,
            completed_at,
            trashed_at,
            version,
            created_at,
            updated_at,
        } = todo;
        reminders.sort_by_key(|reminder| (reminder.minutes_before, reminder.repeatable));
        Self {
            id,
            title,
            due,
            recurrence,
            reminders,
            priority,
            project_id,
            tag_ids,
            dependency_ids,
            notes,
            parent_id,
            completed_at,
            trashed_at,
            version,
            created_at,
            updated_at,
        }
    }
}

impl fmt::Display for TodoQueryProjection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}\t{}\t{}", self.id, self.title, self.priority)?;
        match &self.due {
            Some(due) => write!(formatter, "\t{due:?}")?,
            None => formatter.write_str("\tno-due")?,
        }
        if let Some(project_id) = self.project_id {
            write!(formatter, "\tproject={project_id}")?;
        }
        if let Some(parent_id) = self.parent_id {
            write!(formatter, "\tparent={parent_id}")?;
        }
        if self.completed_at.is_some() {
            formatter.write_str("\tcompleted")?;
        }
        if self.trashed_at.is_some() {
            formatter.write_str("\ttrashed")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagProjection {
    pub id: TagId,
    pub name: String,
    pub created_at: DateTime<chrono::Utc>,
    pub updated_at: DateTime<chrono::Utc>,
}
impl From<Tag> for TagProjection {
    fn from(tag: Tag) -> Self {
        Self {
            id: tag.id,
            name: tag.name,
            created_at: tag.created_at,
            updated_at: tag.updated_at,
        }
    }
}
impl fmt::Display for TagProjection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\t{}", self.id, self.name)
    }
}
pub struct TagUseCases<R> {
    repository: R,
}
impl<R> TagUseCases<R> {
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}
impl<R> TagUseCases<R>
where
    R: TagRepository,
    R::Error: std::error::Error + 'static,
{
    pub fn create_tag(
        &self,
        name: impl Into<String>,
    ) -> Result<TagProjection, ApplicationError<R::Error>> {
        let tag = Tag::new(name)?;
        self.repository
            .save_tag(&tag)
            .map_err(ApplicationError::Repository)?;
        Ok(TagProjection::from(tag))
    }
    pub fn list_tags(&self) -> Result<Vec<TagProjection>, QueryError<R::Error>> {
        let mut tags = self
            .repository
            .list_tags()
            .map_err(QueryError::Repository)?;
        tags.sort_by_cached_key(|tag| (tag.normalized_name.clone(), tag.id.as_uuid()));
        Ok(tags.into_iter().map(TagProjection::from).collect())
    }
}

/// Application boundary for project creation and listing.
pub struct ProjectUseCases<R> {
    repository: R,
}

impl<R> ProjectUseCases<R> {
    #[must_use]
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ProjectUseCases<R>
where
    R: ProjectRepository,
    R::Error: std::error::Error + 'static,
{
    /// # Errors
    /// Returns domain validation or repository persistence errors.
    pub fn create_project(
        &self,
        name: impl Into<String>,
    ) -> Result<ProjectProjection, ApplicationError<R::Error>> {
        let project = Project::new(name)?;
        self.repository
            .save_project(&project)
            .map_err(ApplicationError::Repository)?;
        Ok(ProjectProjection::from(project))
    }

    /// # Errors
    /// Returns a repository query error.
    pub fn list_projects(&self) -> Result<Vec<ProjectProjection>, QueryError<R::Error>> {
        let mut projects = self
            .repository
            .list_projects()
            .map_err(QueryError::Repository)?;
        projects
            .sort_by_cached_key(|project| (project.normalized_name.clone(), project.id.as_uuid()));
        Ok(projects.into_iter().map(ProjectProjection::from).collect())
    }

    /// # Errors
    /// Returns a repository query error or [`QueryError::ProjectNotFound`].
    pub fn show_project(
        &self,
        project_id: ProjectId,
    ) -> Result<ProjectProjection, QueryError<R::Error>> {
        self.repository
            .find_project(project_id)
            .map_err(QueryError::Repository)?
            .map(ProjectProjection::from)
            .ok_or(QueryError::ProjectNotFound { project_id })
    }
}

/// Application boundary for one combined, read-only agenda query.
pub struct AgendaUseCases<R> {
    repository: R,
}

impl<R> AgendaUseCases<R> {
    #[must_use]
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> AgendaUseCases<R>
where
    R: AgendaRepository,
    R::Error: std::error::Error + 'static,
{
    /// Query events and todo recurrence instances, then apply lifecycle
    /// filters and deterministic ordering to the shared snapshot.
    pub fn query(&self, query: AgendaQuery) -> Result<AgendaOutput, QueryError<R::Error>> {
        let zone = query
            .timezone
            .parse::<Tz>()
            .map_err(|_| QueryError::InvalidTimezone {
                timezone: query.timezone.clone(),
            })?;
        local_day_boundary(zone, query.start, &query.timezone).map_err(map_agenda_error)?;
        local_day_boundary(zone, query.end_exclusive, &query.timezone).map_err(map_agenda_error)?;
        // Projection validity is an authority prerequisite. Do not open or await
        // PostgreSQL when the imported todo snapshot is unusable.
        let todos = self
            .repository
            .agenda_todos(query.include_trashed)
            .map_err(QueryError::Repository)?;
        let events = self
            .repository
            .agenda_events(query.include_trashed)
            .map_err(QueryError::Repository)?;
        AgendaOutput::from_projection(query, events, todos).map_err(map_agenda_error)
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

/// Refuse a rule that cannot produce its own first occurrence, before it is stored.
fn validated_rule(rule: EventRecurrence, time: &EventTime) -> Result<EventRecurrence, DomainError> {
    rule.validate()?;
    let first = match time {
        EventTime::Timed { start, .. } => start.date_naive(),
        EventTime::AllDay { start, .. } => *start,
    };
    rule.expand(time, first, first)?;
    Ok(rule)
}

impl<R> EventUseCases<R>
where
    R: CalendarEventRepository,
    R::Error: std::error::Error + 'static,
{
    /// # Errors
    /// Returns domain validation or asynchronous repository persistence errors.
    pub fn create_calendar(
        &self,
        name: impl Into<String>,
    ) -> Result<Calendar, ApplicationError<R::Error>> {
        let calendar = Calendar::new(name)?;
        self.repository
            .save_calendar(&calendar)
            .map_err(ApplicationError::Repository)?;
        Ok(calendar)
    }

    /// # Errors
    /// Returns domain validation or asynchronous repository persistence errors.
    pub fn create_event(
        &self,
        calendar_id: CalendarId,
        title: impl Into<String>,
        time: EventTime,
    ) -> Result<Event, ApplicationError<R::Error>> {
        self.create_repeating_event(calendar_id, title, time, None)
    }

    /// Create an event that may repeat.
    ///
    /// # Errors
    /// Returns a domain error for an invalid title, time, or repeat rule, and a
    /// typed repository error when the write fails.
    pub fn create_repeating_event(
        &self,
        calendar_id: CalendarId,
        title: impl Into<String>,
        time: EventTime,
        recurrence: Option<EventRecurrence>,
    ) -> Result<Event, ApplicationError<R::Error>> {
        self.create_detailed_event(
            calendar_id,
            title,
            time,
            EventDetails {
                recurrence,
                ..EventDetails::default()
            },
        )
    }

    /// Create an event with any of its optional detail.
    ///
    /// # Errors
    /// Returns a domain error for an invalid title, time, text field, URL, or
    /// repeat rule, and a typed repository error when the write fails.
    pub fn create_detailed_event(
        &self,
        calendar_id: CalendarId,
        title: impl Into<String>,
        time: EventTime,
        details: EventDetails,
    ) -> Result<Event, ApplicationError<R::Error>> {
        let mut event = Event::new(calendar_id, title, time)?;
        details.apply_to(&mut event)?;
        self.repository
            .save_event(&event)
            .map_err(ApplicationError::Repository)?;
        Ok(event)
    }

    /// # Errors
    /// Returns a typed repository query error.
    pub fn list_calendars(&self) -> Result<Vec<CalendarProjection>, QueryError<R::Error>> {
        let mut items = self
            .repository
            .list_calendars()
            .map_err(QueryError::Repository)?;
        items.sort_by_cached_key(|calendar| (calendar.name.to_lowercase(), calendar.id.as_uuid()));
        Ok(items.into_iter().map(CalendarProjection::from).collect())
    }

    /// # Errors
    /// Returns a repository query error or [`QueryError::EventNotFound`].
    pub fn show_event(&self, event_id: EventId) -> Result<EventProjection, QueryError<R::Error>> {
        self.repository
            .find_event(event_id)
            .map_err(QueryError::Repository)?
            .map(EventProjection::from)
            .ok_or(QueryError::EventNotFound { event_id })
    }

    /// # Errors
    /// Returns a typed repository query error.
    pub fn list_events(
        &self,
        calendar_id: Option<CalendarId>,
    ) -> Result<Vec<EventProjection>, QueryError<R::Error>> {
        let mut items = self
            .repository
            .list_events(calendar_id)
            .map_err(QueryError::Repository)?;
        items.sort_by(compare_events);
        Ok(items.into_iter().map(EventProjection::from).collect())
    }

    /// # Errors
    /// Returns a typed optimistic lifecycle error.
    pub fn cancel_event(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<EventProjection, EventLifecycleError<R::Error>>
    where
        R::Error: EventLifecycleErrorMapping,
    {
        self.repository
            .cancel_event(event_id, expected_version)
            .map(EventProjection::from)
            .map_err(|error| error.map_event_lifecycle_error(event_id, expected_version))
    }

    /// # Errors
    /// Returns a typed optimistic lifecycle error.
    pub fn restore_event(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<EventProjection, EventLifecycleError<R::Error>>
    where
        R::Error: EventLifecycleErrorMapping,
    {
        self.repository
            .restore_event(event_id, expected_version)
            .map(EventProjection::from)
            .map_err(|error| error.map_event_lifecycle_error(event_id, expected_version))
    }

    /// Edit the title and/or complete explicit temporal form using the
    /// caller-supplied optimistic version.
    pub fn edit_event(
        &self,
        event_id: EventId,
        expected_version: i64,
        edit: &EventEdit,
    ) -> Result<EventProjection, ApplicationError<R::Error>> {
        if expected_version < 1 {
            return Err(ApplicationError::Domain(DomainError::InvalidEventVersion));
        }
        if edit.is_empty() {
            return Err(ApplicationError::Domain(DomainError::EmptyField {
                field: "editable field",
            }));
        }
        if let Some(title) = &edit.title {
            Event::new(
                CalendarId::new(),
                title.clone(),
                EventTime::all_day(
                    NaiveDate::MIN,
                    NaiveDate::MIN.succ_opt().unwrap_or(NaiveDate::MAX),
                )?,
            )?;
        }
        if let Some(time) = &edit.time {
            match time {
                EventTime::Timed {
                    start,
                    end,
                    timezone,
                } => {
                    EventTime::timed(*start, *end, timezone.clone())?;
                }
                EventTime::AllDay {
                    start,
                    end_exclusive,
                } => {
                    EventTime::all_day(*start, *end_exclusive)?;
                }
            }
        }
        self.repository
            .edit_event(event_id, expected_version, edit)
            .map(EventProjection::from)
            .map_err(ApplicationError::Repository)
    }

    /// # Errors
    /// Returns a timezone/day-boundary validation error or repository query error.
    pub fn day_agenda(
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
