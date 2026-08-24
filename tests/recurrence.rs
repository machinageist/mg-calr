use chrono::NaiveDate;
use mg_calr::domain::todo::{RecurrenceFrequency, RecurrenceRule, Todo, TodoDue, TodoError};

fn date(value: &str) -> NaiveDate {
    value.parse().unwrap()
}

#[test]
fn recurrence_expands_deterministically_without_mutating_base() {
    let mut todo = Todo::new("weekly review").unwrap();
    todo.due = Some(TodoDue::date(date("2026-08-24"), "UTC").unwrap());
    todo.recurrence =
        Some(RecurrenceRule::new(RecurrenceFrequency::Weekly, 1, Some(3), None).unwrap());
    let original = todo.clone();
    let instances = todo
        .expand_due_instances(date("2026-08-01"), date("2026-09-30"))
        .unwrap();
    assert_eq!(instances.len(), 3);
    assert_eq!(
        instances[1],
        TodoDue::date(date("2026-08-31"), "UTC").unwrap()
    );
    assert_eq!(todo, original);
}

#[test]
fn recurrence_rejects_unbounded_and_contradictory_rules() {
    assert_eq!(
        RecurrenceRule::new(RecurrenceFrequency::Daily, 0, Some(2), None),
        Err(TodoError::InvalidRecurrenceInterval)
    );
    assert_eq!(
        RecurrenceRule::new(RecurrenceFrequency::Daily, 1, None, None),
        Err(TodoError::InvalidRecurrenceCount)
    );
    let mut todo = Todo::new("bad recurrence").unwrap();
    todo.recurrence = Some(RecurrenceRule {
        frequency: RecurrenceFrequency::Daily,
        interval: 1,
        count: Some(2),
        until: None,
    });
    assert_eq!(todo.rehydrate(), Err(TodoError::RecurrenceWithoutDue));
}

#[test]
fn monthly_recurrence_is_local_and_stable_at_month_end() {
    let mut todo = Todo::new("month end").unwrap();
    todo.due = Some(TodoDue::date(date("2026-01-31"), "UTC").unwrap());
    todo.recurrence =
        Some(RecurrenceRule::new(RecurrenceFrequency::Monthly, 1, Some(3), None).unwrap());
    let instances = todo
        .expand_due_instances(date("2026-01-01"), date("2026-05-01"))
        .unwrap();
    assert_eq!(
        instances
            .iter()
            .map(|due| match due {
                TodoDue::Date { date, .. } => *date,
                TodoDue::Timed { .. } => unreachable!(),
            })
            .collect::<Vec<_>>(),
        vec![date("2026-01-31"), date("2026-02-28"), date("2026-03-28")]
    );
}
