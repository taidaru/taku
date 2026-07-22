//! The step executor: walks a task's `steps` table and dispatches each
//! data-step to the handler its API crate declared. Lua builds the plan at
//! load time; this module performs it.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use mlua::{Lua, Table, Value};
use taku_api::ApiEntry;
use taku_api::steps::{StepCtx, StepFn, TAG};

use crate::error::Error;
use crate::state::{TASKS_KEY, all_apis};

/// The run-scoped set of tasks that have completed by any path (scheduled or
/// `invoke`d) — a later dep on one of them counts as satisfied.
pub(crate) type Done = Arc<Mutex<HashSet<String>>>;

pub(crate) struct Ctx {
    pub dotenv: Arc<HashMap<String, String>>,
    pub vars: HashMap<String, String>,
    /// Directory of the Takufile — the project root (`.taku/`, relative globs).
    pub base: std::path::PathBuf,
    pub yes: bool,
    pub force: bool,
    pub explain: bool,
    pub dry_run: bool,
    pub done: Done,
    pub services: crate::serve::Services,
    /// State file to write after the task succeeds, set by an `unchanged`
    /// guard that decided to run.
    pub pending_state: Option<(std::path::PathBuf, [u8; 32])>,
    /// Task names currently on the `invoke` call chain, for cycle detection —
    /// the planner only sees header deps, not `invoke` steps.
    pub invoke_stack: Vec<String>,
    /// Where command steps and services stream their child output, if the run
    /// prefixes output (a real scheduled run) rather than inheriting (tests).
    pub output: Option<taku_api::steps::OutputSink>,
    /// `--json`: service/info lines this task emits are JSON objects.
    pub json: bool,
    /// `--quiet`: suppress warnings/info this task would emit.
    pub quiet: bool,
}

impl Ctx {
    pub fn format(&self, template: &str) -> Result<String, Error> {
        format_step(template, &self.vars, &self.dotenv).map_err(Error::TaskFailed)
    }
}

/// A step's verdict on the rest of the task.
pub(crate) enum Flow {
    Continue,
    Skip,
    /// Stop the task early, but count it as run (a non-last `serve`, which ends
    /// the task after starting its service).
    Stop,
}

/// Whether a task executed its steps or was short-circuited by an `unchanged`
/// guard. Drives the `✓`/`- skipped` marker the scheduler prints.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Outcome {
    Ran,
    Skipped,
}

/// Registry key holding the live `ctx.vars` table while a function-step runs,
/// so the `fmt()` builtin sees the current task vars.
pub(crate) const VARS_KEY: &str = "taku.vars";

pub(crate) fn table_to_vars(lua: &Lua, t: &Table) -> mlua::Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    for pair in t.pairs::<String, Value>() {
        let (k, v) = pair?;
        let s = lua
            .coerce_string(v)?
            .ok_or_else(|| mlua::Error::external(format!("ctx.vars.{k} is not a string")))?;
        vars.insert(k, s.to_str()?.to_string());
    }
    Ok(vars)
}

/// `--vars KEY=VAL` may only name parameters declared in the task header.
pub(crate) fn validate_vars(
    spec: &Table,
    vars: &[(String, String)],
) -> Result<HashMap<String, String>, Error> {
    let mut declared = Vec::new();
    if let Some(params) = spec.get::<Option<Table>>("params")? {
        for p in params.sequence_values::<Table>() {
            declared.push(p?.get::<String>("name")?);
        }
    }
    let mut out = HashMap::new();
    for (k, v) in vars {
        if !declared.iter().any(|d| d == k) {
            let name: String = spec.get("name")?;
            let mut diag = crate::diagnostic::Diagnostic::error(format!(
                "task '{name}' has no parameter '{k}'"
            ));
            if let Some(close) = crate::taskdef::closest(k, &declared) {
                diag = diag.help(format!("did you mean '{close}'?"));
            }
            return Err(Error::Task(Box::new(diag)));
        }
        out.insert(k.clone(), v.clone());
    }
    Ok(out)
}

/// The placeholder formatter handed to step handlers via `StepCtx`:
/// `${name}` from vars, `$NAME`/`${$NAME}` from the process env with a `.env`
/// fallback, `$$` literal.
pub(crate) fn format_step(
    template: &str,
    vars: &HashMap<String, String>,
    dotenv: &HashMap<String, String>,
) -> Result<String, String> {
    crate::fmtstr::format(template, vars, &|name| {
        std::env::var(name)
            .ok()
            .or_else(|| dotenv.get(name).cloned())
    })
}

