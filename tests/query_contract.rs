use std::convert::Infallible;

use chrono::{DateTime, FixedOffset, NaiveDate};
use mg_calr::application::{
    AsyncCalendarEventRepository, EventEdit, EventUseCases, QueryError, RepositoryFuture,
};
use mg_calr::domain::{Calendar, CalendarId, Event, EventId, EventTime};

fn instant(value: &str) -> DateTime<FixedOffset> {
    value.parse().expect("valid RFC3339 fixture")
}

#[derive(Default)]
struct QueryRepository {
    calendars: Vec<Calendar>,
    events: Vec<Event>,
}

impl AsyncCalendarEventRepository for QueryRepository {
    type Error = Infallible;

    fn save_calendar<'a>(
        &'a self,
        _calendar: &'a Calendar,
    ) -> RepositoryFuture<'a, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn save_event<'a>(&'a self, _event: &'a Event) -> RepositoryFuture<'a, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn list_calendars(&self) -> RepositoryFuture<'_, Vec<Calendar>, Self::Error> {
        Box::pin(async { Ok(self.calendars.clone()) })
    }

    fn find_event(&self, id: EventId) -> RepositoryFuture<'_, Option<Event>, Self::Error> {
        Box::pin(async move { Ok(self.events.iter().find(|event| event.id == id).cloned()) })
    }

    fn list_events(
        &self,
        calendar_id: Option<CalendarId>,
    ) -> RepositoryFuture<'_, Vec<Event>, Self::Error> {
        Box::pin(async move {
            let mut events = self.events.clone();
            events.retain(|event| calendar_id.is_none_or(|id| event.calendar_id == id));
            Ok(events)
        })
    }

    fn cancel_event(
        &self,
        id: EventId,
        _expected_version: i64,
    ) -> RepositoryFuture<'_, Event, Self::Error> {
        Box::pin(async move {
            Ok(self
                .events
                .iter()
                .find(|event| event.id == id)
                .cloned()
                .unwrap())
        })
    }

    fn restore_event(
        &self,
        id: EventId,
        _expected_version: i64,
    ) -> RepositoryFuture<'_, Event, Self::Error> {
        Box::pin(async move {
            Ok(self
                .events
                .iter()
                .find(|event| event.id == id)
                .cloned()
                .unwrap())
        })
    }

    fn edit_event<'a>(
        &'a self,
        id: EventId,
        _expected_version: i64,
        _edit: &'a EventEdit,
    ) -> RepositoryFuture<'a, Event, Self::Error> {
        Box::pin(async move {
            Ok(self
                .events
                .iter()
                .find(|event| event.id == id)
                .cloned()
                .unwrap())
        })
    }

    fn day_agenda(
        &self,
        date: NaiveDate,
        _timezone: &str,
        _starts_at: DateTime<FixedOffset>,
        _ends_at: DateTime<FixedOffset>,
    ) -> RepositoryFuture<'_, Vec<Event>, Self::Error> {
        Box::pin(async move {
            Ok(self
                .events
                .iter()
                .filter(|event| match event.time {
                    EventTime::AllDay {
                        start,
                        end_exclusive,
                    } => start <= date && end_exclusive > date,
                    EventTime::Timed { .. } => true,
                })
                .cloned()
                .collect())
        })
    }
}

#[tokio::test]
async fn application_queries_return_shared_serializable_projections() {
    let calendar = Calendar::new("Work").unwrap();
    let event = Event::new(
        calendar.id,
        "Standup",
        EventTime::timed(
            instant("2026-08-24T09:00:00-07:00"),
            instant("2026-08-24T09:15:00-07:00"),
            "America/Los_Angeles",
        )
        .unwrap(),
    )
    .unwrap();
    let event_id = event.id;
    let repository = QueryRepository {
        calendars: vec![calendar],
        events: vec![event],
    };
    let app = EventUseCases::new(repository);

    let listed = app.list_events_async(None).await.unwrap();
    let shown = app.show_event_async(event_id).await.unwrap();
    let agenda = app
        .day_agenda_async(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            "America/Los_Angeles",
        )
        .await
        .unwrap();

    assert_eq!(listed, vec![shown.clone()]);
    assert_eq!(agenda, vec![shown.clone()]);
    assert_eq!(serde_json::to_value(&shown).unwrap()["title"], "Standup");
    assert!(shown.to_string().contains("Standup"));
}

#[tokio::test]
async fn missing_event_is_a_typed_query_error() {
    let app = EventUseCases::new(QueryRepository::default());
    let id = EventId::new();

    let error = app.show_event_async(id).await.unwrap_err();

    assert!(matches!(error, QueryError::EventNotFound { event_id } if event_id == id));
}

#[tokio::test]
async fn day_agenda_rejects_unknown_iana_timezone_before_repository_access() {
    let app = EventUseCases::new(QueryRepository::default());

    let error = app
        .day_agenda_async(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            "Mars/Olympus",
        )
        .await
        .unwrap_err();

    assert!(error.to_string().contains("valid IANA timezone"));
}

#[tokio::test]
async fn application_enforces_total_order_on_unordered_repository_results() {
    let zeta = Calendar::new("zeta").unwrap();
    let alpha = Calendar::new("Alpha").unwrap();
    let timed_late = Event::new(
        alpha.id,
        "Late",
        EventTime::timed(
            instant("2026-08-24T10:00:00-07:00"),
            instant("2026-08-24T11:00:00-07:00"),
            "US/Pacific",
        )
        .unwrap(),
    )
    .unwrap();
    let timed_early = Event::new(
        alpha.id,
        "Early",
        EventTime::timed(
            instant("2026-08-24T08:00:00-07:00"),
            instant("2026-08-24T09:00:00-07:00"),
            "US/Pacific",
        )
        .unwrap(),
    )
    .unwrap();
    let all_day = Event::new(
        alpha.id,
        "All day",
        EventTime::all_day(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let expected_event_ids = vec![all_day.id, timed_early.id, timed_late.id];
    let app = EventUseCases::new(QueryRepository {
        calendars: vec![zeta, alpha],
        events: vec![timed_late, timed_early, all_day],
    });

    let calendars = app.list_calendars_async().await.unwrap();
    assert_eq!(
        calendars
            .iter()
            .map(|calendar| calendar.name.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "zeta"]
    );
    let events = app.list_events_async(None).await.unwrap();
    assert_eq!(
        events.iter().map(|event| event.id).collect::<Vec<_>>(),
        expected_event_ids
    );
    let agenda = app
        .day_agenda_async(NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(), "US/Pacific")
        .await
        .unwrap();
    assert_eq!(
        agenda.iter().map(|event| event.id).collect::<Vec<_>>(),
        expected_event_ids
    );
}
