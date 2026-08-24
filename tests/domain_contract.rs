use std::str::FromStr;

use mg_calr::domain::{CalendarId, DomainError, EventId, TodoId};

#[test]
fn identifiers_round_trip_without_losing_their_type() {
    let calendar = CalendarId::new();
    let event = EventId::new();
    let todo = TodoId::new();

    assert_eq!(
        CalendarId::from_str(&calendar.to_string()).unwrap(),
        calendar
    );
    assert_eq!(EventId::from_str(&event.to_string()).unwrap(), event);
    assert_eq!(TodoId::from_str(&todo.to_string()).unwrap(), todo);
}

#[test]
fn identifiers_serialize_as_canonical_strings_and_validate_on_decode() {
    let calendar = CalendarId::new();
    let encoded = serde_json::to_string(&calendar).unwrap();
    assert_eq!(encoded, format!("\"{calendar}\""));

    let decoded: CalendarId = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, calendar);

    let error = serde_json::from_str::<CalendarId>("\"not-a-uuid\"")
        .expect_err("malformed serialized IDs must be rejected");
    assert!(error.to_string().contains("invalid calendar identifier"));
}

#[test]
fn malformed_identifier_returns_typed_error() {
    let error = EventId::from_str("not-a-uuid").expect_err("must reject malformed UUID");
    assert!(matches!(
        error,
        DomainError::InvalidIdentifier { kind: "event", .. }
    ));
}