pub(crate) fn initial_vars(spec: &Table) -> mlua::Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    if let Some(params) = spec.get::<Option<Table>>("params")? {
        for p in params.sequence_values::<Table>() {
            let p = p?;
            let name: String = p.get("name")?;
            if let Some(default) = p.get::<Option<String>>("default")? {
                vars.insert(name, default);
            }
        }
    }
    Ok(vars)
}

fn handler(apis: &'static [ApiEntry], tag: &str) -> Option<StepFn> {
    step_def(apis, tag).map(|def| def.run)
}

/// The `StepDef` registered for `tag`, across the runtime builtins and API crates.
fn step_def(apis: &'static [ApiEntry], tag: &str) -> Option<&'static taku_api::steps::StepDef> {
    all_apis(apis)
        .flat_map(|api| api.steps)
        .find(|def| def.tag == tag)
}

pub(crate) fn run_steps(
    lua: &Lua,
    apis: &'static [ApiEntry],
    spec: &Table,
    ctx: &mut Ctx,
) -> Result<Outcome, Error> {
    let steps: Table = spec.get("steps")?;
    let len = steps.raw_len();
    for (i, step) in steps.sequence_values::<Value>().enumerate() {
        let last = i + 1 == len;
        let flow = match run_step(lua, apis, spec, step?, ctx, last, i) {
            Ok(flow) => flow,
            Err(e) => {
                let vars = ctx.vars.clone();
                return Err(framed_step_error(lua, spec, i, &vars, e));
            }
        };
        match flow {
            Flow::Skip => return Ok(Outcome::Skipped),
            Flow::Stop => break,
            Flow::Continue => {}
        }
    }
    if let Some((path, state)) = ctx.pending_state.take() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, state)?;
    }
    Ok(Outcome::Ran)
}

/// Shapes a step failure into a task diagnostic, attaching a code frame from the
/// source map when the error didn't already carry one (data-steps have no Lua
/// traceback, so their frame comes from the scan).
fn framed_step_error(
    lua: &Lua,
    spec: &Table,
    index: usize,
    vars: &HashMap<String, String>,
    e: Error,
) -> Error {
    // `invoke "typo"` gets the task-not-found diagnostic with a did-you-mean edit.
    if let Some(bad) = invoke_unknown(&e) {
        return invoke_unknown_diag(lua, spec, index, bad);
    }
    let mut diag = crate::diagnostic::from_error(&e);
    let site = step_site(lua, spec, index);
    if diag.frames.is_empty()
        && let Some(site) = &site
    {
        diag = diag.frame(site.frame());
    }
    if diag.message.starts_with("stray '$' in template") {
        diag = diag.help("escape the literal dollar sign");
    }
    // A `$NAME` env ref in a template that isn't set — same hint as env.require.
    let env_help = diag
        .helps
        .is_empty()
        .then(|| env_required(&diag.message))
        .flatten()
        .map(|name| format!("'export {name}=...' or 'echo \"{name}=...\" >> .env'"));
    if let Some(help) = env_help {
        diag = diag.help(help);
    }
    // `${typo}` in a template: suggest the nearest declared var + an edit.
    if let Some(bad) = undefined_var(&diag.message) {
        let names: Vec<String> = vars.keys().cloned().collect();
        if let Some(close) = crate::taskdef::closest(&bad, &names) {
            let msg = format!("did you mean '{close}'?");
            match &site {
                Some(site) => {
                    let edit = crate::diagnostic::Edit {
                        line: site.line,
                        before: site.text.clone(),
                        after: site.text.replacen(
                            &format!("${{{bad}}}"),
                            &format!("${{{close}}}"),
                            1,
                        ),
                    };
                    diag = diag.help_edit(msg, edit);
                }
                None => diag = diag.help(msg),
            }
        }
    }
    Error::Task(Box::new(diag))
}

/// The name in an `environment variable '<name>' is required but not set` message.
fn env_required(message: &str) -> Option<&str> {
    message
        .strip_prefix("environment variable '")?
        .strip_suffix("' is required but not set")
}

/// The name in an `undefined variable '${name}' in template` message.
fn undefined_var(message: &str) -> Option<String> {
    message
        .strip_prefix("undefined variable '${")?
        .strip_suffix("}' in template")
        .map(str::to_string)
}

