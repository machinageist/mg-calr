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

/// Where the one SQLite file lives, and what said so.
#[derive(Debug, Clone)]
pub struct StoreSettings {
    pub path: PathBuf,
    pub source: ConfigSource,
}

impl StoreSettings {
    #[must_use]
    pub const fn source(&self) -> ConfigSource {
        self.source
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A description safe to print: a path this user already knows, and where it came from.
    #[must_use]
    pub fn safe_summary(&self) -> String {
        let source = self.source;
        format!("SQLite store {} ({source:?})", self.path.display())
    }
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub paths: ConfigPaths,
    pub database: StoreSettings,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    #[serde(default)]
    database: FileDatabase,
}

#[derive(Debug, Default, Deserialize)]
struct FileDatabase {
    path: Option<PathBuf>,
}

/// Resolve the complete application configuration with documented precedence.
///
/// # Errors
///
/// Returns an error when XDG paths cannot be resolved or TOML is invalid.
pub fn resolve_config<S: std::hash::BuildHasher>(
    vars: &HashMap<String, String, S>,
    file_contents: Option<&str>,
    cli_store_path: Option<PathBuf>,
) -> Result<AppConfig, ConfigError> {
    let paths = ConfigPaths::from_env(vars)?;
    let file = file_contents.map_or_else(|| Ok(FileConfig::default()), toml::from_str)?;
    // the argument wins, then the environment, then the file, then the XDG data directory
    let database = if let Some(path) = cli_store_path {
        StoreSettings {
            path,
            source: ConfigSource::Cli,
        }
    } else if let Some(path) = vars.get(crate::storage::DB_PATH_ENV) {
        StoreSettings {
            path: PathBuf::from(path),
            source: ConfigSource::Environment,
        }
    } else if let Some(path) = file.database.path {
        StoreSettings {
            path,
            source: ConfigSource::File,
        }
    } else {
        StoreSettings {
            path: paths.data_dir.join(crate::storage::DEFAULT_DB_FILE),
            source: ConfigSource::Default,
        }
    };
    Ok(AppConfig { paths, database })
}

/// Load configuration from the current process environment and optional file.
///
/// # Errors
///
/// Returns an error for unresolved XDG paths, unreadable files, or invalid TOML.
pub fn load(cli_store_path: Option<PathBuf>) -> Result<AppConfig, ConfigError> {
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
    resolve_config(&vars, contents.as_deref(), cli_store_path)
}
