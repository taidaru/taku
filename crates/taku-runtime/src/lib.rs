mod diagnostic;
mod error;
mod plan;
mod report;
mod schedule;
mod state;

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

use mlua::{Lua, Table};

pub use error::Error;

use state::{TASKS_KEY, build_state, find_takufile};

pub struct Runtime {
    lua: Lua,
    path: PathBuf,
    source: String,
}

impl Runtime {
    pub fn load() -> Result<Runtime, Error> {
        let path = find_takufile().ok_or(Error::TakufileNotFound)?;
        let source = std::fs::read_to_string(&path).map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("{}: {e}", path.display()),
            ))
        })?;
        let lua = build_state(&path, &source, true)?;
        Ok(Runtime { lua, path, source })
    }

    pub fn run(&self, command: &str, jobs: Option<NonZeroUsize>) -> Result<(), Error> {
        let plan = plan::build(&self.lua, &self.path, command)?;
        let style = report::Style::init();

        let start = Instant::now();
        let ran = schedule::execute(&style, &self.path, &self.source, &plan, jobs)?;
        report::summary(&style, ran, start.elapsed());
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<(String, Option<String>)>, Error> {
        let tasks: Table = self.lua.named_registry_value(TASKS_KEY)?;
        let mut out: Vec<(String, Option<String>)> = Vec::new();
        for pair in tasks.pairs::<String, Table>() {
            let (name, spec) = pair?;
            out.push((name, spec.get("desc")?));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }
}

pub fn format_error(error: &Error) -> String {
    let style = report::Style::init();
    match error {
        Error::Lua(e) => style.error(&diagnostic::render(e, &style)),
        other => style.error(&other.to_string()),
    }
}
