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
    unchanged { "src/**", outputs = "target" },
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

    let cli = Cli::parse();

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{}", taku_runtime::format_error(&e));
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), taku_runtime::Error> {
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
            Runtime::load(APIS)?.run(
                &task,
                &taku_runtime::RunOpts {
                    jobs,
                    vars: &vars,
                    yes,
                    force,
                    explain,
                    dry_run,
                },
            )
        }
        Some(Command::List) => list(),
        None => {
            let _ = Cli::command().print_help();
            println!();
            Ok(())
        }
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

fn list() -> Result<(), taku_runtime::Error> {
    let tasks = Runtime::load(APIS)?.list()?;
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
