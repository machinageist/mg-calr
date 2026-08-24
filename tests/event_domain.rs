use chrono::{DateTime, FixedOffset, NaiveDate};
use mg_calr::application::{CalendarEventRepository, EventUseCases};
use mg_calr::domain::{Calendar, CalendarId, DomainError, Event, EventTime};
use serde_json::Value;

fn instant(value: &str) -> DateTime<FixedOffset> {
    value.parse().expect("valid RFC3339 fixture")
}

#[test]
fn timed_events_require_iana_timezone_and_end_after_start() {
    let start = instant("2026-08-23T09:00:00-07:00");
    let end = instant("2026-08-23T10:00:00-07:00");

    assert!(matches!(
        EventTime::timed(start, end, ""),
        Err(DomainError::MissingTimezone)
    ));
    assert!(matches!(
        EventTime::timed(start, end, "Mars/Olympus"),
        Err(DomainError::InvalidTimezone { .. })
    ));
    assert_eq!(
        EventTime::timed(start, start, "America/Los_Angeles"),
        Err(DomainError::EndNotAfterStart)
    );
    assert_eq!(
        EventTime::timed(end, start, "America/Los_Angeles"),
        Err(DomainError::EndNotAfterStart)
    );
    assert!(EventTime::timed(start, end, "America/Los_Angeles").is_ok());
}

#[test]
fn timed_event_offsets_must_match_the_iana_zone_at_each_instant() {
    let error = EventTime::timed(
        instant("2026-08-24T09:00:00+00:00"),
        instant("2026-08-24T10:00:00+00:00"),
        "America/Los_Angeles",
    )
    .unwrap_err();
    assert!(matches!(
        error,
        DomainError::OffsetTimezoneMismatch {
            boundary: "start",
            ..
        }
    ));

    let crossing = EventTime::timed(
        instant("2026-11-01T01:30:00-07:00"),
        instant("2026-11-01T01:30:00-08:00"),
        "US/Pacific",
    )
    .expect("each side of a DST transition may have its own valid offset");
    assert!(matches!(crossing, EventTime::Timed { .. }));
}

#[test]
fn all_day_events_use_an_exclusive_end_date() {
    let start = NaiveDate::from_ymd_opt(2026, 8, 23).unwrap();
    let next_day = NaiveDate::from_ymd_opt(2026, 8, 24).unwrap();

    assert!(EventTime::all_day(start, next_day).is_ok());
    assert_eq!(
        EventTime::all_day(start, start),
        Err(DomainError::InvalidAllDayRange)
    );
    assert_eq!(
        EventTime::all_day(next_day, start),
        Err(DomainError::InvalidAllDayRange)
    );
}

#[test]
fn constructors_supply_standard_metadata_and_stable_rfc_uid() {
    let calendar = Calendar::new("Personal").unwrap();
    let event = Event::new(
        calendar.id,
        "Dentist",
        EventTime::timed(
            instant("2026-08-23T09:00:00-07:00"),
            instant("2026-08-23T10:00:00-07:00"),
            "America/Los_Angeles",
        )
        .unwrap(),
    )
    .unwrap();

    assert!(event.rfc_uid.as_str().ends_with("@mg-calr.local"));
    assert_eq!(
        event.rfc_uid.as_str(),
        format!("{}@mg-calr.local", event.id)
    );
    assert!(event.metadata.busy);
    assert!(event.metadata.alarms.is_empty());
    assert!(event.metadata.categories.is_empty());
    assert!(event.created_at <= event.updated_at);
}

#[test]
fn serialized_rfc_uid_cannot_bypass_validation() {
    let error = serde_json::from_str::<mg_calr::domain::RfcUid>("\"bad uid\"")
        .expect_err("whitespace in an RFC UID must be rejected on decode");
    assert!(error.to_string().contains("RFC UID"));
}

#[test]
fn domain_models_serialize_temporal_forms_and_metadata() {
    let calendar = Calendar::new("Personal").unwrap();
    let event = Event::new(
        calendar.id,
        "Conference",
        EventTime::all_day(
            NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let encoded = serde_json::to_value(&event).unwrap();
    assert_eq!(
        encoded["rfc_uid"],
        Value::String(event.rfc_uid.as_str().to_owned())
    );
    assert_eq!(encoded["time"]["AllDay"]["end_exclusive"], "2026-08-25");
    assert_eq!(encoded["busy"], true);

    let decoded: Event = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded, event);
}

#[derive(Default)]
struct MemoryRepository {
    calendars: Vec<Calendar>,
    events: Vec<Event>,
}

impl CalendarEventRepository for MemoryRepository {
    type Error = std::convert::Infallible;

    fn save_calendar(&mut self, calendar: Calendar) -> Result<(), Self::Error> {
        self.calendars.push(calendar);
        Ok(())
    }

    fn save_event(&mut self, event: Event) -> Result<(), Self::Error> {
        self.events.push(event);
        Ok(())
    }
}

#[test]
fn application_boundary_constructs_then_persists_without_transport() {
    let mut app = EventUseCases::new(MemoryRepository::default());
    let calendar = app.create_calendar("Work").unwrap();
    let event = app
        .create_event(
            calendar.id,
            "Standup",
            EventTime::timed(
                instant("2026-08-23T09:00:00-07:00"),
                instant("2026-08-23T09:15:00-07:00"),
                "America/Los_Angeles",
            )
            .unwrap(),
        )
        .unwrap();

    let repository = app.into_repository();
    assert_eq!(repository.calendars, vec![calendar]);
    assert_eq!(repository.events, vec![event]);
}

#[test]
fn invalid_event_title_is_rejected_before_repository_write() {
    let mut app = EventUseCases::new(MemoryRepository::default());
    let calendar = app.create_calendar("Work").unwrap();
    let result = app.create_event(
        CalendarId::new(),
        "bad\n",
        EventTime::all_day(
            NaiveDate::from_ymd_opt(2026, 8, 23).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
        )
        .unwrap(),
    );
    assert!(matches!(
        result,
        Err(mg_calr::application::ApplicationError::Domain(
            DomainError::ControlCharacter {
                field: "event title"
            }
        ))
    ));
    let repository = app.into_repository();
    assert_eq!(repository.calendars, vec![calendar]);
    assert!(repository.events.is_empty());
}
