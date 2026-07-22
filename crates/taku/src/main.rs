mod cli;
mod console;

use std::path::Path;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use taku_runtime::Runtime;

use cli::{Cli, Command};

/// The full API registry: every crate the sandbox exposes to a Takufile.
/// The runtime depends only on taku-api; this list is the single place the
/// concrete crates are wired in.
static APIS: &[taku_api::ApiEntry] = &[
    taku_fs::API,
    taku_cmd::API,
    taku_net::API,
    taku_env::API,
    taku_ops::API,
];

const TEMPLATE: &str = r#"-- Takufile.lua — tasks for this project, written in Lua.
--
-- A task is a list of steps: command strings, step constructors (rm, cp,
-- write, unchanged, serve, ...), or an escape-hatch `function(ctx)`.
-- The header is "name <param=default>: dep1 dep2".
--
--   taku list            all tasks with their short docs
--   taku run <task>      run it (--dry-run to preview, --vars k=v to set params)
--
-- API reference & docs: https://taidaru.github.io/taku/

--- say hello
task "hello <name=world>" {
    echo "Hello, ${name}!",
}

--- build the project
--- skips itself when the inputs did not change
task "build" {
    unchanged { "src/**/*", outputs = "target" },
    "echo replace me with your build command",
}

--- run the test suite
task "test: build" {
    "echo replace me with your test command",
}

--- wipe and reseed the local database
task "db-reset" {
    confirm "wipe the local database?",
    "echo replace me with your db reset command",
}

--- start the dev server, wait until it answers
task "api" {
    serve {
        "echo replace me with your server command",
        -- ready = { http = "http://127.0.0.1:8000/health", timeout = 10 },
    },
}

--- everything a dev session needs
task "dev: build api" {}
"#;

fn main() -> ExitCode {
    console::init();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        // An unknown subcommand becomes our diagnostic with a `run <name>` hint;
        // clap handles everything else (usage errors, --help, --version) itself.
        Err(e) if e.kind() == clap::error::ErrorKind::InvalidSubcommand => {
            let name = invalid_subcommand(&e);
            let json = std::env::args().any(|a| a == "--json");
            eprintln!("{}", taku_runtime::render_unknown_command(&name, json));
            return ExitCode::FAILURE;
        }
        // A bad --jobs value (e.g. 0) gets the catalogue diagnostic, not clap's.
        Err(e) if e.kind() == clap::error::ErrorKind::ValueValidation && is_jobs_error(&e) => {
            let value = invalid_value(&e);
            let json = std::env::args().any(|a| a == "--json");
            eprintln!("{}", taku_runtime::render_bad_jobs(&value, json));
            return ExitCode::FAILURE;
        }
        Err(e) => e.exit(),
    };
    let json = cli.json;

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", taku_runtime::format_error(&e, json));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), taku_runtime::Error> {
    let json = cli.json;
    let quiet = cli.quiet;
    match cli.command {
        Some(Command::Init) => init(),
        Some(Command::Run {
            task,
            jobs,
            vars,
            yes,
            force,
            explain,
            dry_run,
        }) => {
            let vars = parse_vars(&vars)?;
            Runtime::load(APIS, json, quiet)?.run(
                &task,
                &taku_runtime::RunOpts {
                    jobs,
                    vars: &vars,
                    yes,
                    force,
                    explain,
                    dry_run,
                    json,
                    quiet,
                },
            )
        }
        Some(Command::List) => list(json, quiet),
        None => {
            let _ = Cli::command().print_help();
            println!();
            Ok(())
        }
    }
}

/// The subcommand clap rejected, pulled from its error context (falls back to
/// the raw string if the shape ever changes).
fn invalid_subcommand(e: &clap::Error) -> String {
    use clap::error::{ContextKind, ContextValue};
    match e.get(ContextKind::InvalidSubcommand) {
        Some(ContextValue::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// Whether a validation error is about the `--jobs` argument.
fn is_jobs_error(e: &clap::Error) -> bool {
    use clap::error::{ContextKind, ContextValue};
    matches!(
        e.get(ContextKind::InvalidArg),
        Some(ContextValue::String(s)) if s.contains("jobs")
    )
}

/// The offending value from a validation error.
fn invalid_value(e: &clap::Error) -> String {
    use clap::error::{ContextKind, ContextValue};
    match e.get(ContextKind::InvalidValue) {
        Some(ContextValue::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn parse_vars(vars: &[String]) -> Result<Vec<(String, String)>, taku_runtime::Error> {
    vars.iter()
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .ok_or_else(|| {
                    taku_runtime::Error::TaskFailed(format!("--vars expects KEY=VAL, got '{kv}'"))
                })
        })
        .collect()
}

fn init() -> Result<(), taku_runtime::Error> {
    let path = Path::new("Takufile.lua");
    if path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Takufile.lua already exists in this directory",
        )
        .into());
    }
    std::fs::write(path, TEMPLATE)?;
    println!("created Takufile.lua — run `taku list` to see its tasks");
    Ok(())
}

fn list(json: bool, quiet: bool) -> Result<(), taku_runtime::Error> {
    let tasks = Runtime::load(APIS, json, quiet)?.list()?;
    if json {
        use taku_api::steps::json_string;
        for (name, desc) in tasks {
            let doc = desc.map_or("null".to_string(), |d| json_string(&d));
            println!(
                "{{\"event\":\"task\",\"name\":{},\"doc\":{doc}}}",
                json_string(&name),
            );
        }
        return Ok(());
    }
    if tasks.is_empty() {
        println!("no tasks defined in the Takufile");
        return Ok(());
    }
    // chars(), not len(): byte width misaligns non-ASCII task names.
    let width = tasks
        .iter()
        .map(|(name, _)| name.chars().count())
        .max()
        .unwrap_or(0);
    for (name, desc) in tasks {
        match desc {
            Some(desc) => println!("  {name:<width$}  {desc}"),
            None => println!("  {name}"),
        }
    }
    Ok(())
}
