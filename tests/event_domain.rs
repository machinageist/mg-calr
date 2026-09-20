use chrono::{DateTime, FixedOffset, NaiveDate};
use mg_calr::application::EventUseCases;
use mg_calr::domain::{Calendar, CalendarId, DomainError, Event, EventTime};
use mg_calr::storage::Store;
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

// The store is a file this test owns, so the boundary can be exercised against the
// real thing rather than a stand-in
fn scratch() -> (tempfile::TempDir, Store) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = Store::open(directory.path().join("calr.sqlite")).expect("store opens");
    store.migrate().expect("store migrates");
    (directory, store)
}

#[test]
fn application_boundary_constructs_then_persists() {
    let (_directory, store) = scratch();
    let app = EventUseCases::new(store.clone());
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

    // the store keeps instants to the microsecond, so compare what it can hold
    let calendars = store.list_calendars().unwrap();
    assert_eq!(calendars.len(), 1);
    assert_eq!(
        (calendars[0].id, &calendars[0].name),
        (calendar.id, &calendar.name)
    );
    let events = store.list_events(None).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].id, &events[0].title), (event.id, &event.title));
}

#[test]
fn invalid_event_title_is_rejected_before_repository_write() {
    let (_directory, store) = scratch();
    let app = EventUseCases::new(store.clone());
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
    assert_eq!(
        store.list_calendars().unwrap().len(),
        1,
        "the calendar is there"
    );
    assert_eq!(store.list_calendars().unwrap()[0].id, calendar.id);
    assert!(store.list_events(None).unwrap().is_empty());
}

// Recurrence

use chrono::{Datelike, Weekday};
use mg_calr::domain::{EventFrequency, EventRecurrence};

fn on(value: &str) -> chrono::NaiveDate {
    value.parse().expect("valid date")
}

fn weekly(
    count: Option<u32>,
    until: Option<chrono::NaiveDate>,
    days: Vec<Weekday>,
) -> EventRecurrence {
    EventRecurrence::new(EventFrequency::Weekly, 1, count, until, days).expect("valid rule")
}

fn morning() -> EventTime {
    EventTime::timed(
        "2026-09-07T08:00:00-07:00".parse().unwrap(),
        "2026-09-07T08:15:00-07:00".parse().unwrap(),
        "America/Los_Angeles",
    )
    .unwrap()
}

#[test]
fn a_weekday_set_lands_on_every_named_day_and_stops_at_its_count() {
    let rule = weekly(
        Some(78),
        None,
        vec![
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
        ],
    );
    let all = rule
        .expand(&morning(), on("2026-09-07"), on("2027-12-31"))
        .unwrap();
    // Six days a week for thirteen weeks, and Sundays are never among them
    assert_eq!(all.len(), 78);
    assert_eq!(all.first().unwrap().0, 0);
    assert_eq!(all.last().unwrap().0, 77);
    for (_, time) in &all {
        let EventTime::Timed { start, .. } = time else {
            panic!("expected timed occurrences");
        };
        assert_ne!(start.weekday(), Weekday::Sun);
    }
    let last = match &all.last().unwrap().1 {
        EventTime::Timed { start, .. } => start.date_naive(),
        EventTime::AllDay { start, .. } => *start,
    };
    assert_eq!(last, on("2026-12-05"));
}

#[test]
fn expansion_is_clipped_to_the_window_without_renumbering_occurrences() {
    let rule = weekly(Some(13), None, vec![Weekday::Mon]);
    let week = rule
        .expand(&morning(), on("2026-09-21"), on("2026-09-27"))
        .unwrap();
    assert_eq!(week.len(), 1);
    // The index is the occurrence's place in the series, not in the window
    assert_eq!(week[0].0, 2);
}

#[test]
fn an_until_bound_stops_the_series_and_may_not_precede_the_start() {
    let rule = weekly(None, Some(on("2026-09-28")), vec![Weekday::Mon]);
    let found = rule
        .expand(&morning(), on("2026-09-01"), on("2027-01-01"))
        .unwrap();
    assert_eq!(found.len(), 4);

    let backwards = weekly(None, Some(on("2026-09-01")), vec![Weekday::Mon]);
    assert_eq!(
        backwards.expand(&morning(), on("2026-09-01"), on("2027-01-01")),
        Err(DomainError::RecurrenceUntilNotAfterStart)
    );
}

#[test]
fn an_occurrence_keeps_its_wall_time_across_a_daylight_saving_transition() {
    let rule = weekly(Some(20), None, vec![Weekday::Mon]);
    let found = rule
        .expand(&morning(), on("2026-09-07"), on("2027-02-01"))
        .unwrap();
    let stamps: Vec<String> = found
        .iter()
        .map(|(_, time)| match time {
            EventTime::Timed { start, .. } => start.to_rfc3339(),
            EventTime::AllDay { start, .. } => start.to_string(),
        })
        .collect();
    // Pacific leaves daylight time on 1 November 2026; 08:00 stays 08:00
    assert!(stamps.contains(&"2026-10-26T08:00:00-07:00".to_owned()));
    assert!(stamps.contains(&"2026-11-02T08:00:00-08:00".to_owned()));
}

