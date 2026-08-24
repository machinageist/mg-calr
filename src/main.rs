use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use mg_calr::config;
use mg_calr::storage::{self, MigrationState};
use mg_calr::{AppError, Envelope, ErrorBody, ErrorEnvelope};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "mg-calr", version, about = "Local calendar foundation CLI")]
struct Cli {
    /// Emit the stable machine-readable JSON envelope.
    #[arg(long, global = true)]
    json: bool,
    /// Disable ANSI color. `NO_COLOR` also disables color.
    #[arg(long, global = true)]
    no_color: bool,
    /// Override the PostgreSQL connection for database commands.
    #[arg(long, global = true, value_name = "URL")]
    database_url: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print build version information.
    Version,
    /// Inspect configuration.
    Config(ConfigArgs),
    /// Run explicit database operations.
    Database(DatabaseArgs),
    /// Diagnose configuration and database readiness without migration.
    Doctor,
    /// Diagnose prerequisites and print administrator guidance; never runs sudo.
    Init,
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print resolved XDG paths.
    Paths,
}

#[derive(Debug, Args)]
struct DatabaseArgs {
    #[command(subcommand)]
    command: DatabaseCommand,
}

#[derive(Debug, Subcommand)]
enum DatabaseCommand {
    /// Apply all pending embedded migrations transactionally.
    Migrate,
    /// Report embedded migration state without applying migrations.
    Status,
}

#[derive(Debug, Serialize)]
struct VersionOutput {
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct DatabaseOutput {
    connection: String,
    migrations: Vec<MigrationState>,
}

#[derive(Debug, Serialize)]
struct DoctorOutput {
    connection: String,
    database_reachable: bool,
    migrations: Vec<MigrationState>,
    administrator_guidance: Vec<String>,
}

fn print_output<T: Serialize + std::fmt::Debug>(
    json: bool,
    command: &'static str,
    output: T,
) -> Result<(), AppError> {
    if json {
        println!(
            "{}",
            serde_json::to_string(&Envelope::success(command, output))?
        );
    } else {
        println!("{output:#?}");
    }
    Ok(())
}

async fn run(cli: &Cli) -> Result<(), AppError> {
    let _color_disabled = cli.no_color || std::env::var_os("NO_COLOR").is_some();
    let app_config = config::load(cli.database_url.clone())?;

    match &cli.command {
        Command::Version => print_output(
            cli.json,
            "version",
            VersionOutput {
                version: env!("CARGO_PKG_VERSION"),
            },
        ),
        Command::Config(ConfigArgs {
            command: ConfigCommand::Paths,
        }) => print_output(cli.json, "config.paths", app_config.paths),
        Command::Database(database) => {
            let (command, migrations) = match database.command {
                DatabaseCommand::Migrate => (
                    "database.migrate",
                    storage::migrate(&app_config.database).await?,
                ),
                DatabaseCommand::Status => (
                    "database.status",
                    storage::migration_status(&app_config.database).await?,
                ),
            };
            print_output(
                cli.json,
                command,
                DatabaseOutput {
                    connection: app_config.database.safe_summary(),
                    migrations,
                },
            )
        }
        Command::Doctor => {
            let migrations = storage::doctor(&app_config.database).await?;
            print_output(
                cli.json,
                "doctor",
                DoctorOutput {
                    connection: app_config.database.safe_summary(),
                    database_reachable: true,
                    migrations,
                    administrator_guidance: Vec::new(),
                },
            )
        }
        Command::Init => {
            let (database_reachable, migrations) = match storage::doctor(&app_config.database).await
            {
                Ok(migrations) => (true, migrations),
                Err(_) => (false, Vec::new()),
            };
            print_output(
                cli.json,
                "init",
                DoctorOutput {
                    connection: app_config.database.safe_summary(),
                    database_reachable,
                    migrations,
                    administrator_guidance: vec![
                        "Install PostgreSQL 18 using the operating system package manager.".to_owned(),
                        "Administrator example: sudo -u postgres createuser --login \"$USER\"".to_owned(),
                        "Administrator example: sudo -u postgres createdb --owner \"$USER\" mg_calr".to_owned(),
                        "Then, as the unprivileged application user: mg-calr database migrate".to_owned(),
                        "Review commands before running them; mg-calr never invokes sudo or provisions roles/databases.".to_owned(),
                    ],
                },
            )
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if cli.json {
                let message = error.to_string();
                let envelope = ErrorEnvelope {
                    schema_version: 1,
                    ok: false,
                    error: ErrorBody {
                        code: error.code(),
                        message: &message,
                    },
                };
                match serde_json::to_string(&envelope) {
                    Ok(json) => eprintln!("{json}"),
                    Err(_) => eprintln!("mg-calr: {message}"),
                }
            } else {
                eprintln!("mg-calr: {error}");
            }
            ExitCode::from(error.exit_code())
        }
    }
}
