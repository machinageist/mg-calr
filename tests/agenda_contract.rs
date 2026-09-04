use chrono::NaiveDate;
use mg_calr::application::{AgendaKind, AgendaOutput, AgendaQuery, QueryError};
use mg_calr::domain::todo::{RecurrenceFrequency, RecurrenceRule, Todo, TodoDue};
use mg_calr::domain::{Calendar, Event, EventFrequency, EventRecurrence, EventTime};

fn date(value: &str) -> NaiveDate {
    value.parse().expect("valid date")
}

#[test]
fn agenda_combines_recurrence_and_events_in_stable_serializable_order() {
    let calendar = Calendar::new("work").unwrap();
    let event = Event::new(
        calendar.id,
        "Planning",
        EventTime::all_day(date("2026-08-24"), date("2026-08-25")).unwrap(),
    )
    .unwrap();
    let mut todo = Todo::new("Review").unwrap();
    todo.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());
    todo.recurrence =
        Some(RecurrenceRule::new(RecurrenceFrequency::Daily, 1, Some(2), None).unwrap());

    let output = AgendaOutput::from_snapshot(
        AgendaQuery::new(date("2026-08-24"), date("2026-08-26")),
        vec![event],
        vec![todo],
    )
    .unwrap();

    assert_eq!(output.items.len(), 3);
    assert!(matches!(output.items[0].kind, AgendaKind::Event));
    assert_eq!(output.items[1].occurrence_index, Some(0));
    assert_eq!(output.items[2].occurrence_index, Some(1));
    let json = serde_json::to_value(&output).unwrap();
    assert_eq!(json["items"][1]["kind"], "todo");
    assert_eq!(json["items"][1]["due"]["Date"]["timezone"], "UTC");
}

#[test]
fn agenda_filters_completed_trashed_and_blocked_todos_by_default() {
    let mut completed = Todo::new("Completed").unwrap();
    completed.completed_at = Some(chrono::Utc::now());
    completed.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());

    let mut trashed = Todo::new("Trashed").unwrap();
    trashed.trashed_at = Some(chrono::Utc::now());
    trashed.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());

    let prerequisite = Todo::new("Prerequisite").unwrap();
    let mut blocked = Todo::new("Blocked").unwrap();
    blocked.dependency_ids = vec![prerequisite.id];
    blocked.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());

    let mut query = AgendaQuery::new(date("2026-08-24"), date("2026-08-25"));
    query.include_completed = true;
    query.include_trashed = true;
    query.include_blocked = false;
    let output = AgendaOutput::from_snapshot(
        query,
        Vec::new(),
        vec![completed, trashed, prerequisite, blocked],
    )
    .unwrap();

    assert_eq!(
        output
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Completed", "Trashed"]
    );
}

#[test]
fn agenda_never_returns_deleted_events_even_when_trashed_is_included() {
    let calendar = Calendar::new("work").unwrap();
    let mut deleted = Event::new(
        calendar.id,
        "Deleted",
        EventTime::all_day(date("2026-08-24"), date("2026-08-25")).unwrap(),
    )
    .unwrap();
    deleted.deleted_at = Some(chrono::Utc::now());

    let mut query = AgendaQuery::new(date("2026-08-24"), date("2026-08-25"));
    query.include_trashed = true;
    let output = AgendaOutput::from_snapshot(query, vec![deleted], Vec::new()).unwrap();

    assert!(output.items.is_empty());
}