#[test]
fn every_occurrence_keeps_the_base_duration() {
    let rule = weekly(Some(5), None, vec![Weekday::Mon, Weekday::Thu]);
    for (_, time) in rule
        .expand(&morning(), on("2026-09-07"), on("2026-12-31"))
        .unwrap()
    {
        let EventTime::Timed { start, end, .. } = time else {
            panic!("expected timed occurrences");
        };
        assert_eq!((end - start).num_minutes(), 15);
    }
}

#[test]
fn all_day_occurrences_keep_their_span() {
    let base = EventTime::all_day(on("2026-09-07"), on("2026-09-09")).unwrap();
    let rule = weekly(Some(3), None, vec![Weekday::Mon]);
    let found = rule
        .expand(&base, on("2026-09-07"), on("2026-12-31"))
        .unwrap();
    assert_eq!(found.len(), 3);
    for (_, time) in found {
        let EventTime::AllDay {
            start,
            end_exclusive,
        } = time
        else {
            panic!("expected all-day occurrences");
        };
        assert_eq!((end_exclusive - start).num_days(), 2);
    }
}

#[test]
fn daily_and_monthly_rules_step_by_their_interval() {
    let daily = EventRecurrence::new(EventFrequency::Daily, 3, Some(4), None, Vec::new()).unwrap();
    let dates: Vec<chrono::NaiveDate> = daily
        .expand(&morning(), on("2026-09-01"), on("2026-12-31"))
        .unwrap()
        .iter()
        .map(|(_, time)| match time {
            EventTime::Timed { start, .. } => start.date_naive(),
            EventTime::AllDay { start, .. } => *start,
        })
        .collect();
    assert_eq!(
        dates,
        vec![
            on("2026-09-07"),
            on("2026-09-10"),
            on("2026-09-13"),
            on("2026-09-16")
        ]
    );

    let monthly =
        EventRecurrence::new(EventFrequency::Monthly, 1, Some(3), None, Vec::new()).unwrap();
    let months: Vec<chrono::NaiveDate> = monthly
        .expand(&morning(), on("2026-09-01"), on("2027-12-31"))
        .unwrap()
        .iter()
        .map(|(_, time)| match time {
            EventTime::Timed { start, .. } => start.date_naive(),
            EventTime::AllDay { start, .. } => *start,
        })
        .collect();
    assert_eq!(
        months,
        vec![on("2026-09-07"), on("2026-10-07"), on("2026-11-07")]
    );
}

#[test]
fn unrepresentable_rules_are_refused_on_construction() {
    let cases = [
        (
            EventRecurrence::new(EventFrequency::Weekly, 0, Some(5), None, Vec::new()),
            DomainError::InvalidRecurrenceInterval,
        ),
        (
            EventRecurrence::new(EventFrequency::Weekly, 1, Some(0), None, Vec::new()),
            DomainError::InvalidRecurrenceCount { max: 1000 },
        ),
        (
            EventRecurrence::new(EventFrequency::Weekly, 1, None, None, Vec::new()),
            DomainError::UnboundedRecurrence,
        ),
        (
            EventRecurrence::new(EventFrequency::Daily, 1, Some(5), None, vec![Weekday::Mon]),
            DomainError::WeekdaySetWithoutWeekly,
        ),
        (
            EventRecurrence::new(
                EventFrequency::Weekly,
                1,
                Some(5),
                None,
                vec![Weekday::Mon, Weekday::Mon],
            ),
            DomainError::InvalidWeekdaySet,
        ),
    ];
    for (result, expected) in cases {
        assert_eq!(result.unwrap_err(), expected);
    }
}

#[test]
fn a_backwards_window_is_refused() {
    assert_eq!(
        weekly(Some(3), None, Vec::new()).expand(&morning(), on("2026-09-30"), on("2026-09-01")),
        Err(DomainError::InvalidRecurrenceRange)
    );
}

#[test]
fn a_repeating_event_is_created_with_its_rule_and_an_unusable_rule_is_refused() {
    let (_directory, store) = scratch();
    let app = EventUseCases::new(store);
    let calendar = app.create_calendar("Study").unwrap();

    let time = EventTime::timed(
        instant("2026-09-07T07:00:00-07:00"),
        instant("2026-09-07T07:30:00-07:00"),
        "America/Los_Angeles",
    )
    .unwrap();
    let rule = EventRecurrence::new(
        EventFrequency::Weekly,
        1,
        Some(6),
        None,
        vec![Weekday::Mon, Weekday::Wed, Weekday::Fri],
    )
    .unwrap();

    let event = app
        .create_repeating_event(calendar.id, "Wake", time.clone(), Some(rule.clone()))
        .unwrap();
    assert_eq!(event.metadata.recurrence_rule, Some(rule));

    // An event created without a rule keeps carrying none
    let plain = app
        .create_repeating_event(calendar.id, "One off", time.clone(), None)
        .unwrap();
    assert_eq!(plain.metadata.recurrence_rule, None);
}