/// The name in an `invoke '<name>': no such task` error.
fn invoke_unknown(e: &Error) -> Option<String> {
    let Error::TaskFailed(msg) = e else {
        return None;
    };
    msg.strip_prefix("invoke '")?
        .strip_suffix("': no such task")
        .map(str::to_string)
}

fn invoke_unknown_diag(lua: &Lua, spec: &Table, index: usize, bad: String) -> Error {
    let mut diag = crate::diagnostic::Diagnostic::error(format!("task '{bad}' does not exist"));
    let site = step_site(lua, spec, index);
    if let Some(site) = &site {
        diag = diag.frame(site.frame());
    }
    let names = task_names(lua);
    if let Some(close) = crate::taskdef::closest(&bad, &names) {
        match &site {
            Some(site) => {
                let edit = crate::diagnostic::Edit {
                    line: site.line,
                    before: site.text.clone(),
                    after: site.text.replacen(&bad, close, 1),
                };
                diag = diag.help_edit(format!("did you mean '{close}'?"), edit);
            }
            None => diag = diag.help(format!("did you mean '{close}'?")),
        }
    }
    Error::Task(Box::new(diag))
}

fn task_names(lua: &Lua) -> Vec<String> {
    lua.named_registry_value::<Table>(TASKS_KEY)
        .map(|t| {
            t.pairs::<String, Table>()
                .filter_map(Result::ok)
                .map(|(k, _)| k)
                .collect()
        })
        .unwrap_or_default()
}

/// The source-map site for the step at `index` of `spec`.
fn step_site(lua: &Lua, spec: &Table, index: usize) -> Option<crate::srcmap::Site> {
    let name: String = spec.get("name").ok()?;
    let sources = lua.app_data_ref::<crate::srcmap::Sources>()?;
    Some(sources.map.get(&name)?.steps.get(index)?.site.clone())
}

/// Warns that the step after a non-last `serve` is unreachable.
fn warn_step_after_serve(lua: &Lua, spec: &Table, serve_index: usize, json: bool) {
    let Some(after) = step_site(lua, spec, serve_index + 1) else {
        return;
    };
    let mut diag = crate::diagnostic::Diagnostic::warning("step after 'serve' will never run")
        .frame(after.frame());
    if let Some(serve) = step_site(lua, spec, serve_index) {
        diag = diag.note(format!(
            "'serve' at {}:{} ends the task — later steps are unreachable",
            serve.file, serve.line
        ));
    }
    eprintln!(
        "{}",
        crate::diagnostic::renderer(json, crate::report::Style::init()).render(&diag)
    );
}

/// The full source-map site (step + fields + closing brace) for step `index`.
fn step_full_site(lua: &Lua, spec: &Table, index: usize) -> Option<crate::srcmap::StepSite> {
    let name: String = spec.get("name").ok()?;
    let sources = lua.app_data_ref::<crate::srcmap::Sources>()?;
    Some(sources.map.get(&name)?.steps.get(index)?.clone())
}

fn dispatch(
    lua: &Lua,
    apis: &'static [ApiEntry],
    tag: &str,
    t: &Table,
    ctx: &mut Ctx,
) -> Result<(), Error> {
    let run =
        handler(apis, tag).ok_or_else(|| Error::TaskFailed(format!("unknown step tag '{tag}'")))?;
    let mut step_ctx = StepCtx {
        vars: &mut ctx.vars,
        dotenv: &ctx.dotenv,
        formatter: format_step,
        yes: ctx.yes,
        output: ctx.output.as_ref(),
    };
    // Keep the mlua error itself (not its string) so any structured `Diag`
    // payload — note/help — survives into the diagnostic.
    run(lua, t, &mut step_ctx).map_err(Error::Lua)
}

