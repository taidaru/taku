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
-- write, ...), or an escape-hatch function. Deps go after `:` in the header.
-- Run a task with `taku run <name>`; list tasks with `taku list`.
--
-- API reference & docs: https://taidaru.github.io/taku/

--- say hello
task("hello <name=world>", {
    echo "Hello, ${name}!",
})

--- build the project
task("build", {
    "echo replace me with your build command",
})
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
        }) => {
            let vars = parse_vars(&vars)?;
            Runtime::load(APIS)?.run(
                &task,
                &taku_runtime::RunOpts {
                    jobs,
                    vars: &vars,
                    yes,
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
