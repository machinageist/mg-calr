#![allow(clippy::missing_errors_doc)]
use std::cmp::Ordering;
use std::convert::Infallible;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Duration, FixedOffset, LocalResult, NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;
use serde::Serialize;
use thiserror::Error;

use crate::domain::{
    Calendar, CalendarId, DomainError, Event, EventId, EventTime,
    todo::{Priority, Project, ProjectId, Tag, TagId, Todo, TodoDue, TodoId, TodoReminder},
};

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
    fn cancel_event(
        &self,
        id: EventId,
        expected_version: i64,
    ) -> RepositoryFuture<'_, Event, Self::Error>;
    /// # Errors
    /// Returns a typed repository lifecycle error.
    fn restore_event(
        &self,
        id: EventId,
        expected_version: i64,
    ) -> RepositoryFuture<'_, Event, Self::Error>;
    /// # Errors
    /// Returns a typed repository lifecycle error.
    fn edit_event<'a>(
        &'a self,
        id: EventId,
        expected_version: i64,
        edit: &'a EventEdit,
    ) -> RepositoryFuture<'a, Event, Self::Error>;
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

/// Explicitly supplied fields for one optimistic event edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEdit {
    pub title: Option<String>,
    pub time: Option<EventTime>,
}

impl EventEdit {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.title.is_none() && self.time.is_none()
    }
}

/// Asynchronous persistence boundary for the local-only todo store.
pub trait AsyncTodoRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_todo<'a>(&'a self, todo: &'a Todo) -> RepositoryFuture<'a, (), Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn find_todo(&self, id: TodoId) -> RepositoryFuture<'_, Option<Todo>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_todos(&self) -> RepositoryFuture<'_, Vec<Todo>, Self::Error>;
    /// # Errors
    /// Returns a typed persistence, not-found, or optimistic-lock error.
    fn complete_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> RepositoryFuture<'_, Todo, Self::Error>;
    /// # Errors
    /// Returns a typed persistence, not-found, or optimistic-lock error.
    fn trash_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> RepositoryFuture<'_, Todo, Self::Error>;
    /// # Errors
    /// Returns a typed persistence, not-found, or optimistic-lock error.
    fn restore_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> RepositoryFuture<'_, Todo, Self::Error>;
    /// # Errors
    /// Returns a typed persistence, not-found, not-trashed, or optimistic-lock error.
    fn purge_todo(
        &self,
        id: TodoId,
        expected_version: i64,
    ) -> RepositoryFuture<'_, TodoId, Self::Error>;
    /// # Errors
    /// Returns a typed persistence, not-found, or optimistic-lock error.
    fn edit_todo(
        &self,
        id: TodoId,
        expected_version: i64,
        edit: TodoEdit,
    ) -> RepositoryFuture<'_, Todo, Self::Error>;
    /// Query reminders whose trigger is at or before the supplied instant.
    fn due_reminders(
        &self,
        at: DateTime<chrono::Utc>,
    ) -> RepositoryFuture<'_, Vec<Reminder>, Self::Error>;
    /// Scan due reminders and idempotently record delivery candidates.
    fn scan_reminders(
        &self,
        at: DateTime<chrono::Utc>,
        dry_run: bool,
    ) -> RepositoryFuture<'_, Vec<ReminderDelivery>, Self::Error>;
}

/// Read-only persistence boundary for one combined agenda snapshot.
pub trait AsyncAgendaRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed query error.
    fn agenda_events(&self, include_trashed: bool)
    -> RepositoryFuture<'_, Vec<Event>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn agenda_todos(
        &self,
        include_trashed: bool,
    ) -> RepositoryFuture<'_, AgendaTodoSnapshot, Self::Error>;
}

/// Asynchronous persistence boundary for project metadata.
pub trait AsyncTagRepository {
    type Error;
    fn save_tag<'a>(&'a self, tag: &'a Tag) -> RepositoryFuture<'a, (), Self::Error>;
    fn list_tags(&self) -> RepositoryFuture<'_, Vec<Tag>, Self::Error>;
}

pub trait AsyncProjectRepository {
    type Error;

    /// # Errors
    /// Returns the repository's typed persistence error.
    fn save_project<'a>(&'a self, project: &'a Project) -> RepositoryFuture<'a, (), Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn find_project(&self, id: ProjectId) -> RepositoryFuture<'_, Option<Project>, Self::Error>;
    /// # Errors
    /// Returns the repository's typed query error.
    fn list_projects(&self) -> RepositoryFuture<'_, Vec<Project>, Self::Error>;
}

/// The explicitly supplied fields for one optimistic todo edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoEdit {
    pub title: Option<String>,
    pub priority: Option<Priority>,
    pub due: Option<TodoDue>,
    pub recurrence: Option<Option<crate::domain::todo::RecurrenceRule>>,
    pub notes: Option<Option<String>>,
    /// `None` preserves the existing project; `Some(None)` clears it.
    pub project_id: Option<Option<ProjectId>>,
    /// `None` preserves the existing parent; `Some(None)` clears it.
    pub parent_id: Option<Option<TodoId>>,
    pub tag_ids: Option<Vec<TagId>>,
    /// `None` preserves dependencies; `Some` atomically replaces them.
    pub dependency_ids: Option<Vec<TodoId>>,
    /// `None` preserves reminders; `Some` atomically replaces them.
    pub reminders: Option<Vec<TodoReminder>>,
}

