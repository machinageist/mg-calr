use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const APP_DIR: &str = "mg-calr";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("HOME is unset and an XDG base directory is missing")]
    MissingHome,
    #[error("could not read configuration at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid TOML configuration: {0}")]
    InvalidToml(#[from] toml::de::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl ConfigPaths {
    /// Resolve all application paths from XDG environment values.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingHome`] when a required XDG base and
    /// `HOME` are both absent.
    pub fn from_env<S: std::hash::BuildHasher>(
        vars: &HashMap<String, String, S>,
    ) -> Result<Self, ConfigError> {
        let home = vars.get("HOME").map(PathBuf::from);
        let base = |key: &str, fallback: &str| -> Result<PathBuf, ConfigError> {
            vars.get(key).map_or_else(
                || {
                    home.as_ref()
                        .map(|path| path.join(fallback))
                        .ok_or(ConfigError::MissingHome)
                },
                |value| Ok(PathBuf::from(value)),
            )
        };

        let config_dir = base("XDG_CONFIG_HOME", ".config")?.join(APP_DIR);
        Ok(Self {
            config_file: config_dir.join("config.toml"),
            config_dir,
            data_dir: base("XDG_DATA_HOME", ".local/share")?.join(APP_DIR),
            state_dir: base("XDG_STATE_HOME", ".local/state")?.join(APP_DIR),
            cache_dir: base("XDG_CACHE_HOME", ".cache")?.join(APP_DIR),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSource {
    Cli,
    Environment,
    File,
    Default,
}

#[derive(Debug, Clone)]
pub enum ConnectionSettings {
    Url {
        url: String,
        source: ConfigSource,
    },
    Peer {
        socket_dir: PathBuf,
        user: Option<String>,
        dbname: String,
        source: ConfigSource,
    },
}

impl ConnectionSettings {
    #[must_use]
    pub const fn source(&self) -> ConfigSource {
        match self {
            Self::Url { source, .. } | Self::Peer { source, .. } => *source,
        }
    }

    #[must_use]
    pub fn dbname(&self) -> &str {
        match self {
            Self::Url { .. } => "from_url",
            Self::Peer { dbname, .. } => dbname,
        }
    }

    #[must_use]
    pub fn socket_dir(&self) -> Option<&Path> {
        match self {
            Self::Url { .. } => None,
            Self::Peer { socket_dir, .. } => Some(socket_dir),
        }
    }

    #[must_use]
    pub fn safe_summary(&self) -> String {
        match self {
            Self::Url { source, .. } => {
                format!("PostgreSQL URL ({source:?}; credentials redacted)")
            }
            Self::Peer {
                socket_dir,
                user,
                dbname,
                source,
            } => format!(
                "Unix socket {} database {dbname} user {} ({source:?})",
                socket_dir.display(),
                user.as_deref().unwrap_or("current OS user")
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub paths: ConfigPaths,
    pub database: ConnectionSettings,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    #[serde(default)]
    database: FileDatabase,
}

#[derive(Debug, Default, Deserialize)]
struct FileDatabase {
    url: Option<String>,
    socket_dir: Option<PathBuf>,
    user: Option<String>,
    dbname: Option<String>,
}

/// Resolve the complete application configuration with documented precedence.
///
/// # Errors
///
/// Returns an error when XDG paths cannot be resolved or TOML is invalid.
pub fn resolve_config<S: std::hash::BuildHasher>(
    vars: &HashMap<String, String, S>,
    file_contents: Option<&str>,
    cli_database_url: Option<String>,
) -> Result<AppConfig, ConfigError> {
    let paths = ConfigPaths::from_env(vars)?;
    let file = file_contents.map_or_else(|| Ok(FileConfig::default()), toml::from_str)?;
    let database = if let Some(url) = cli_database_url {
        ConnectionSettings::Url {
            url,
            source: ConfigSource::Cli,
        }
    } else if let Some(url) = vars.get("DATABASE_URL") {
        ConnectionSettings::Url {
            url: url.clone(),
            source: ConfigSource::Environment,
        }
    } else if let Some(url) = file.database.url {
        ConnectionSettings::Url {
            url,
            source: ConfigSource::File,
        }
    } else {
        let has_file_override = file.database.socket_dir.is_some()
            || file.database.user.is_some()
            || file.database.dbname.is_some();
        ConnectionSettings::Peer {
            socket_dir: file
                .database
                .socket_dir
                .unwrap_or_else(|| PathBuf::from("/run/postgresql")),
            user: file.database.user.or_else(|| vars.get("USER").cloned()),
            dbname: file.database.dbname.unwrap_or_else(|| "mg_calr".to_owned()),
            source: if has_file_override {
                ConfigSource::File
            } else {
                ConfigSource::Default
            },
        }
    };
    Ok(AppConfig { paths, database })
}

/// Load configuration from the current process environment and optional file.
///
/// # Errors
///
/// Returns an error for unresolved XDG paths, unreadable files, or invalid TOML.
pub fn load(cli_database_url: Option<String>) -> Result<AppConfig, ConfigError> {
    let vars = std::env::vars().collect::<HashMap<_, _>>();
    let paths = ConfigPaths::from_env(&vars)?;
    let contents = match fs::read_to_string(&paths.config_file) {
        Ok(contents) => Some(contents),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(ConfigError::Read {
                path: paths.config_file,
                source,
            });
        }
    };
    resolve_config(&vars, contents.as_deref(), cli_database_url)
}
