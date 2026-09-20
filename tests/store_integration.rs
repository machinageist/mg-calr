// Author: Jeff
// Date: 2026-09-20
// Description: The store itself — migration, round trips, ordering, agenda overlap and the
//              refusals that protect the authority
// Notes: Each test owns a file in a throwaway directory. The PostgreSQL suite this replaces
//        needed a disposable server, an opt-in variable and a guard against pointing the tests
//        at a real database; a file needs none of that, so these always run

use chrono::Weekday;
use chrono::{DateTime, FixedOffset, NaiveDate};
use mg_calr::application::EventEdit;
use mg_calr::domain::{Calendar, CalendarId, Event, EventFrequency, EventRecurrence, EventTime};
use mg_calr::storage::{StorageError, Store};
use tempfile::TempDir;

fn instant(value: &str) -> DateTime<FixedOffset> {
    value.parse().expect("valid RFC3339 fixture")
}

// A migrated store in a throwaway directory, returned with it because dropping it deletes the file
fn scratch() -> (TempDir, Store) {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = Store::open(directory.path().join("calr.sqlite")).expect("store opens");
    store.migrate().expect("store migrates");
    (directory, store)
}

fn recurring_fixture(calendar_id: CalendarId) -> (Event, EventRecurrence) {
    let mut event = Event::new(
        calendar_id,
        "Recurring fixture",
        EventTime::timed(
            instant("2026-09-07T08:00:00-07:00"),
            instant("2026-09-07T08:15:00-07:00"),
            "America/Los_Angeles",
        )
        .unwrap(),
    )
    .unwrap();
    let rule = EventRecurrence::new(
        EventFrequency::Weekly,
        1,
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
    )
    .unwrap();
    event.metadata.recurrence_rule = Some(rule.clone());
    (event, rule)
}

#[test]
fn migrating_twice_applies_everything_once() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = Store::open(directory.path().join("calr.sqlite")).expect("store opens");

    assert!(
        store
            .migration_status()
            .unwrap()
            .iter()
            .all(|state| !state.applied),
        "opening never migrates: status only diagnoses"
    );
    for _ in 0..2 {
        assert!(store.migrate().unwrap().iter().all(|state| state.applied));
    }
    assert!(
        store
            .migration_status()
            .unwrap()
            .iter()
            .all(|state| state.applied)
    );
}

#[test]
fn calendars_and_events_round_trip_in_a_deterministic_order() {
    let (_directory, store) = scratch();
    let alpha = Calendar::new("Alpha").unwrap();
    let zulu = Calendar::new("Zulu").unwrap();
    store.save_calendar(&zulu).unwrap();
    store.save_calendar(&alpha).unwrap();

    let timed = Event::new(
        alpha.id,
        "Timed alias fixture",
        // an IANA alias must survive the round trip as the zone it names
        EventTime::timed(
            instant("2026-08-24T09:00:00-07:00"),
            instant("2026-08-24T10:00:00-07:00"),
            "US/Pacific",
        )
        .unwrap(),
    )
    .unwrap();
    let all_day = Event::new(
        alpha.id,
        "All day fixture",
        EventTime::all_day(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let (recurring, rule) = recurring_fixture(alpha.id);
    store.save_event(&timed).unwrap();
    store.save_event(&all_day).unwrap();
    store.save_event(&recurring).unwrap();

    // a rule must survive the JSON column, and an event without one must stay without one
    assert_eq!(
        store
            .find_event(recurring.id)
            .unwrap()
            .and_then(|event| event.metadata.recurrence_rule)
            .as_ref(),
        Some(&rule)
    );
    assert!(
        store
            .find_event(timed.id)
            .unwrap()
            .and_then(|event| event.metadata.recurrence_rule)
            .is_none()
    );

    assert_eq!(
        store
            .list_calendars()
            .unwrap()
            .iter()
            .map(|calendar| calendar.id)
            .collect::<Vec<_>>(),
        vec![alpha.id, zulu.id]
    );
    assert_eq!(
        store
            .list_events(Some(alpha.id))
            .unwrap()
            .iter()
            .map(|event| event.id)
            .filter(|id| *id != recurring.id)
            .collect::<Vec<_>>(),
        vec![all_day.id, timed.id]
    );
    assert_eq!(
        store.find_event(timed.id).unwrap().map(|event| event.time),
        Some(timed.time.clone())
    );
}

#[test]
fn a_day_agenda_holds_every_event_that_overlaps_the_day() {
    let (_directory, store) = scratch();
    let calendar = Calendar::new("Alpha").unwrap();
    store.save_calendar(&calendar).unwrap();
    let timed = Event::new(
        calendar.id,
        "Timed",
        EventTime::timed(
            instant("2026-08-24T09:00:00-07:00"),
            instant("2026-08-24T10:00:00-07:00"),
            "US/Pacific",
        )
        .unwrap(),
    )
    .unwrap();
    let all_day = Event::new(
        calendar.id,
        "All day",
        EventTime::all_day(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    store.save_event(&timed).unwrap();
    store.save_event(&all_day).unwrap();

    let agenda = store
        .day_agenda(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            instant("2026-08-24T00:00:00-07:00"),
            instant("2026-08-25T00:00:00-07:00"),
        )
        .unwrap();
    assert_eq!(agenda.len(), 2, "both the timed and the all-day event");
}

#[test]
fn an_event_whose_calendar_is_not_live_is_refused() {
    let (_directory, store) = scratch();
    let orphan = Event::new(
        CalendarId::new(),
        "Missing parent",
        EventTime::all_day(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();

    assert!(matches!(
        store.save_event(&orphan),
        Err(StorageError::CalendarNotLive { .. })
    ));
}

#[test]
fn an_edit_needs_the_version_the_writer_saw() {
    let (_directory, store) = scratch();
    let calendar = Calendar::new("Alpha").unwrap();
    store.save_calendar(&calendar).unwrap();
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
    store.save_event(&event).unwrap();

    let edit = EventEdit {
        title: Some("Daily standup".to_owned()),
        time: None,
    };
    let edited = store.edit_event(event.id, event.version, &edit).unwrap();
    assert_eq!(edited.title, "Daily standup");
    assert!(edited.version > event.version);

    // the version the writer observed is spent
    assert!(matches!(
        store.edit_event(event.id, event.version, &edit),
        Err(StorageError::EventVersionConflict { .. })
    ));

    let cancelled = store.cancel_event(event.id, edited.version).unwrap();
    assert!(cancelled.version > edited.version);
    // cancelling a cancelled event is a lifecycle answer, not a version one
    assert!(matches!(
        store.cancel_event(event.id, cancelled.version),
        Err(StorageError::EventNotFound { .. })
    ));
    let restored = store.restore_event(event.id, cancelled.version).unwrap();
    assert!(restored.version > cancelled.version);
}