#[test]
fn agenda_sorts_timed_instants_by_query_timezone_civil_date() {
    let calendar = Calendar::new("work").unwrap();
    let shifted_event = Event::new(
        calendar.id,
        "Shifted event",
        EventTime::timed(
            "2026-08-24T00:30:00+00:00".parse().unwrap(),
            "2026-08-24T01:30:00+00:00".parse().unwrap(),
            "UTC",
        )
        .unwrap(),
    )
    .unwrap();
    let mut shifted_todo = Todo::new("Shifted todo").unwrap();
    shifted_todo.due =
        Some(TodoDue::timed("2026-08-24T00:30:00+00:00".parse().unwrap(), "UTC").unwrap());
    let day_event = Event::new(
        calendar.id,
        "Day event",
        EventTime::all_day(date("2026-08-24"), date("2026-08-25")).unwrap(),
    )
    .unwrap();

    let query = AgendaQuery::try_new(
        date("2026-08-23"),
        date("2026-08-25"),
        "America/Los_Angeles",
    )
    .unwrap();
    let output =
        AgendaOutput::from_snapshot(query, vec![day_event, shifted_event], vec![shifted_todo])
            .unwrap();

    assert_eq!(
        output
            .items
            .iter()
            .map(|item| item.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Shifted event", "Shifted todo", "Day event"]
    );
}

#[test]
fn agenda_rejects_invalid_timezone_and_unrepresentable_dst_boundary() {
    let invalid = AgendaQuery {
        timezone: "Not/AZone".to_owned(),
        ..AgendaQuery::new(date("2026-08-24"), date("2026-08-25"))
    };
    assert!(matches!(
        AgendaOutput::from_snapshot(invalid, Vec::new(), Vec::new()),
        Err(QueryError::InvalidTimezone { timezone }) if timezone == "Not/AZone"
    ));

    let skipped_day = AgendaQuery {
        timezone: "Pacific/Apia".to_owned(),
        ..AgendaQuery::new(date("2011-12-30"), date("2011-12-31"))
    };
    assert!(matches!(
        AgendaOutput::from_snapshot(skipped_day, Vec::new(), Vec::new()),
        Err(QueryError::InvalidDayBoundary {
            date: boundary_date,
            timezone,
        }) if boundary_date == date("2011-12-30") && timezone == "Pacific/Apia"
    ));
}

#[test]
fn agenda_timed_recurrence_uses_query_timezone_for_membership() {
    let mut todo = Todo::new("Overnight recurrence").unwrap();
    todo.due = Some(TodoDue::timed("2026-08-24T00:30:00+00:00".parse().unwrap(), "UTC").unwrap());
    todo.recurrence =
        Some(RecurrenceRule::new(RecurrenceFrequency::Daily, 1, Some(2), None).unwrap());

    let output = AgendaOutput::from_snapshot(
        AgendaQuery::try_new(
            date("2026-08-23"),
            date("2026-08-25"),
            "America/Los_Angeles",
        )
        .unwrap(),
        Vec::new(),
        vec![todo],
    )
    .unwrap();

    assert_eq!(output.items.len(), 2);
    assert_eq!(
        output
            .items
            .iter()
            .map(|item| item.occurrence_index)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1)]
    );
}

#[test]
fn agenda_completed_prerequisite_does_not_block_dependent() {
    let mut prerequisite = Todo::new("Completed prerequisite").unwrap();
    prerequisite.completed_at = Some(chrono::Utc::now());
    let mut dependent = Todo::new("Dependent").unwrap();
    dependent.dependency_ids = vec![prerequisite.id];
    dependent.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());

    let query = AgendaQuery {
        include_blocked: false,
        ..AgendaQuery::new(date("2026-08-24"), date("2026-08-25"))
    };
    let output =
        AgendaOutput::from_snapshot(query, Vec::new(), vec![prerequisite, dependent]).unwrap();

    assert_eq!(output.items.len(), 1);
    assert_eq!(output.items[0].title, "Dependent");
    assert!(!output.items[0].blocked);
}

