use std::convert::Infallible;

use chrono::{DateTime, FixedOffset, NaiveDate};
use mg_calr::application::{CalendarEventRepository, EventEdit, EventUseCases, QueryError};
use mg_calr::domain::{Calendar, CalendarId, Event, EventId, EventTime};

fn instant(value: &str) -> DateTime<FixedOffset> {
    value.parse().expect("valid RFC3339 fixture")
}

#[derive(Default)]
struct QueryRepository {
    calendars: Vec<Calendar>,
    events: Vec<Event>,
}

impl CalendarEventRepository for QueryRepository {
    type Error = Infallible;

    fn save_calendar(&self, _calendar: &Calendar) -> Result<(), Self::Error> {
        Ok(())
    }

    fn save_event(&self, _event: &Event) -> Result<(), Self::Error> {
        Ok(())
    }

    fn list_calendars(&self) -> Result<Vec<Calendar>, Self::Error> {
        Ok(self.calendars.clone())
    }

    fn find_event(&self, id: EventId) -> Result<Option<Event>, Self::Error> {
        Ok(self.events.iter().find(|event| event.id == id).cloned())
    }

    fn list_events(&self, calendar_id: Option<CalendarId>) -> Result<Vec<Event>, Self::Error> {
        let mut events = self.events.clone();
        events.retain(|event| calendar_id.is_none_or(|id| event.calendar_id == id));
        Ok(events)
    }

    fn cancel_event(&self, id: EventId, _expected_version: i64) -> Result<Event, Self::Error> {
        Ok(self
            .events
            .iter()
            .find(|event| event.id == id)
            .cloned()
            .unwrap())
    }

    fn restore_event(&self, id: EventId, _expected_version: i64) -> Result<Event, Self::Error> {
        Ok(self
            .events
            .iter()
            .find(|event| event.id == id)
            .cloned()
            .unwrap())
    }

    fn edit_event(
        &self,
        id: EventId,
        _expected_version: i64,
        _edit: &EventEdit,
    ) -> Result<Event, Self::Error> {
        Ok(self
            .events
            .iter()
            .find(|event| event.id == id)
            .cloned()
            .unwrap())
    }

    fn day_agenda(
        &self,
        date: NaiveDate,
        _timezone: &str,
        _starts_at: DateTime<FixedOffset>,
        _ends_at: DateTime<FixedOffset>,
    ) -> Result<Vec<Event>, Self::Error> {
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
    }
}

#[test]
fn application_queries_return_shared_serializable_projections() {
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

    let listed = app.list_events(None).unwrap();
    let shown = app.show_event(event_id).unwrap();
    let agenda = app
        .day_agenda(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            "America/Los_Angeles",
        )
        .unwrap();

    assert_eq!(listed, vec![shown.clone()]);
    assert_eq!(agenda, vec![shown.clone()]);
    assert_eq!(serde_json::to_value(&shown).unwrap()["title"], "Standup");
    assert!(shown.to_string().contains("Standup"));
}

#[test]
fn missing_event_is_a_typed_query_error() {
    let app = EventUseCases::new(QueryRepository::default());
    let id = EventId::new();

    let error = app.show_event(id).unwrap_err();

    assert!(matches!(error, QueryError::EventNotFound { event_id } if event_id == id));
}

#[test]
fn day_agenda_rejects_unknown_iana_timezone_before_repository_access() {
    let app = EventUseCases::new(QueryRepository::default());

    let error = app
        .day_agenda(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            "Mars/Olympus",
        )
        .unwrap_err();

    assert!(error.to_string().contains("valid IANA timezone"));
}

#[test]
fn application_enforces_total_order_on_unordered_repository_results() {
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

    let calendars = app.list_calendars().unwrap();
    assert_eq!(
        calendars
            .iter()
            .map(|calendar| calendar.name.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "zeta"]
    );
    let events = app.list_events(None).unwrap();
    assert_eq!(
        events.iter().map(|event| event.id).collect::<Vec<_>>(),
        expected_event_ids
    );
    let agenda = app
        .day_agenda(NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(), "US/Pacific")
        .unwrap();
    assert_eq!(
        agenda.iter().map(|event| event.id).collect::<Vec<_>>(),
        expected_event_ids
    );
}
