use chrono::Weekday;
use chrono::{DateTime, FixedOffset, NaiveDate};
use mg_calr::config::{ConfigSource, ConnectionSettings};
use mg_calr::domain::{Calendar, CalendarId, Event, EventFrequency, EventRecurrence, EventTime};
use mg_calr::storage::{PostgresCalendarEventRepository, StorageError};
use tokio_postgres::{Client, NoTls};

fn opted_in_settings() -> ConnectionSettings {
    assert_eq!(
        std::env::var("MG_CALR_RUN_DATABASE_TESTS").as_deref(),
        Ok("1")
    );
    let url = std::env::var("MG_CALR_TEST_DATABASE_URL")
        .expect("MG_CALR_TEST_DATABASE_URL must name a disposable database");
    let settings = ConnectionSettings::Url {
        url,
        source: ConfigSource::Environment,
    };
    assert_eq!(
        test_postgres_config(&settings).get_dbname(),
        Some("mg_calr_test"),
        "refusing to use a database other than mg_calr_test"
    );
    settings
}

fn test_postgres_config(settings: &ConnectionSettings) -> tokio_postgres::Config {
    let mut config = tokio_postgres::Config::new();
    match settings {
        ConnectionSettings::Url { url, .. }
            if url == "postgresql:///mg_calr_test" || url == "postgres:///mg_calr_test" =>
        {
            config.host_path("/run/postgresql").dbname("mg_calr_test");
        }
        ConnectionSettings::Url { url, .. } => {
            config = url.parse().expect("validated PostgreSQL test URL");
        }
        ConnectionSettings::Peer {
            socket_dir,
            user,
            dbname,
            ..
        } => {
            config.host_path(socket_dir).dbname(dbname);
            if let Some(user) = user {
                config.user(user);
            }
        }
    }
    config
}

async fn cleanup_client(settings: &ConnectionSettings) -> Client {
    let (client, connection) = test_postgres_config(settings)
        .connect(NoTls)
        .await
        .expect("test database connects");
    tokio::spawn(connection);
    client
}

#[test]
fn disposable_guard_uses_effective_database_name_not_url_substrings() {
    let settings = ConnectionSettings::Url {
        url: "postgresql:///production?application_name=mg_calr_test".to_owned(),
        source: ConfigSource::Environment,
    };

    assert_ne!(
        test_postgres_config(&settings).get_dbname(),
        Some("mg_calr_test")
    );
}

fn instant(value: &str) -> DateTime<FixedOffset> {
    value.parse().expect("valid RFC3339 fixture")
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL opt-in"]
async fn migration_is_idempotent_on_disposable_database() {
    let settings = opted_in_settings();

    let first = mg_calr::storage::migrate(&settings).await.unwrap();
    let second = mg_calr::storage::migrate(&settings).await.unwrap();

    assert_eq!(first.len(), second.len());
    assert!(second.iter().all(|migration| migration.applied));
}

/// The bug this guards: the ledger bootstrap used to run on a bare connection
/// before the transaction opened, so it sat outside the advisory lock. Concurrent
/// `CREATE TABLE IF NOT EXISTS` is not race-safe in PostgreSQL — two sessions can
/// both find the table absent and both attempt it — and the loser got a raw 42P07
/// surfaced as a generic query error. It only ever bit on a genuinely fresh
/// database with two callers, which is why running tests single-threaded hid it.
#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL opt-in"]
async fn concurrent_migrations_of_a_fresh_database_both_succeed() {
    // Its own database: the race needs an absent ledger, and emptying the shared
    // one would pull the schema out from under every test running beside it
    let shared = opted_in_settings();
    let name = format!("mg_calr_test_race_{}", std::process::id());
    let settings = disposable_database(&shared, &name).await;

    let first = mg_calr::storage::migrate(&settings);
    let second = mg_calr::storage::migrate(&settings);
    let (left, right) = tokio::join!(first, second);

    // Drop before asserting, so a failure still cleans up after itself
    let outcome = (left.is_ok(), right.is_ok());
    let counts = (left.map(|a| a.len()).ok(), right.map(|b| b.len()).ok());
    drop_database(&shared, &name).await;

    assert_eq!(
        outcome,
        (true, true),
        "both concurrent migrations of a fresh database must succeed"
    );
    assert_eq!(counts.0.unwrap(), counts.1.unwrap());
}

/// Create an empty database beside the opted-in one, so a test that needs an
/// absent schema cannot disturb anything sharing the main one.
async fn disposable_database(shared: &ConnectionSettings, name: &str) -> ConnectionSettings {
    assert!(
        name.starts_with("mg_calr_test"),
        "a disposable database must be named for the test suite"
    );
    let client = cleanup_client(shared).await;
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name}"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create the disposable race database");

    let config = test_postgres_config(shared);
    let host = config
        .get_hosts()
        .first()
        .map_or_else(String::new, |host| match host {
            tokio_postgres::config::Host::Unix(path) => path.display().to_string(),
            tokio_postgres::config::Host::Tcp(name) => name.clone(),
        });
    ConnectionSettings::Url {
        url: format!("postgresql:///{name}?host={host}"),
        source: ConfigSource::Environment,
    }
}

