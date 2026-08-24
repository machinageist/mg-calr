use std::fmt::Display;
use std::io::{self, Write};
use std::process::ExitCode;
use std::str::FromStr;

use chrono::{DateTime, FixedOffset, NaiveDate};
use clap::{Args, Parser, Subcommand};
use mg_calr::application::{
    ApplicationError, CalendarProjection, EventProjection, EventUseCases, ProjectUseCases,
    QueryError, TodoEdit, TodoUseCases,
};
use mg_calr::config;
use mg_calr::domain::todo::ProjectId;
use mg_calr::domain::todo::{Priority, TodoDue, TodoId};
use mg_calr::domain::{CalendarId, EventId, EventTime};
use mg_calr::storage::{
    self, MigrationState, PostgresCalendarEventRepository, PostgresProjectRepository,
    PostgresTodoRepository, StorageError,
};
use mg_calr::{AppError, Envelope, ErrorBody, ErrorEnvelope};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "mg-calr", version, about = "Local calendar foundation CLI")]
struct Cli {
    /// Emit the stable machine-readable JSON envelope.
    #[arg(long, global = true)]
    json: bool,
    /// Never prompt; fail when required input is absent.
    #[arg(long, global = true)]
    no_input: bool,
    /// Disable ANSI color. `NO_COLOR` also disables color.
    #[arg(long, global = true)]
    no_color: bool,
    /// Override the PostgreSQL connection.
    #[arg(long, global = true, value_name = "URL")]
    database_url: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
    Config(ConfigArgs),
    Database(DatabaseArgs),
    Doctor,
    Init,
    /// Create and list calendars.
    Calendar(CalendarArgs),
    /// Create and query events.
    Event(EventArgs),
    /// Create and query todos.
    Todo(TodoArgs),
    /// Create and list projects.
    Project(ProjectArgs),
}

#[derive(Debug, Args)]
struct CalendarArgs {
    #[command(subcommand)]
    command: CalendarCommand,
}

#[derive(Debug, Subcommand)]
enum CalendarCommand {
    /// Create a calendar. Missing name is prompted unless --no-input is set.
    Create(CalendarCreateArgs),
    /// List live calendars in stable order.
    List,
}

#[derive(Debug, Args)]
struct CalendarCreateArgs {
    #[arg(long)]
    name: Option<String>,
}

#[derive(Debug, Args)]
struct EventArgs {
    #[command(subcommand)]
    command: EventCommand,
}

#[derive(Debug, Subcommand)]
enum EventCommand {
    /// Create one explicit timed or all-day event.
    Create(EventCreateArgs),
    /// Show one live event by its full stable ID.
    Show { event_id: EventId },
    /// List live events, optionally scoped to a calendar.
    List {
        #[arg(long)]
        calendar: Option<CalendarId>,
    },
    /// Show events overlapping one local day in an explicit IANA timezone.
    DayAgenda {
        #[arg(long)]
        date: NaiveDate,
        #[arg(long)]
        timezone: String,
    },
}

#[derive(Debug, Args)]
struct ProjectArgs {
    #[command(subcommand)]
    command: ProjectCommand,
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Create a project. Missing name is prompted unless --no-input is set.
    Create { name: Option<String> },
    /// List live projects in stable order.
    List,
}

#[derive(Debug, Args)]
struct TodoArgs {
    #[command(subcommand)]
    command: TodoCommand,
}

#[derive(Debug, Subcommand)]
enum TodoCommand {
    /// Create one todo, optionally with a due date or instant.
    Create(TodoCreateArgs),
    /// List todos in stable repository order.
    List,
    /// Show one todo by its full stable ID.
    Show {
        #[arg(long)]
        todo_id: TodoId,
    },
    /// Complete one live todo using its current optimistic-lock version.
    Complete {
        #[arg(long)]
        todo_id: TodoId,
        #[arg(long)]
        version: i64,
    },
    /// Edit live core fields using its current optimistic-lock version.
    Edit(TodoEditArgs),
    /// Trash one live todo using its current optimistic-lock version.
    Trash {
        #[arg(long)]
        todo_id: TodoId,
        #[arg(long)]
        version: i64,
    },
    /// Restore one trashed todo using its current optimistic-lock version.
    Restore {
        #[arg(long)]
        todo_id: TodoId,
        #[arg(long)]
        version: i64,
    },
}

#[derive(Debug, Args)]
struct TodoCreateArgs {
    #[arg(long)]
    title: Option<String>,
    #[arg(long, default_value = "none")]
    priority: Priority,
    #[arg(long, conflicts_with = "due_at")]
    due_date: Option<NaiveDate>,
    #[arg(long, conflicts_with = "due_date")]
    due_at: Option<DateTime<FixedOffset>>,
    #[arg(long)]
    timezone: Option<String>,
}

