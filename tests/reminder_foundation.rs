use chrono::{TimeZone, Utc};
use mg_calr::{
    domain::reminder::{
        DeliveryChannel, DeliveryKey, OccurrenceKey, ScheduleRef, ScheduledReminder,
        plan_deliveries,
    },
    notify::{
        BackendError, NotSentCause, NotificationBackend, PresentationRequest, Urgency,
        null::NullBackend,
    },
};

fn key(second: u32) -> DeliveryKey {
    DeliveryKey::new(
        ScheduleRef::new("mg-remindr:reminder:fixture").unwrap(),
        OccurrenceKey::singleton(),
        Utc.with_ymd_and_hms(2026, 8, 31, 12, 0, second)
            .single()
            .unwrap(),
        DeliveryChannel::Null,
    )
}

fn request(summary: &str) -> PresentationRequest {
    PresentationRequest {
        summary: summary.to_owned(),
        body: "Synthetic reminder".to_owned(),
        actions: vec!["dismiss".to_owned()],
        urgency: Urgency::Normal,
        category: "x-mg-calr.reminder".to_owned(),
        expire_timeout_ms: -1,
        replaces: None,
        stack_tag: "fixture".to_owned(),
    }
}

#[test]
fn delivery_key_is_total_ordered_and_namespaced() {
    assert!(ScheduleRef::new("reminder:fixture").is_err());
    assert!(OccurrenceKey::new("").is_err());
    assert_eq!(key(1).channel.as_str(), "null");
    assert!(key(1) < key(2));
    assert_eq!(
        serde_json::to_value(key(1)).unwrap()["schedule_ref"],
        "mg-remindr:reminder:fixture"
    );
}

#[test]
fn planner_is_repeatable_deduplicated_and_omits_suppressed_schedules() {
    let input = || {
        vec![
            ScheduledReminder {
                key: key(2),
                suppressed: false,
            },
            ScheduledReminder {
                key: key(1),
                suppressed: false,
            },
            ScheduledReminder {
                key: key(1),
                suppressed: false,
            },
            ScheduledReminder {
                key: key(0),
                suppressed: true,
            },
        ]
    };
    let expected = vec![key(1), key(2)];
    assert_eq!(plan_deliveries(input()), expected);
    for _ in 0..1_000 {
        assert_eq!(plan_deliveries(input()), expected);
    }
}

#[tokio::test]
async fn null_backend_distinguishes_calls_from_rendered_presentations() {
    let backend = NullBackend::with_outcomes([
        Err(BackendError::NotSent(NotSentCause::Unavailable)),
        Ok(()),
    ]);

    assert!(backend.present(request("first")).await.is_err());
    let handle = backend.present(request("second")).await.unwrap();
    backend.close(handle).await.unwrap();

    assert_eq!(backend.calls().len(), 2);
    assert_eq!(backend.rendered(), vec![request("second")]);
    assert_eq!(backend.closed(), vec![handle]);
}