#[test]
fn agenda_items_render_their_time_in_the_queried_zone() {
    let calendar = Calendar::new("work").unwrap();
    let standup = Event::new(
        calendar.id,
        "Standup",
        EventTime::timed(
            "2026-08-24T09:00:00-04:00".parse().unwrap(),
            "2026-08-24T09:30:00-04:00".parse().unwrap(),
            "America/New_York",
        )
        .unwrap(),
    )
    .unwrap();
    let mut call = Todo::new("Call the bank").unwrap();
    call.due = Some(
        TodoDue::timed(
            "2026-08-24T15:30:00-04:00".parse().unwrap(),
            "America/New_York",
        )
        .unwrap(),
    );
    let mut rent = Todo::new("Pay rent").unwrap();
    rent.due = Some(TodoDue::date(date("2026-08-24"), "America/New_York").unwrap());

    let output = AgendaOutput::from_snapshot(
        AgendaQuery::new(date("2026-08-24"), date("2026-08-25"))
            .with_timezone("US/Pacific")
            .unwrap(),
        vec![standup],
        vec![call, rent],
    )
    .unwrap();

    assert_eq!(output.timezone, "US/Pacific");
    let zone: chrono_tz::Tz = output.timezone.parse().unwrap();
    let rendered = output
        .items
        .iter()
        .map(|item| (item.title.clone(), item.when(zone), item.on(zone)))
        .collect::<Vec<_>>();

    // Eastern wall times are restated in the zone the day was asked for
    assert!(rendered.contains(&(
        "Standup".to_owned(),
        "06:00-06:30".to_owned(),
        Some(date("2026-08-24"))
    )));
    assert!(rendered.contains(&(
        "Call the bank".to_owned(),
        "12:30".to_owned(),
        Some(date("2026-08-24"))
    )));
    // An all-day value keeps its civil date rather than shifting across the offset
    assert!(rendered.contains(&(
        "Pay rent".to_owned(),
        "all-day".to_owned(),
        Some(date("2026-08-24"))
    )));
}

#[test]
fn agenda_items_state_a_closed_or_blocked_lifecycle() {
    let mut done = Todo::new("Water the plants").unwrap();
    done.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());
    done.completed_at = Some("2026-08-24T10:00:00Z".parse().unwrap());

    let output = AgendaOutput::from_snapshot(
        AgendaQuery {
            include_completed: true,
            ..AgendaQuery::new(date("2026-08-24"), date("2026-08-25"))
        },
        Vec::new(),
        vec![done],
    )
    .unwrap();

    let item = output
        .items
        .iter()
        .find(|item| item.kind == AgendaKind::Todo)
        .expect("completed todo is included");
    assert_eq!(item.notes(), "  (done)");
    assert_eq!(item.when("UTC".parse().unwrap()), "all-day");
}

#[test]
fn a_recurring_event_lists_every_occurrence_inside_the_window() {
    let calendar = Calendar::new("study").unwrap();
    let mut standup = Event::new(
        calendar.id,
        "Wake",
        EventTime::timed(
            "2026-09-07T08:00:00-07:00".parse().unwrap(),
            "2026-09-07T08:15:00-07:00".parse().unwrap(),
            "America/Los_Angeles",
        )
        .unwrap(),
    )
    .unwrap();
    standup.metadata.recurrence_rule = Some(
        EventRecurrence::new(
            EventFrequency::Weekly,
            1,
            Some(78),
            None,
            vec![
                chrono::Weekday::Mon,
                chrono::Weekday::Tue,
                chrono::Weekday::Wed,
                chrono::Weekday::Thu,
                chrono::Weekday::Fri,
                chrono::Weekday::Sat,
            ],
        )
        .unwrap(),
    );

    // One working week, so Sunday the 13th contributes nothing
    let output = AgendaOutput::from_snapshot(
        AgendaQuery::new(date("2026-09-07"), date("2026-09-14"))
            .with_timezone("America/Los_Angeles")
            .unwrap(),
        vec![standup],
        Vec::new(),
    )
    .unwrap();

    assert_eq!(output.items.len(), 6);
    let indexes: Vec<u32> = output
        .items
        .iter()
        .filter_map(|item| item.occurrence_index)
        .collect();
    assert_eq!(indexes, vec![0, 1, 2, 3, 4, 5]);
    for item in &output.items {
        assert_eq!(item.kind, AgendaKind::Event);
        assert_eq!(
            item.when("America/Los_Angeles".parse().unwrap()),
            "08:00-08:15"
        );
    }
}

