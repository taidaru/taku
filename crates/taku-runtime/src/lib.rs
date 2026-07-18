mod diagnostic;
mod dotenv;
mod error;
mod exec;
mod fmtstr;
mod plan;
mod report;
mod schedule;
mod state;
mod taskdef;

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

use mlua::{Lua, Table};
use taku_api::ApiEntry;

pub use error::Error;

use state::{TASKS_KEY, build_state, find_takufile};

pub struct Runtime {
    lua: Lua,
    path: PathBuf,
    source: String,
    apis: &'static [ApiEntry],
}

impl Runtime {
    /// `apis` is the full registry of API crates, assembled by the binary —
    /// the runtime itself depends only on `taku-api`.
    pub fn load(apis: &'static [ApiEntry]) -> Result<Runtime, Error> {
        let path = find_takufile().ok_or(Error::TakufileNotFound)?;
        let source = std::fs::read_to_string(&path).map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("{}: {e}", path.display()),
            ))
        })?;
        let (lua, _dotenv) = build_state(&path, &source, true, apis)?;
        Ok(Runtime {
            lua,
            path,
            source,
            apis,
        })
    }

    pub fn run(&self, command: &str, jobs: Option<NonZeroUsize>) -> Result<(), Error> {
        let plan = plan::build(&self.lua, &self.path, command)?;
        let style = report::Style::init();

        let start = Instant::now();
        let ran = schedule::execute(&style, &self.path, &self.source, &plan, jobs, self.apis)?;
        report::summary(&style, ran, start.elapsed());
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<(String, Option<String>)>, Error> {
        let tasks: Table = self.lua.named_registry_value(TASKS_KEY)?;
        let mut out: Vec<(String, Option<String>)> = Vec::new();
        for pair in tasks.pairs::<String, Table>() {
            let (name, spec) = pair?;
            // the first line of the `---` doc block is the short description
            let doc: Option<String> = spec.get("doc")?;
            let short = doc.map(|d| d.lines().next().unwrap_or_default().to_string());
            out.push((name, short));
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

#[cfg(test)]
pub(crate) fn test_apis() -> &'static [ApiEntry] {
    &[
        taku_fs::API,
        taku_cmd::API,
        taku_net::API,
        taku_env::API,
        taku_ops::API,
    ]
}
