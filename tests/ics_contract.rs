use mg_calr::domain::{EventFrequency, EventTime};
use mg_calr::ics::{IcsError, read};

const DAILY: &str = include_str!("fixtures/daily-skeleton.ics");
const WEEKLY: &str = include_str!("fixtures/weekly-study-rhythm.ics");

#[test]
fn the_source_schedules_read_into_events_and_rules() {
    let daily = read(DAILY).expect("daily skeleton reads");
    let weekly = read(WEEKLY).expect("weekly rhythm reads");
    assert_eq!(daily.len(), 12);
    assert_eq!(weekly.len(), 24);

    // A VTIMEZONE carries FREQ=YEARLY;BYMONTH rules; none of them is an event
    for event in daily.iter().chain(weekly.iter()) {
        let rule = event.recurrence.as_ref().expect("every event repeats");
        assert_eq!(rule.frequency, EventFrequency::Weekly);
        assert_eq!(rule.interval, 1);
        assert!(rule.count.is_some());
        assert!(!rule.by_weekday.is_empty());
        assert!(event.uid.is_some());
    }
}

#[test]
fn every_occurrence_the_files_state_is_reachable() {
    let daily = read(DAILY).unwrap();
    let weekly = read(WEEKLY).unwrap();
    let from = "2026-09-01".parse().unwrap();
    let through = "2027-01-31".parse().unwrap();

    let count = |events: &[mg_calr::ics::IcsEvent]| -> usize {
        events
            .iter()
            .map(|event| {
                event
                    .recurrence
                    .as_ref()
                    .unwrap()
                    .expand(&event.time, from, through)
                    .unwrap()
                    .len()
            })
            .sum()
    };
    // Twelve blocks six days a week for thirteen weeks, and twenty-four weekly blocks
    assert_eq!(count(&daily), 12 * 78);
    assert_eq!(count(&weekly), 24 * 13);
}

#[test]
fn a_timed_event_keeps_its_zone_and_span() {
    let daily = read(DAILY).unwrap();
    let wake = daily
        .iter()
        .find(|event| event.title.contains("Wake"))
        .expect("the skeleton starts with waking");
    let EventTime::Timed {
        start,
        end,
        timezone,
    } = &wake.time
    else {
        panic!("expected a timed event");
    };
    assert_eq!(timezone, "America/Los_Angeles");
    assert_eq!(start.to_rfc3339(), "2026-09-07T08:00:00-07:00");
    assert_eq!((*end - *start).num_minutes(), 15);
    assert!(wake.description.is_some());
}

#[test]
fn folded_lines_and_escaped_text_are_restored() {
    let document = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:folded-1\r\n\
        SUMMARY:Long title that keeps\r\n  going\r\n\
        DESCRIPTION:First\\nSecond\\, third\r\n\
        DTSTART;VALUE=DATE:20260907\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = read(document).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].title, "Long title that keeps going");
    assert_eq!(
        events[0].description.as_deref(),
        Some("First\nSecond, third")
    );
    // An all-day event with no DTEND covers exactly one day
    assert_eq!(
        events[0].time,
        EventTime::all_day("2026-09-07".parse().unwrap(), "2026-09-08".parse().unwrap()).unwrap()
    );
}

fn one_event(body: &str) -> String {
    format!("BEGIN:VCALENDAR\nBEGIN:VEVENT\n{body}\nEND:VEVENT\nEND:VCALENDAR\n")
}

#[test]
fn rrule_parts_this_application_cannot_represent_are_named() {
    let cases = [
        ("BYMONTH=3", "BYMONTH"),
        ("BYSETPOS=-1", "BYSETPOS"),
        ("BYMONTHDAY=15", "BYMONTHDAY"),
        ("WKST=SU", "WKST"),
    ];
    for (part, named) in cases {
        let document = one_event(&format!(
            "SUMMARY:Nope\nDTSTART;TZID=America/Los_Angeles:20260907T080000\n\
             DTEND;TZID=America/Los_Angeles:20260907T081500\nRRULE:FREQ=WEEKLY;COUNT=5;{part}"
        ));
        assert_eq!(
            read(&document),
            Err(IcsError::UnsupportedRulePart {
                part: named.to_owned()
            }),
            "{part} should be refused by name"
        );
    }

    // The ordinal BYDAY form picks the nth weekday of a period, which this cannot do
    let ordinal = one_event(
        "SUMMARY:Nope\nDTSTART;TZID=America/Los_Angeles:20260907T080000\n\
         DTEND;TZID=America/Los_Angeles:20260907T081500\nRRULE:FREQ=MONTHLY;COUNT=5;BYDAY=1SU",
    );
    assert_eq!(
        read(&ordinal),
        Err(IcsError::UnsupportedRulePart {
            part: "BYDAY=1SU".to_owned()
        })
    );

    let yearly = one_event(
        "SUMMARY:Nope\nDTSTART;TZID=America/Los_Angeles:20260907T080000\n\
         DTEND;TZID=America/Los_Angeles:20260907T081500\nRRULE:FREQ=YEARLY;COUNT=5",
    );
    assert_eq!(
        read(&yearly),
        Err(IcsError::UnsupportedRulePart {
            part: "FREQ=YEARLY".to_owned()
        })
    );
}

#[test]
fn incomplete_and_malformed_documents_fail_closed() {
    let no_title = one_event("DTSTART;VALUE=DATE:20260907");
    assert_eq!(
        read(&no_title),
        Err(IcsError::MissingProperty { field: "SUMMARY" })
    );

    let no_start = one_event("SUMMARY:Nope");
    assert_eq!(
        read(&no_start),
        Err(IcsError::MissingProperty { field: "DTSTART" })
    );

    // A timed start with no end would otherwise become a zero-length event
    let no_end = one_event("SUMMARY:Nope\nDTSTART;TZID=America/Los_Angeles:20260907T080000");
    assert_eq!(read(&no_end), Err(IcsError::MissingEnd));

    let no_zone = one_event("SUMMARY:Nope\nDTSTART:20260907T080000");
    assert_eq!(
        read(&no_zone),
        Err(IcsError::MissingProperty { field: "TZID" })
    );

    let bad_zone = one_event(
        "SUMMARY:Nope\nDTSTART;TZID=Mars/Olympus:20260907T080000\n\
         DTEND;TZID=Mars/Olympus:20260907T081500",
    );
    assert_eq!(
        read(&bad_zone),
        Err(IcsError::UnknownTimezone {
            timezone: "Mars/Olympus".to_owned()
        })
    );

    assert_eq!(
        read("BEGIN:VCALENDAR\nBEGIN:VEVENT\nEND:VCALENDAR\n"),
        Err(IcsError::UnbalancedBlock {
            line: 3,
            name: "VCALENDAR".to_owned()
        })
    );
    assert_eq!(read("BEGIN:VCALENDAR\n"), Err(IcsError::UnclosedBlock));
    assert_eq!(
        read("not a property\n"),
        Err(IcsError::Malformed { line: 1 })
    );
}

#[test]
fn a_document_with_no_events_reads_as_empty_rather_than_failing() {
    let only_zone = "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:America/Los_Angeles\n\
        BEGIN:DAYLIGHT\nDTSTART:19700308T020000\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\n\
        END:DAYLIGHT\nEND:VTIMEZONE\nEND:VCALENDAR\n";
    assert_eq!(read(only_zone), Ok(Vec::new()));
}