#[test]
fn a_window_after_the_series_ends_lists_nothing() {
    let calendar = Calendar::new("study").unwrap();
    let mut weekly = Event::new(
        calendar.id,
        "Review",
        EventTime::timed(
            "2026-09-07T17:00:00-07:00".parse().unwrap(),
            "2026-09-07T18:00:00-07:00".parse().unwrap(),
            "America/Los_Angeles",
        )
        .unwrap(),
    )
    .unwrap();
    weekly.metadata.recurrence_rule =
        Some(EventRecurrence::new(EventFrequency::Weekly, 1, Some(2), None, Vec::new()).unwrap());

    let inside = AgendaOutput::from_snapshot(
        AgendaQuery::new(date("2026-09-07"), date("2026-09-21"))
            .with_timezone("America/Los_Angeles")
            .unwrap(),
        vec![weekly.clone()],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(inside.items.len(), 2);

    let after = AgendaOutput::from_snapshot(
        AgendaQuery::new(date("2026-10-05"), date("2026-10-12"))
            .with_timezone("America/Los_Angeles")
            .unwrap(),
        vec![weekly],
        Vec::new(),
    )
    .unwrap();
    assert!(after.items.is_empty());
}

#[test]
fn an_event_without_a_rule_keeps_its_single_instance_and_no_index() {
    let calendar = Calendar::new("work").unwrap();
    let once = Event::new(
        calendar.id,
        "Standup",
        EventTime::timed(
            "2026-09-07T09:00:00-07:00".parse().unwrap(),
            "2026-09-07T09:30:00-07:00".parse().unwrap(),
            "America/Los_Angeles",
        )
        .unwrap(),
    )
    .unwrap();

    let output = AgendaOutput::from_snapshot(
        AgendaQuery::new(date("2026-09-07"), date("2026-09-14"))
            .with_timezone("America/Los_Angeles")
            .unwrap(),
        vec![once],
        Vec::new(),
    )
    .unwrap();
    assert_eq!(output.items.len(), 1);
    assert_eq!(output.items[0].occurrence_index, None);
}

#[test]
fn recurring_occurrences_order_alongside_the_todos_already_there() {
    let calendar = Calendar::new("study").unwrap();
    let mut morning = Event::new(
        calendar.id,
        "Wake",
        EventTime::timed(
            "2026-09-07T08:00:00-07:00".parse().unwrap(),
            "2026-09-07T08:15:00-07:00".parse().unwrap(),
            "America/Los_Angeles",
        )
        .unwrap(),
    )
    .unwrap();
    morning.metadata.recurrence_rule =
        Some(EventRecurrence::new(EventFrequency::Daily, 1, Some(3), None, Vec::new()).unwrap());
    let mut errand = Todo::new("Pay rent").unwrap();
    errand.due = Some(
        TodoDue::timed(
            "2026-09-08T12:00:00-07:00".parse().unwrap(),
            "America/Los_Angeles",
        )
        .unwrap(),
    );

    let output = AgendaOutput::from_snapshot(
        AgendaQuery::new(date("2026-09-07"), date("2026-09-10"))
            .with_timezone("America/Los_Angeles")
            .unwrap(),
        vec![morning],
        vec![errand],
    )
    .unwrap();

    let zone: chrono_tz::Tz = "America/Los_Angeles".parse().unwrap();
    let ordered: Vec<(String, String)> = output
        .items
        .iter()
        .map(|item| (item.on(zone).unwrap().to_string(), item.title.clone()))
        .collect();
    assert_eq!(
        ordered,
        vec![
            ("2026-09-07".to_owned(), "Wake".to_owned()),
            ("2026-09-08".to_owned(), "Wake".to_owned()),
            ("2026-09-08".to_owned(), "Pay rent".to_owned()),
            ("2026-09-09".to_owned(), "Wake".to_owned()),
        ]
    );
}