async fn drop_database(shared: &ConnectionSettings, name: &str) {
    let client = cleanup_client(shared).await;
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name}"))
        .await;
}

/// A weekly rule with a weekday set, the shape the imported schedules use.
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

/// A rule must survive the jsonb column, and an event without one must stay without one.
async fn prove_recurrence_round_trip(
    repository: &PostgresCalendarEventRepository,
    recurring: mg_calr::domain::EventId,
    plain: mg_calr::domain::EventId,
    rule: &EventRecurrence,
) -> Result<(), StorageError> {
    let stored = repository.find_event(recurring).await?;
    if stored
        .and_then(|event| event.metadata.recurrence_rule)
        .as_ref()
        != Some(rule)
    {
        return Err(StorageError::InvalidStoredData(
            "recurrence rule did not round-trip".to_owned(),
        ));
    }
    if repository
        .find_event(plain)
        .await?
        .and_then(|event| event.metadata.recurrence_rule)
        .is_some()
    {
        return Err(StorageError::InvalidStoredData(
            "a non-recurring event gained a rule".to_owned(),
        ));
    }
    Ok(())
}

async fn exercise_repository(
    repository: &PostgresCalendarEventRepository,
    alpha: &Calendar,
    zulu: &Calendar,
) -> Result<(), StorageError> {
    repository.save_calendar(zulu).await?;
    repository.save_calendar(alpha).await?;
    let timed = Event::new(
        alpha.id,
        "Timed alias fixture",
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

    repository.save_event(&timed).await?;
    repository.save_event(&all_day).await?;
    repository.save_event(&recurring).await?;
    prove_recurrence_round_trip(repository, recurring.id, timed.id, &rule).await?;

    let calendars = repository.list_calendars().await?;
    let selected = calendars
        .iter()
        .filter(|calendar| calendar.id == alpha.id || calendar.id == zulu.id)
        .map(|calendar| calendar.id)
        .collect::<Vec<_>>();
    if selected != vec![alpha.id, zulu.id] {
        return Err(StorageError::InvalidStoredData(
            "calendar ordering contract failed".to_owned(),
        ));
    }
    let events = repository.list_events(Some(alpha.id)).await?;
    if events
        .iter()
        .map(|event| event.id)
        .filter(|id| *id != recurring.id)
        .collect::<Vec<_>>()
        != vec![all_day.id, timed.id]
    {
        return Err(StorageError::InvalidStoredData(
            "event ordering contract failed".to_owned(),
        ));
    }
    let shown = repository.find_event(timed.id).await?;
    if shown.as_ref().map(|event| &event.time) != Some(&timed.time) {
        return Err(StorageError::InvalidStoredData(
            "timed event or IANA alias did not round trip".to_owned(),
        ));
    }
    let agenda = repository
        .day_agenda(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            "US/Pacific",
            instant("2026-08-24T00:00:00-07:00"),
            instant("2026-08-25T00:00:00-07:00"),
        )
        .await?;
    if agenda
        .iter()
        .filter(|event| event.calendar_id == alpha.id)
        .count()
        != 2
    {
        return Err(StorageError::InvalidStoredData(
            "day agenda overlap contract failed".to_owned(),
        ));
    }
    let missing_parent = Event::new(
        CalendarId::new(),
        "Missing parent",
        EventTime::all_day(
            NaiveDate::from_ymd_opt(2026, 8, 24).unwrap(),
            NaiveDate::from_ymd_opt(2026, 8, 25).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    if !matches!(
        repository.save_event(&missing_parent).await,
        Err(StorageError::CalendarNotLive { .. })
    ) {
        return Err(StorageError::InvalidStoredData(
            "missing parent did not return CalendarNotLive".to_owned(),
        ));
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL opt-in"]
async fn repository_reads_round_trip_order_and_clean_up_test_rows() {
    let settings = opted_in_settings();
    mg_calr::storage::migrate(&settings).await.unwrap();
    let repository = PostgresCalendarEventRepository::new(settings.clone());
    let alpha = Calendar::new(format!("A integration {}", uuid::Uuid::now_v7())).unwrap();
    let zulu = Calendar::new(format!("Z integration {}", uuid::Uuid::now_v7())).unwrap();
    let alpha_id = alpha.id;
    let zulu_id = zulu.id;

    let exercise = exercise_repository(&repository, &alpha, &zulu).await;

    let client = cleanup_client(&settings).await;
    client
        .execute(
            "DELETE FROM events WHERE calendar_id = $1 OR calendar_id = $2",
            &[&alpha_id.as_uuid(), &zulu_id.as_uuid()],
        )
        .await
        .expect("test events clean up");
    client
        .execute(
            "DELETE FROM calendars WHERE id = $1 OR id = $2",
            &[&alpha_id.as_uuid(), &zulu_id.as_uuid()],
        )
        .await
        .expect("test calendars clean up");

    exercise.expect("repository runtime contract");
}
