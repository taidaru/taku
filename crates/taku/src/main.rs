mod cli;
mod console;

use std::path::Path;
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use taku_runtime::Runtime;

use cli::{Cli, Command};

const TEMPLATE: &str = r#"-- Takufile.lua — tasks for this project, written in Lua.
--
-- Run a task with `taku run <name>`; list tasks with `taku list`.
-- Tasks may declare dependencies; independent ones run in parallel.
--
-- API reference & docs: https://taidaru.github.io/taku/

task("hello", function()
    print("Hello from taku!")
end)

task("build", {
    desc = "build the project",
    run = function()
        -- Commands are argument lists, run directly (no shell): { "prog", "arg", ... }.
        -- For example:
        --   local code = sh.run({ "cargo", "build" })
        --   if code ~= 0 then
        --       error("build failed (exit " .. code .. ")")
        --   end
        print("replace me with your build command")
    end,
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
        Some(Command::Run { task, jobs }) => Runtime::load()?.run(&task, jobs),
        Some(Command::List) => list(),
        None => {
            let _ = Cli::command().print_help();
            println!();
            Ok(())
        }
    }
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
    let tasks = Runtime::load()?.list()?;
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
