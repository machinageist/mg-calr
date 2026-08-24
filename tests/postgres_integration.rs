use mg_calr::config::{ConfigSource, ConnectionSettings};

#[tokio::test]
#[ignore = "requires explicit disposable PostgreSQL opt-in"]
async fn migration_is_idempotent_on_disposable_database() {
    assert_eq!(
        std::env::var("MG_CALR_RUN_DATABASE_TESTS").as_deref(),
        Ok("1")
    );
    let url = std::env::var("MG_CALR_TEST_DATABASE_URL")
        .expect("MG_CALR_TEST_DATABASE_URL must name a disposable database");
    assert!(
        url.contains("mg_calr_test"),
        "refusing to migrate a URL not visibly named mg_calr_test"
    );
    let settings = ConnectionSettings::Url {
        url,
        source: ConfigSource::Environment,
    };

    let first = mg_calr::storage::migrate(&settings).await.unwrap();
    let second = mg_calr::storage::migrate(&settings).await.unwrap();

    assert_eq!(first.len(), second.len());
    assert!(second.iter().all(|migration| migration.applied));
}