#[derive(Debug, Args)]
struct TodoEditArgs {
    #[arg(long)]
    todo_id: TodoId,
    #[arg(long)]
    version: i64,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    priority: Option<Priority>,
    #[arg(long, conflicts_with = "due_at")]
    due_date: Option<NaiveDate>,
    #[arg(long, conflicts_with = "due_date")]
    due_at: Option<DateTime<FixedOffset>>,
    #[arg(long)]
    timezone: Option<String>,
    #[arg(long, conflicts_with = "clear_notes")]
    notes: Option<String>,
    #[arg(long, conflicts_with = "notes")]
    clear_notes: bool,
    #[arg(long, conflicts_with = "clear_project")]
    project_id: Option<ProjectId>,
    #[arg(long, conflicts_with = "project_id")]
    clear_project: bool,
}

#[derive(Debug, Args)]
struct EventCreateArgs {
    #[arg(long)]
    calendar: Option<CalendarId>,
    #[arg(long)]
    title: Option<String>,
    #[arg(long, conflicts_with_all = ["all_day_start", "all_day_end"])]
    start: Option<DateTime<FixedOffset>>,
    #[arg(long, conflicts_with_all = ["all_day_start", "all_day_end"])]
    end: Option<DateTime<FixedOffset>>,
    #[arg(long, conflicts_with_all = ["all_day_start", "all_day_end"])]
    timezone: Option<String>,
    #[arg(long, conflicts_with_all = ["start", "end", "timezone"])]
    all_day_start: Option<NaiveDate>,
    #[arg(long, conflicts_with_all = ["start", "end", "timezone"])]
    all_day_end: Option<NaiveDate>,
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Paths,
}

#[derive(Debug, Args)]
struct DatabaseArgs {
    #[command(subcommand)]
    command: DatabaseCommand,
}

#[derive(Debug, Subcommand)]
enum DatabaseCommand {
    Migrate,
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

fn print_debug<T: Serialize + std::fmt::Debug>(
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

fn print_projection<T: Serialize + Display>(
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
        println!("{output}");
    }
    Ok(())
}

fn print_projections<T: Serialize + Display>(
    json: bool,
    command: &'static str,
    output: Vec<T>,
) -> Result<(), AppError> {
    if json {
        println!(
            "{}",
            serde_json::to_string(&Envelope::success(command, output))?
        );
    } else {
        for item in output {
            println!("{item}");
        }
    }
    Ok(())
}

fn required<T: FromStr>(
    value: Option<T>,
    no_input: bool,
    field: &'static str,
    prompt_text: &str,
) -> Result<T, AppError>
where
    T::Err: Display,
{
    if let Some(value) = value {
        return Ok(value);
    }
    if no_input {
        return Err(AppError::RequiredInput { field });
    }
    let value = prompt(prompt_text)?;
    value
        .parse()
        .map_err(|error: T::Err| AppError::InvalidInput(format!("{field}: {error}")))
}

fn prompt(prompt_text: &str) -> Result<String, AppError> {
    eprint!("{prompt_text}: ");
    io::stderr().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "{prompt_text} must not be empty"
        )));
    }
    Ok(value)
}

fn event_time(args: &EventCreateArgs, no_input: bool) -> Result<EventTime, AppError> {
    let has_timed = args.start.is_some() || args.end.is_some() || args.timezone.is_some();
    let has_all_day = args.all_day_start.is_some() || args.all_day_end.is_some();
    let kind = if has_timed {
        "timed".to_owned()
    } else if has_all_day {
        "all-day".to_owned()
    } else if no_input {
        return Err(AppError::RequiredInput {
            field: "event time (--start/--end/--timezone or --all-day-start/--all-day-end)",
        });
    } else {
        let kind = prompt("Event kind (timed/all-day)")?;
        match kind.as_str() {
            "timed" | "all-day" => kind,
            _ => {
                return Err(AppError::InvalidInput(
                    "event kind must be 'timed' or 'all-day'".to_owned(),
                ));
            }
        }
    };

    if kind == "timed" {
        let start = required(args.start, no_input, "start", "Start (RFC3339)")?;
        let end = required(args.end, no_input, "end", "End (RFC3339)")?;
        let timezone = required(args.timezone.clone(), no_input, "timezone", "IANA timezone")?;
        EventTime::timed(start, end, timezone).map_err(AppError::from)
    } else {
        let start = required(
            args.all_day_start,
            no_input,
            "all-day start",
            "All-day start (YYYY-MM-DD)",
        )?;
        let end = required(
            args.all_day_end,
            no_input,
            "all-day end",
            "All-day exclusive end (YYYY-MM-DD)",
        )?;
        EventTime::all_day(start, end).map_err(AppError::from)
    }
}