#[allow(clippy::too_many_arguments)]
fn run_step(
    lua: &Lua,
    apis: &'static [ApiEntry],
    spec: &Table,
    step: Value,
    ctx: &mut Ctx,
    last: bool,
    index: usize,
) -> Result<Flow, Error> {
    if ctx.dry_run {
        return dry_step(lua, apis, spec, step, ctx);
    }
    let cont = |r: Result<(), Error>| r.map(|()| Flow::Continue);
    match step {
        // "cargo build ${target}" — a bare command template
        Value::String(s) => {
            let t = lua.create_table()?;
            t.set(1, s)?;
            cont(dispatch(lua, apis, "cmd", &t, ctx))
        }
        // the imperative escape hatch: fn(ctx) with a live vars table
        Value::Function(f) => {
            let vars_t = lua.create_table()?;
            for (k, v) in &ctx.vars {
                vars_t.set(k.as_str(), v.as_str())?;
            }
            // published for the fmt() builtin, which reads the live table
            lua.set_named_registry_value(VARS_KEY, &vars_t)?;
            let lua_ctx = lua.create_table()?;
            lua_ctx.set("vars", &vars_t)?;
            lua_ctx.set("task", spec)?;
            f.call::<()>(&lua_ctx)?;
            ctx.vars = table_to_vars(lua, &vars_t)?;
            Ok(Flow::Continue)
        }
        Value::Table(t) => {
            let tag: Option<String> = t.get(TAG)?;
            // Schema check (unknown/missing/wrong-type fields) before running,
            // using the schema the step's `StepDef` declares.
            let site = step_full_site(lua, spec, index);
            if let Some(def) = step_def(apis, tag.as_deref().unwrap_or("cmd")) {
                crate::validate::check(&t, def.fields, def.positional.as_ref(), site.as_ref())?;
            }
            match tag.as_deref() {
                // { "cmd ...", cwd = ..., env = {...} }
                None => {
                    if t.get::<Value>(1)?.is_nil() {
                        return Err(Error::TaskFailed(
                            "a table step needs a command string at [1] or a step constructor"
                                .to_string(),
                        ));
                    }
                    cont(dispatch(lua, apis, "cmd", &t, ctx))
                }
                // steps the executor interprets itself: task recursion and
                // the incrementality guard
                Some("invoke") => {
                    let name: String = t.get(1)?;
                    cont(run_invoke(lua, apis, &name, &t, ctx))
                }
                Some("unchanged") => match crate::incremental::check(spec, &t, ctx)? {
                    crate::incremental::Decision::Skip => Ok(Flow::Skip),
                    crate::incremental::Decision::Run => Ok(Flow::Continue),
                },
                Some("serve") => {
                    // `serve` ends the task; a later step is unreachable — warn,
                    // start the service, and skip the rest.
                    if !last {
                        if !ctx.quiet {
                            warn_step_after_serve(lua, spec, index, ctx.json);
                        }
                        crate::serve::run(spec, &t, ctx)?;
                        return Ok(Flow::Stop);
                    }
                    cont(crate::serve::run(spec, &t, ctx))
                }
                Some(tag) => cont(dispatch(lua, apis, tag, &t, ctx)),
            }
        }
        other => Err(Error::TaskFailed(format!(
            "a step must be a string, a table, or a function, got {}",
            other.type_name()
        ))),
    }
}

fn dry_step(
    lua: &Lua,
    apis: &'static [ApiEntry],
    spec: &Table,
    step: Value,
    ctx: &mut Ctx,
) -> Result<Flow, Error> {
    match step {
        Value::String(s) => println!("  {}", s.to_string_lossy()),
        Value::Function(f) => {
            let info = f.info();
            let src = info.short_src.as_deref().unwrap_or("?").to_string();
            let line = info
                .line_defined
                .map_or_else(|| "?".to_string(), |l| l.to_string());
            println!("  <lua {src}:{line}>");
        }
        Value::Table(t) => {
            let tag: Option<String> = t.get(TAG)?;
            match tag.as_deref() {
                None => println!("  {}", dry_table("cmd", &t)?),
                Some("invoke") => {
                    let name: String = t.get(1)?;
                    println!("  invoke \"{name}\"");
                    run_invoke(lua, apis, &name, &t, ctx)?;
                }
                Some("unchanged") => {
                    let decision = crate::incremental::check(spec, &t, ctx)?;
                    ctx.pending_state = None; // a dry run must not write state
                    if matches!(decision, crate::incremental::Decision::Skip) {
                        println!("  unchanged: the remaining steps would be skipped");
                        return Ok(Flow::Skip);
                    }
                    println!("  unchanged: the remaining steps would run");
                }
                Some(tag) => println!("  {}", dry_table(tag, &t)?),
            }
        }
        other => {
            return Err(Error::TaskFailed(format!(
                "a step must be a string, a table, or a function, got {}",
                other.type_name()
            )));
        }
    }
    Ok(Flow::Continue)
}