#[test]
fn event_text_fields_are_bounded_and_refuse_stray_control_characters() {
    use mg_calr::domain::{
        MAX_DESCRIPTION_CHARS, MAX_LOCATION_CHARS, MAX_URL_CHARS, validate_description,
        validate_location, validate_url,
    };

    // a description is multi-line, so newlines and tabs are the only controls it keeps
    assert_eq!(
        validate_description("Two\nlines\tapart".to_owned()).unwrap(),
        "Two\nlines\tapart"
    );
    assert!(matches!(
        validate_description("bell\u{7}".to_owned()),
        Err(DomainError::ControlCharacter { .. })
    ));
    assert!(matches!(
        validate_description("   ".to_owned()),
        Err(DomainError::EmptyField { .. })
    ));
    assert!(matches!(
        validate_description("x".repeat(MAX_DESCRIPTION_CHARS + 1)),
        Err(DomainError::TooLong {
            max: MAX_DESCRIPTION_CHARS,
            ..
        })
    ));

    // a location is one line
    assert_eq!(validate_location("Room 4".to_owned()).unwrap(), "Room 4");
    assert!(matches!(
        validate_location("Room\n4".to_owned()),
        Err(DomainError::ControlCharacter { .. })
    ));
    assert!(matches!(
        validate_location("x".repeat(MAX_LOCATION_CHARS + 1)),
        Err(DomainError::TooLong {
            max: MAX_LOCATION_CHARS,
            ..
        })
    ));

    // a link is one a desktop can open, and nothing else
    for url in [
        "https://example.test/a",
        "http://example.test",
        "mailto:someone@example.test",
        "HTTPS://EXAMPLE.TEST",
    ] {
        assert_eq!(validate_url(url.to_owned()).unwrap(), url);
    }
    for url in [
        "ftp://example.test",
        "javascript:alert(1)",
        "https://",
        "https://example.test/a b",
        "",
    ] {
        assert!(
            matches!(validate_url(url.to_owned()), Err(DomainError::InvalidUrl)),
            "{url} should be refused"
        );
    }
    assert!(matches!(
        validate_url(format!("https://{}", "x".repeat(MAX_URL_CHARS))),
        Err(DomainError::TooLong { .. })
    ));
}

#[test]
fn an_edit_keeps_sets_and_clears_each_field_and_rechecks_the_repeat_rule() {
    use mg_calr::application::{Change, EventEdit};

    let (_directory, store) = scratch();
    let app = EventUseCases::new(store.clone());
    let calendar = app.create_calendar("Work").unwrap();
    let time = EventTime::timed(
        instant("2026-09-07T07:00:00-07:00"),
        instant("2026-09-07T07:30:00-07:00"),
        "America/Los_Angeles",
    )
    .unwrap();
    let event = app
        .create_detailed_event(
            calendar.id,
            "Standup",
            time.clone(),
            mg_calr::application::EventDetails {
                description: Some("First line\nsecond line".to_owned()),
                location: Some("Room 4".to_owned()),
                url: Some("https://example.test/standup".to_owned()),
                busy: true,
                recurrence: None,
            },
        )
        .unwrap();
    assert_eq!(event.metadata.location.as_deref(), Some("Room 4"));

    // an untouched field stays as it was; only what the edit names changes
    let edited = store
        .edit_event(
            event.id,
            event.version,
            &EventEdit {
                location: Change::Set("Room 9".to_owned()),
                busy: Some(false),
                ..EventEdit::default()
            },
        )
        .unwrap();
    assert_eq!(edited.metadata.location.as_deref(), Some("Room 9"));
    assert_eq!(
        edited.metadata.description.as_deref(),
        Some("First line\nsecond line")
    );
    assert!(!edited.metadata.busy);
    assert_eq!(edited.title, "Standup");

    // clearing is its own intention, distinct from leaving a field alone
    let cleared = store
        .edit_event(
            event.id,
            edited.version,
            &EventEdit {
                url: Change::Clear,
                ..EventEdit::default()
            },
        )
        .unwrap();
    assert_eq!(cleared.metadata.url, None);
    assert_eq!(cleared.metadata.location.as_deref(), Some("Room 9"));

    // an invalid value is refused and the stored event is untouched
    assert!(matches!(
        store.edit_event(
            event.id,
            cleared.version,
            &EventEdit {
                url: Change::Set("ftp://example.test".to_owned()),
                ..EventEdit::default()
            },
        ),
        Err(mg_calr::storage::StorageError::InvalidEdit { .. })
    ));
    assert_eq!(
        store.find_event(event.id).unwrap().unwrap().version,
        cleared.version,
        "a refused edit does not spend the version"
    );
}