fn todo_due(args: &TodoCreateArgs, no_input: bool) -> Result<Option<TodoDue>, AppError> {
    if args.due_date.is_none() && args.due_at.is_none() {
        if args.timezone.is_some() {
            return Err(AppError::InvalidInput(
                "--timezone requires --due-date or --due-at".to_owned(),
            ));
        }
        return Ok(None);
    }
    let timezone = required(args.timezone.clone(), no_input, "timezone", "IANA timezone")?;
    match (args.due_date, args.due_at) {
        (Some(date), None) => TodoDue::date(date, timezone)
            .map(Some)
            .map_err(AppError::from),
        (None, Some(at)) => TodoDue::timed(at, timezone)
            .map(Some)
            .map_err(AppError::from),
        _ => unreachable!("clap prevents both todo due forms"),
    }
}

fn todo_edit(args: &TodoEditArgs) -> Result<TodoEdit, AppError> {
    let due = if args.due_date.is_none() && args.due_at.is_none() {
        if args.timezone.is_some() {
            return Err(AppError::InvalidInput(
                "--timezone requires --due-date or --due-at".to_owned(),
            ));
        }
        None
    } else {
        let timezone = args.timezone.clone().ok_or_else(|| {
            AppError::InvalidInput("--due-date or --due-at requires --timezone".to_owned())
        })?;
        match (args.due_date, args.due_at) {
            (Some(date), None) => Some(TodoDue::date(date, timezone)?),
            (None, Some(at)) => Some(TodoDue::timed(at, timezone)?),
            _ => unreachable!("clap prevents both todo due forms"),
        }
    };
    let notes = if args.clear_notes {
        Some(None)
    } else {
        args.notes.clone().map(Some)
    };
    let project_id = if args.clear_project {
        Some(None)
    } else {
        args.project_id.map(Some)
    };
    let edit = TodoEdit {
        title: args.title.clone(),
        priority: args.priority,
        due,
        notes,
        project_id,
    };
    if edit.is_empty() {
        return Err(AppError::InvalidInput(
            "todo edit requires at least one editable field".to_owned(),
        ));
    }
    Ok(edit)
}

fn application_error(error: ApplicationError<StorageError>) -> AppError {
    match error {
        ApplicationError::Domain(error) => AppError::Domain(error),
        ApplicationError::Todo(error) => AppError::Todo(error),
        ApplicationError::Repository(error) => AppError::Storage(error),
    }
}

fn query_error(error: QueryError<StorageError>) -> AppError {
    match error {
        QueryError::EventNotFound { event_id } => AppError::EventNotFound { event_id },
        QueryError::TodoNotFound { todo_id } => AppError::TodoNotFound { todo_id },
        QueryError::ProjectNotFound { project_id } => {
            AppError::InvalidInput(format!("project {project_id} was not found"))
        }
        QueryError::InvalidTimezone { .. } | QueryError::InvalidDayBoundary { .. } => {
            AppError::InvalidInput(error.to_string())
        }
        QueryError::Repository(error) => AppError::Storage(error),
    }
}

async fn run_calendar_command(
    args: &CalendarArgs,
    database: config::ConnectionSettings,
    json: bool,
    no_input: bool,
) -> Result<(), AppError> {
    let app = EventUseCases::new(PostgresCalendarEventRepository::new(database));
    match &args.command {
        CalendarCommand::Create(args) => {
            let name = required(
                args.name.clone(),
                no_input,
                "calendar name",
                "Calendar name",
            )?;
            let calendar = app
                .create_calendar_async(name)
                .await
                .map_err(application_error)?;
            print_projection(json, "calendar.create", CalendarProjection::from(calendar))
        }
        CalendarCommand::List => print_projections(
            json,
            "calendar.list",
            app.list_calendars_async().await.map_err(query_error)?,
        ),
    }
}