/// `tag "positional" key=value ...`, named keys sorted, templates unresolved.
fn dry_table(tag: &str, t: &Table) -> Result<String, Error> {
    let mut out = tag.to_string();
    for v in t.sequence_values::<Value>() {
        out.push(' ');
        crate::incremental::write_value(&mut out, &v?, false);
    }
    let mut named: Vec<(String, Value)> = Vec::new();
    for pair in t.pairs::<Value, Value>() {
        let (k, v) = pair?;
        if let Value::String(s) = &k {
            let key = s.to_string_lossy().to_string();
            if key != TAG {
                named.push((key, v));
            }
        }
    }
    named.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in named {
        out.push_str(&format!(" {k}="));
        crate::incremental::write_value(&mut out, &v, false);
    }
    Ok(out)
}

/// `invoke "task"` — run another task's steps here and now.
fn run_invoke(
    lua: &Lua,
    apis: &'static [ApiEntry],
    name: &str,
    t: &Table,
    ctx: &mut Ctx,
) -> Result<(), Error> {
    // Cycle detection: the planner walks header deps only, never `invoke`
    // steps, so a self/mutual invoke would recurse until the stack overflows.
    if ctx.invoke_stack.iter().any(|n| n == name) {
        let mut chain = ctx.invoke_stack.clone();
        chain.push(name.to_string());
        return Err(Error::TaskFailed(format!(
            "invoke cycle: {}",
            chain.join(" -> ")
        )));
    }

    let tasks: Table = lua.named_registry_value(TASKS_KEY)?;
    let spec: Table = tasks
        .get::<Option<Table>>(name)?
        .ok_or_else(|| Error::TaskFailed(format!("invoke '{name}': no such task")))?;

    // Real runs claim the task up front so an invoke of an already-run (or
    // concurrently-scheduled) task is a no-op, not a second execution. A dry
    // run only prints, so it never touches the shared set.
    if !ctx.dry_run && !ctx.done.lock().unwrap().insert(name.to_string()) {
        return Ok(());
    }

    let mut invoke_stack = ctx.invoke_stack.clone();
    invoke_stack.push(name.to_string());
    let mut sub = Ctx {
        dotenv: ctx.dotenv.clone(),
        vars: initial_vars(&spec)?,
        base: ctx.base.clone(),
        yes: ctx.yes,
        force: ctx.force,
        explain: ctx.explain,
        dry_run: ctx.dry_run,
        done: ctx.done.clone(),
        services: ctx.services.clone(),
        pending_state: None,
        invoke_stack,
        // an invoked task's steps stream under the invoking task's prefix
        output: ctx.output.clone(),
        json: ctx.json,
        quiet: ctx.quiet,
    };
    // Validated like `--vars` so an invoke with an unknown/missing param
    // errors instead of silently passing through.
    if let Some(vars) = t.get::<Option<Table>>("vars")? {
        let mut pairs = Vec::new();
        for pair in vars.pairs::<String, String>() {
            let (k, v) = pair?;
            pairs.push((k, ctx.format(&v)?));
        }
        sub.vars.extend(validate_vars(&spec, &pairs)?);
    }
    match run_steps(lua, apis, &spec, &mut sub) {
        Ok(_) => Ok(()),
        // A task diagnostic already frames the failing inner step and carries
        // its notes/help — keep it instead of flattening to a string.
        Err(e @ Error::Task(_)) => Err(e),
        Err(e) => Err(Error::TaskFailed(format!("invoke '{name}': {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vars_must_name_declared_params_with_a_hint() {
        let lua = Lua::new();
        let spec = lua.create_table().unwrap();
        spec.set("name", "build").unwrap();
        let params = lua.create_table().unwrap();
        let p = lua.create_table().unwrap();
        p.set("name", "sha").unwrap();
        params.set(1, p).unwrap();
        spec.set("params", params).unwrap();

        let ok = validate_vars(&spec, &[("sha".into(), "abc".into())]).unwrap();
        assert_eq!(ok["sha"], "abc");

        let err = validate_vars(&spec, &[("sah".into(), "abc".into())]).unwrap_err();
        let Error::Task(diag) = err else {
            panic!("expected a task diagnostic, got: {err}");
        };
        assert_eq!(diag.message, "task 'build' has no parameter 'sah'");
        assert!(
            diag.helps
                .iter()
                .any(|h| h.message == "did you mean 'sha'?"),
            "missing hint: {diag:?}"
        );
    }
}