impl TodoEdit {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.priority.is_none()
            && self.due.is_none()
            && self.recurrence.is_none()
            && self.notes.is_none()
            && self.project_id.is_none()
            && self.parent_id.is_none()
            && self.tag_ids.is_none()
            && self.dependency_ids.is_none()
            && self.reminders.is_none()
    }
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EventProjection {
    pub id: EventId,
    pub calendar_id: CalendarId,
    pub title: String,
    pub time: EventTime,
    pub version: i64,
    pub cancelled: bool,
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
    R: AsyncTagRepository,
    R::Error: std::error::Error + 'static,
{
    pub async fn create_tag_async(
        &self,
        name: impl Into<String>,
    ) -> Result<TagProjection, ApplicationError<R::Error>> {
        let tag = Tag::new(name)?;
        self.repository
            .save_tag(&tag)
            .await
            .map_err(ApplicationError::Repository)?;
        Ok(TagProjection::from(tag))
    }
    pub async fn list_tags_async(&self) -> Result<Vec<TagProjection>, QueryError<R::Error>> {
        let mut tags = self
            .repository
            .list_tags()
            .await
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
    R: AsyncProjectRepository,
    R::Error: std::error::Error + 'static,
{
    /// # Errors
    /// Returns domain validation or repository persistence errors.
    pub async fn create_project_async(
        &self,
        name: impl Into<String>,
    ) -> Result<ProjectProjection, ApplicationError<R::Error>> {
        let project = Project::new(name)?;
        self.repository
            .save_project(&project)
            .await
            .map_err(ApplicationError::Repository)?;
        Ok(ProjectProjection::from(project))
    }

    /// # Errors
    /// Returns a repository query error.
    pub async fn list_projects_async(
        &self,
    ) -> Result<Vec<ProjectProjection>, QueryError<R::Error>> {
        let mut projects = self
            .repository
            .list_projects()
            .await
            .map_err(QueryError::Repository)?;
        projects
            .sort_by_cached_key(|project| (project.normalized_name.clone(), project.id.as_uuid()));
        Ok(projects.into_iter().map(ProjectProjection::from).collect())
    }

    /// # Errors
    /// Returns a repository query error or [`QueryError::ProjectNotFound`].
    pub async fn show_project_async(
        &self,
        project_id: ProjectId,
    ) -> Result<ProjectProjection, QueryError<R::Error>> {
        self.repository
            .find_project(project_id)
            .await
            .map_err(QueryError::Repository)?
            .map(ProjectProjection::from)
            .ok_or(QueryError::ProjectNotFound { project_id })
    }
}

/// Application boundary for todo creation and query projection.
pub struct TodoUseCases<R> {
    repository: R,
}

impl<R> TodoUseCases<R> {
    #[must_use]
    pub const fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> TodoUseCases<R>
where
    R: AsyncTodoRepository,
    R::Error: std::error::Error + 'static,
{
    /// # Errors
    /// Returns domain validation or asynchronous repository persistence errors.
    pub async fn create_todo_async(
        &self,
        title: impl Into<String>,
        priority: Priority,
        due: Option<TodoDue>,
    ) -> Result<TodoQueryProjection, ApplicationError<R::Error>> {
        let mut todo = Todo::new(title)?;
        todo.priority = priority;
        todo.due = due;
        self.repository
            .save_todo(&todo)
            .await
            .map_err(ApplicationError::Repository)?;
        Ok(TodoQueryProjection::from(todo))
    }

    /// # Errors
    /// Returns a typed repository query error.
    pub async fn list_todos_async(&self) -> Result<Vec<TodoQueryProjection>, QueryError<R::Error>> {
        let todos = self
            .repository
            .list_todos()
            .await
            .map_err(QueryError::Repository)?;
        Ok(todos.into_iter().map(TodoQueryProjection::from).collect())
    }

    /// # Errors
    /// Returns a typed repository query error or [`QueryError::TodoNotFound`].
    pub async fn show_todo_async(
        &self,
        todo_id: TodoId,
    ) -> Result<TodoQueryProjection, QueryError<R::Error>> {
        self.repository
            .find_todo(todo_id)
            .await
            .map_err(QueryError::Repository)?
            .map(TodoQueryProjection::from)
            .ok_or(QueryError::TodoNotFound { todo_id })
    }

    /// # Errors
    /// Returns domain or repository persistence/conflict errors.
    pub async fn complete_todo_async(
        &self,
        todo_id: TodoId,
        expected_version: i64,
    ) -> Result<TodoQueryProjection, ApplicationError<R::Error>> {
        self.repository
            .complete_todo(todo_id, expected_version)
            .await
            .map(TodoQueryProjection::from)
            .map_err(ApplicationError::Repository)
    }

    /// Trash a live todo using its current optimistic-lock version. Completed
    /// todos may be trashed; the legacy `deleted_at` column is untouched.
    ///
    /// # Errors
    /// Returns a typed repository persistence, not-found, or version-conflict error.
    pub async fn trash_todo_async(
        &self,
        todo_id: TodoId,
        expected_version: i64,
    ) -> Result<TodoQueryProjection, ApplicationError<R::Error>> {
        self.repository
            .trash_todo(todo_id, expected_version)
            .await
            .map(TodoQueryProjection::from)
            .map_err(ApplicationError::Repository)
    }

    /// Restore a trashed todo using its current optimistic-lock version. Legacy
    /// `deleted_at` tombstones are cleared during restoration.
    ///
    /// # Errors
    /// Returns a typed repository persistence, not-found, or version-conflict error.
    pub async fn restore_todo_async(
        &self,
        todo_id: TodoId,
        expected_version: i64,
    ) -> Result<TodoQueryProjection, ApplicationError<R::Error>> {
        self.repository
            .restore_todo(todo_id, expected_version)
            .await
            .map(TodoQueryProjection::from)
            .map_err(ApplicationError::Repository)
    }

    /// Permanently remove a trashed todo using its current optimistic-lock version.
    ///
    /// # Errors
    /// Returns a typed repository persistence, not-found, not-trashed, or conflict error.
    pub async fn purge_todo_async(
        &self,
        todo_id: TodoId,
        expected_version: i64,
    ) -> Result<TodoId, ApplicationError<R::Error>> {
        self.repository
            .purge_todo(todo_id, expected_version)
            .await
            .map_err(ApplicationError::Repository)
    }

    /// Edit explicitly supplied core fields using the caller's optimistic version.
    ///
    /// # Errors
    /// Returns validation or repository persistence, not-found, or conflict errors.
    pub async fn edit_todo_async(
        &self,
        todo_id: TodoId,
        expected_version: i64,
        edit: TodoEdit,
    ) -> Result<TodoQueryProjection, ApplicationError<R::Error>> {
        if edit.is_empty() {
            return Err(ApplicationError::Todo(
                crate::domain::todo::TodoError::EmptyField {
                    field: "editable field",
                },
            ));
        }
        if let Some(title) = &edit.title {
            // Reuse the domain's canonical title validation without duplicating it here.
            Todo::new(title.clone())?;
        }
        self.repository
            .edit_todo(todo_id, expected_version, edit)
            .await
            .map(TodoQueryProjection::from)
            .map_err(ApplicationError::Repository)
    }
    pub async fn due_reminders_async(
        &self,
        at: DateTime<chrono::Utc>,
    ) -> Result<Vec<Reminder>, QueryError<R::Error>> {
        self.repository
            .due_reminders(at)
            .await
            .map_err(QueryError::Repository)
    }

    pub async fn scan_reminders_async(
        &self,
        at: DateTime<chrono::Utc>,
        dry_run: bool,
    ) -> Result<Vec<ReminderDelivery>, QueryError<R::Error>> {
        self.repository
            .scan_reminders(at, dry_run)
            .await
            .map_err(QueryError::Repository)
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
    R: AsyncAgendaRepository,
    R::Error: std::error::Error + 'static,
{
    /// Query events and todo recurrence instances, then apply lifecycle
    /// filters and deterministic ordering to the shared snapshot.
    pub async fn query_async(
        &self,
        query: AgendaQuery,
    ) -> Result<AgendaOutput, QueryError<R::Error>> {
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
            .await
            .map_err(QueryError::Repository)?;
        let events = self
            .repository
            .agenda_events(query.include_trashed)
            .await
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
    /// Returns a typed optimistic lifecycle error.
    pub async fn cancel_event_async(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<EventProjection, EventLifecycleError<R::Error>>
    where
        R::Error: EventLifecycleErrorMapping,
    {
        self.repository
            .cancel_event(event_id, expected_version)
            .await
            .map(EventProjection::from)
            .map_err(|error| error.map_event_lifecycle_error(event_id, expected_version))
    }

    /// # Errors
    /// Returns a typed optimistic lifecycle error.
    pub async fn restore_event_async(
        &self,
        event_id: EventId,
        expected_version: i64,
    ) -> Result<EventProjection, EventLifecycleError<R::Error>>
    where
        R::Error: EventLifecycleErrorMapping,
    {
        self.repository
            .restore_event(event_id, expected_version)
            .await
            .map(EventProjection::from)
            .map_err(|error| error.map_event_lifecycle_error(event_id, expected_version))
    }

    /// Edit the title and/or complete explicit temporal form using the
    /// caller-supplied optimistic version.
    pub async fn edit_event_async(
        &self,
        event_id: EventId,
        expected_version: i64,
        edit: EventEdit,
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
            .edit_event(event_id, expected_version, &edit)
            .await
            .map(EventProjection::from)
            .map_err(ApplicationError::Repository)
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
