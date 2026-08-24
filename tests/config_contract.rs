use std::collections::HashMap;
use std::path::PathBuf;

use mg_calr::config::{ConfigPaths, ConfigSource, resolve_config};

fn env(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn xdg_paths_use_each_distinct_base_directory() {
    let vars = env(&[
        ("HOME", "/home/tester"),
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_DATA_HOME", "/xdg/data"),
        ("XDG_STATE_HOME", "/xdg/state"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
    ]);

    let paths = ConfigPaths::from_env(&vars).expect("valid XDG environment");

    assert_eq!(paths.config_dir, PathBuf::from("/xdg/config/mg-calr"));
    assert_eq!(paths.data_dir, PathBuf::from("/xdg/data/mg-calr"));
    assert_eq!(paths.state_dir, PathBuf::from("/xdg/state/mg-calr"));
    assert_eq!(paths.cache_dir, PathBuf::from("/xdg/cache/mg-calr"));
    assert_eq!(
        paths.config_file,
        PathBuf::from("/xdg/config/mg-calr/config.toml")
    );
}

#[test]
fn xdg_paths_fall_back_under_home() {
    let vars = env(&[("HOME", "/home/tester")]);
    let paths = ConfigPaths::from_env(&vars).expect("HOME is sufficient");

    assert_eq!(
        paths.config_dir,
        PathBuf::from("/home/tester/.config/mg-calr")
    );
    assert_eq!(
        paths.data_dir,
        PathBuf::from("/home/tester/.local/share/mg-calr")
    );
    assert_eq!(
        paths.state_dir,
        PathBuf::from("/home/tester/.local/state/mg-calr")
    );
    assert_eq!(
        paths.cache_dir,
        PathBuf::from("/home/tester/.cache/mg-calr")
    );
}

#[test]
fn database_url_precedence_is_cli_then_environment_then_file_then_peer_default() {
    let vars = env(&[
        ("HOME", "/home/tester"),
        ("USER", "tester"),
        ("DATABASE_URL", "postgresql:///env"),
    ]);
    let file = r#"[database]
url = "postgresql:///file"
"#;

    let from_cli = resolve_config(&vars, Some(file), Some("postgresql:///cli".to_owned()))
        .expect("CLI config");
    assert_eq!(from_cli.database.source(), ConfigSource::Cli);

    let from_env = resolve_config(&vars, Some(file), None).expect("environment config");
    assert_eq!(from_env.database.source(), ConfigSource::Environment);

    let mut no_url_env = vars;
    no_url_env.remove("DATABASE_URL");
    let from_file = resolve_config(&no_url_env, Some(file), None).expect("file config");
    assert_eq!(from_file.database.source(), ConfigSource::File);

    let from_default = resolve_config(&no_url_env, None, None).expect("peer default");
    assert_eq!(from_default.database.source(), ConfigSource::Default);
    assert_eq!(from_default.database.dbname(), "mg_calr");
    assert_eq!(
        from_default.database.socket_dir(),
        Some(PathBuf::from("/run/postgresql").as_path())
    );
}