async fn run_event_command(
    args: &EventArgs,
    database: config::ConnectionSettings,
    json: bool,
    no_input: bool,
) -> Result<(), AppError> {
    let create_input = if let EventCommand::Create(args) = &args.command {
        Some((
            required(args.calendar, no_input, "calendar", "Calendar ID")?,
            required(args.title.clone(), no_input, "title", "Event title")?,
            event_time(args, no_input)?,
        ))
    } else {
        None
    };
    let app = EventUseCases::new(PostgresCalendarEventRepository::new(database));
    match &args.command {
        EventCommand::Create(_) => {
            let (calendar_id, title, time) = create_input.expect("create input exists");
            let event = app
                .create_event_async(calendar_id, title, time)
                .await
                .map_err(application_error)?;
            print_projection(json, "event.create", EventProjection::from(event))
        }
        EventCommand::Show { event_id } => print_projection(
            json,
            "event.show",
            app.show_event_async(*event_id).await.map_err(query_error)?,
        ),
        EventCommand::List { calendar } => print_projections(
            json,
            "event.list",
            app.list_events_async(*calendar)
                .await
                .map_err(query_error)?,
        ),
        EventCommand::DayAgenda { date, timezone } => print_projections(
            json,
            "event.day-agenda",
            app.day_agenda_async(*date, timezone)
                .await
                .map_err(query_error)?,
        ),
    }
}

async fn run_project_command(
    args: &ProjectArgs,
    database: config::ConnectionSettings,
    json: bool,
    no_input: bool,
) -> Result<(), AppError> {
    let app = ProjectUseCases::new(PostgresProjectRepository::new(database));
    match &args.command {
        ProjectCommand::Create { name } => {
            let name = required(name.clone(), no_input, "project name", "Project name")?;
            print_projection(
                json,
                "project.create",
                app.create_project_async(name)
                    .await
                    .map_err(application_error)?,
            )
        }
        ProjectCommand::List => print_projections(
            json,
            "project.list",
            app.list_projects_async().await.map_err(query_error)?,
        ),
    }
}

async fn run_todo_command(
    args: &TodoArgs,
    database: config::ConnectionSettings,
    json: bool,
    no_input: bool,
) -> Result<(), AppError> {
    match &args.command {
        TodoCommand::Create(args) => {
            let title = required(args.title.clone(), no_input, "title", "Todo title")?;
            let due = todo_due(args, no_input)?;
            let todo = TodoUseCases::new(PostgresTodoRepository::new(database))
                .create_todo_async(title, args.priority, due)
                .await
                .map_err(application_error)?;
            print_projection(json, "todo.create", todo)
        }
        TodoCommand::List => print_projections(
            json,
            "todo.list",
            TodoUseCases::new(PostgresTodoRepository::new(database))
                .list_todos_async()
                .await
                .map_err(query_error)?,
        ),
        TodoCommand::Show { todo_id } => print_projection(
            json,
            "todo.show",
            TodoUseCases::new(PostgresTodoRepository::new(database))
                .show_todo_async(*todo_id)
                .await
                .map_err(query_error)?,
        ),
        TodoCommand::Complete { todo_id, version } => print_projection(
            json,
            "todo.complete",
            TodoUseCases::new(PostgresTodoRepository::new(database))
                .complete_todo_async(*todo_id, *version)
                .await
                .map_err(application_error)?,
        ),
        TodoCommand::Edit(args) => {
            let edit = todo_edit(args)?;
            print_projection(
                json,
                "todo.edit",
                TodoUseCases::new(PostgresTodoRepository::new(database))
                    .edit_todo_async(args.todo_id, args.version, edit)
                    .await
                    .map_err(application_error)?,
            )
        }
        TodoCommand::Trash { todo_id, version } => print_projection(
            json,
            "todo.trash",
            TodoUseCases::new(PostgresTodoRepository::new(database))
                .trash_todo_async(*todo_id, *version)
                .await
                .map_err(application_error)?,
        ),
        TodoCommand::Restore { todo_id, version } => print_projection(
            json,
            "todo.restore",
            TodoUseCases::new(PostgresTodoRepository::new(database))
                .restore_todo_async(*todo_id, *version)
                .await
                .map_err(application_error)?,
        ),
    }
}

async fn run(cli: &Cli) -> Result<(), AppError> {
    let _color_disabled = cli.no_color || std::env::var_os("NO_COLOR").is_some();
    let app_config = config::load(cli.database_url.clone())?;

    match &cli.command {
        Command::Version => print_debug(
            cli.json,
            "version",
            VersionOutput {
                version: env!("CARGO_PKG_VERSION"),
            },
        ),
        Command::Config(ConfigArgs {
            command: ConfigCommand::Paths,
        }) => print_debug(cli.json, "config.paths", app_config.paths),
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
            print_debug(
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
            print_debug(
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
            print_debug(
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
        Command::Calendar(calendar) => {
            run_calendar_command(calendar, app_config.database, cli.json, cli.no_input).await
        }
        Command::Event(event) => {
            run_event_command(event, app_config.database, cli.json, cli.no_input).await
        }
        Command::Todo(todo) => {
            run_todo_command(todo, app_config.database, cli.json, cli.no_input).await
        }
        Command::Project(project) => {
            run_project_command(project, app_config.database, cli.json, cli.no_input).await
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
