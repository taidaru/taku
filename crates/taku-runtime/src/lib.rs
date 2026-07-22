mod diagnostic;
mod dotenv;
mod error;
mod exec;
mod fmtstr;
mod incremental;
mod plan;
mod report;
mod schedule;
mod serve;
mod srcmap;
mod state;
mod taskdef;
mod validate;

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

use mlua::{Lua, Table};
use taku_api::ApiEntry;

pub use error::Error;

use state::{TASKS_KEY, build_state, find_takufile};

/// Options for [`Runtime::run`], settable from the CLI.
#[derive(Default)]
pub struct RunOpts<'a> {
    /// Maximum number of tasks to run in parallel.
    pub jobs: Option<NonZeroUsize>,
    /// `--vars KEY=VAL` overrides for the target task's declared params.
    pub vars: &'a [(String, String)],
    /// `--yes`: `confirm` steps answer themselves.
    pub yes: bool,
    /// `--force`: `unchanged` guards rebuild regardless of the stored state.
    pub force: bool,
    /// `--explain`: print why an `unchanged` guard skipped or rebuilt.
    pub explain: bool,
    /// `--dry-run`: print the plan instead of executing it. Command steps
    /// show their unresolved templates so secrets stay out of the output.
    pub dry_run: bool,
    /// `--json`: emit diagnostics and run events as JSON.
    pub json: bool,
    /// `--quiet`: print only error diagnostics — no warnings, info, markers,
    /// summary, or command output.
    pub quiet: bool,
}

pub struct Runtime {
    lua: Lua,
    path: PathBuf,
    source: String,
    apis: &'static [ApiEntry],
}

impl Runtime {
    /// `apis` is the full registry of API crates, assembled by the binary —
    /// the runtime itself depends only on `taku-api`.
    pub fn load(apis: &'static [ApiEntry], json: bool, quiet: bool) -> Result<Runtime, Error> {
        let path = find_takufile().ok_or(Error::TakufileNotFound)?;
        // Operate from the project root (where the Takufile lives) so relative
        // paths, globs, `.env`, and `.taku/` resolve consistently no matter which
        // subdirectory `taku` was invoked from.
        if let Some(dir) = path.parent() {
            std::env::set_current_dir(dir).map_err(|e| {
                Error::Io(std::io::Error::new(
                    e.kind(),
                    format!("{}: {e}", dir.display()),
                ))
            })?;
        }
        let source = std::fs::read_to_string(&path).map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("{}: {e}", path.display()),
            ))
        })?;
        let warnings = if quiet {
            state::Warnings::Off
        } else {
            state::Warnings::On { json }
        };
        let (lua, _dotenv) = build_state(&path, &source, warnings, apis)?;
        Ok(Runtime {
            lua,
            path,
            source,
            apis,
        })
    }

    /// An unknown `--vars` name is rejected with a did-you-mean hint.
    pub fn run(&self, command: &str, opts: &RunOpts) -> Result<(), Error> {
        let plan = plan::build(&self.lua, &self.path, command)?;
        let tasks: Table = self.lua.named_registry_value(TASKS_KEY)?;
        let spec: Table = tasks.get(command)?; // plan::build validated the name
        let overrides = exec::validate_vars(&spec, opts.vars)?;
        if opts.dry_run {
            println!("{}", plan::render(&plan, command));
        }
        let hold = holds_services(&spec)?;
        let style = report::Style::init();

        let start = Instant::now();
        let ran = schedule::execute(
            &style,
            &self.path,
            &self.source,
            &plan,
            self.apis,
            command,
            opts,
            &overrides,
            hold,
        )?;
        // A dry run prints the plan only — no ✓ markers, no run summary.
        if !opts.dry_run {
            report::summary(&style, ran, start.elapsed(), opts.json, opts.quiet);
        } else if !opts.quiet {
            let info = diagnostic::Diagnostic::info("dry run — no commands were executed");
            eprintln!("{}", diagnostic::renderer(opts.json, style).render(&info));
        }
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

/// Keep services running when the target is a service (ends in `serve`) or a
/// bare aggregator (e.g. `dev: build api`). One-shot targets keep the usual
/// serve-as-a-dep behavior: their services stop when they finish. The hold only
/// applies if services are still running.
fn holds_services(spec: &Table) -> Result<bool, Error> {
    let steps: Table = spec.get("steps")?;
    let len = steps.raw_len();
    if len == 0 {
        return Ok(true);
    }
    let last: mlua::Value = steps.raw_get(len)?;
    Ok(matches!(&last, mlua::Value::Table(t)
        if t.get::<Option<String>>(taku_api::steps::TAG).ok().flatten().as_deref()
            == Some("serve")))
}

pub fn format_error(error: &Error, json: bool) -> String {
    diagnostic::renderer(json, report::Style::init()).render(&diagnostic::from_error(error))
}

/// Renders the "unknown subcommand" diagnostic for the CLI (`taku build`),
/// where clap only knows the attempted name.
pub fn render_unknown_command(name: &str, json: bool) -> String {
    diagnostic::renderer(json, report::Style::init()).render(&diagnostic::unknown_command(name))
}

/// Renders the "bad --jobs value" diagnostic for the CLI.
pub fn render_bad_jobs(value: &str, json: bool) -> String {
    diagnostic::renderer(json, report::Style::init()).render(&diagnostic::bad_jobs(value))
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
