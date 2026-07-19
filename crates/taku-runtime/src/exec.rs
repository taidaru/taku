//! The step executor: walks a task's `steps` table and dispatches each
//! data-step to the handler its API crate declared. Lua builds the plan at
//! load time; this module performs it.

use std::collections::HashMap;
use std::sync::Arc;

use mlua::{Lua, Table, Value};
use taku_api::ApiEntry;
use taku_api::steps::{StepCtx, StepFn, TAG};

use crate::error::Error;
use crate::state::{TASKS_KEY, all_apis};

pub(crate) struct Ctx {
    pub dotenv: Arc<HashMap<String, String>>,
    pub vars: HashMap<String, String>,
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
            let mut msg = format!("task '{name}' has no <{k}> parameter");
            if let Some(close) = crate::taskdef::closest(k, &declared) {
                msg.push_str(&format!(" (did you mean '{close}'?)"));
            }
            return Err(Error::TaskFailed(msg));
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
    all_apis(apis)
        .flat_map(|api| api.steps)
        .find(|def| def.tag == tag)
        .map(|def| def.run)
}

pub(crate) fn run_steps(
    lua: &Lua,
    apis: &'static [ApiEntry],
    spec: &Table,
    ctx: &mut Ctx,
) -> Result<(), Error> {
    let steps: Table = spec.get("steps")?;
    for (i, step) in steps.sequence_values::<Value>().enumerate() {
        run_step(lua, apis, spec, step?, ctx).map_err(|e| match e {
            Error::TaskFailed(msg) => Error::TaskFailed(format!("step {}: {msg}", i + 1)),
            other => other,
        })?;
    }
    Ok(())
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
    };
    run(lua, t, &mut step_ctx).map_err(|e| Error::TaskFailed(e.to_string()))
}

fn run_step(
    lua: &Lua,
    apis: &'static [ApiEntry],
    spec: &Table,
    step: Value,
    ctx: &mut Ctx,
) -> Result<(), Error> {
    match step {
        // "cargo build ${target}" — a bare command template
        Value::String(s) => {
            let t = lua.create_table()?;
            t.set(1, s)?;
            dispatch(lua, apis, "cmd", &t, ctx)
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
            Ok(())
        }
        Value::Table(t) => {
            let tag: Option<String> = t.get(TAG)?;
            match tag.as_deref() {
                // { "cmd ...", cwd = ..., env = {...} }
                None => {
                    if t.get::<Value>(1)?.is_nil() {
                        return Err(Error::TaskFailed(
                            "a table step needs a command string at [1] or a step constructor"
                                .to_string(),
                        ));
                    }
                    dispatch(lua, apis, "cmd", &t, ctx)
                }
                // the one step the executor interprets itself: task recursion
                Some("invoke") => {
                    let name: String = t.get(1)?;
                    run_invoke(lua, apis, &name, &t, ctx)
                }
                Some(tag) => dispatch(lua, apis, tag, &t, ctx),
            }
        }
        other => Err(Error::TaskFailed(format!(
            "a step must be a string, a table, or a function, got {}",
            other.type_name()
        ))),
    }
}

/// `invoke "task"` — run another task's steps here and now.
fn run_invoke(
    lua: &Lua,
    apis: &'static [ApiEntry],
    name: &str,
    t: &Table,
    ctx: &mut Ctx,
) -> Result<(), Error> {
    let tasks: Table = lua.named_registry_value(TASKS_KEY)?;
    let spec: Table = tasks
        .get::<Option<Table>>(name)?
        .ok_or_else(|| Error::TaskFailed(format!("invoke '{name}': no such task")))?;
    let mut sub = Ctx {
        dotenv: ctx.dotenv.clone(),
        vars: initial_vars(&spec)?,
    };
    if let Some(vars) = t.get::<Option<Table>>("vars")? {
        for pair in vars.pairs::<String, String>() {
            let (k, v) = pair?;
            sub.vars.insert(k, v);
        }
    }
    run_steps(lua, apis, &spec, &mut sub)
        .map_err(|e| Error::TaskFailed(format!("invoke '{name}': {e}")))
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
        let msg = err.to_string();
        assert!(msg.contains("no <sah> parameter"), "got: {msg}");
        assert!(msg.contains("did you mean 'sha'?"), "got: {msg}");
    }
}
