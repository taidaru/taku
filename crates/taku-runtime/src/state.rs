use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use mlua::{Lua, LuaOptions, StdLib, Table, Value};
use taku_api::steps::{Arg, StepDef, TAG};
use taku_api::{ApiEntry, RegisterCtx};

use crate::error::Error;

pub(crate) const TAKUFILE: &str = "Takufile.lua";

pub(crate) const TASKS_KEY: &str = "taku.tasks";
pub(crate) const DOCS_KEY: &str = "taku.docs";

/// The runtime's own builtins, declared in the same [`ApiEntry`] shape as
/// every other API crate. `invoke`/`serve`/`unchanged` are task-level steps
/// the executor interprets itself, so their handlers are stubs.
pub(crate) const RUNTIME_API: ApiEntry = ApiEntry {
    globals: &["task", "import", "fmt", "raw"],
    register: register_builtins,
    steps: &[
        StepDef {
            tag: "invoke",
            arg: Arg::Custom(|lua, tag| {
                lua.create_function(move |lua, (name, vars): (String, Option<Table>)| {
                    let t = lua.create_table()?;
                    t.set(TAG, tag)?;
                    t.set(1, name)?;
                    if let Some(vars) = vars {
                        t.set("vars", vars)?;
                    }
                    Ok(t)
                })
            }),
            run: |_, _, _| Err(mlua::Error::external("invoke is handled by the runtime")),
        },
        StepDef {
            tag: "serve",
            // both `serve "cmd"` and `serve { "cmd", ready = ... }` are valid
            arg: Arg::Custom(|lua, tag| {
                lua.create_function(move |lua, v: Value| match v {
                    Value::Table(t) => {
                        t.set(TAG, tag)?;
                        Ok(t)
                    }
                    other => {
                        let t = lua.create_table()?;
                        t.set(TAG, tag)?;
                        t.set(1, other)?;
                        Ok(t)
                    }
                })
            }),
            run: |_, _, _| Err(mlua::Error::external("serve is handled by the runtime")),
        },
        StepDef {
            tag: "unchanged",
            arg: Arg::Table,
            run: |_, _, _| Err(mlua::Error::external("unchanged is handled by the runtime")),
        },
    ],
};

pub(crate) fn all_apis(
    apis: &'static [ApiEntry],
) -> impl Iterator<Item = &'static ApiEntry> + Clone {
    std::iter::once(&RUNTIME_API).chain(apis.iter())
}

/// Builds the canonical Lua state. `warnings` is set only for the state the
/// planner loads: workers rebuild the same state per task, and re-printing
/// every warning once per task would be noise. The parsed `.env` map is
/// returned alongside for the step executor.
pub(crate) fn build_state(
    path: &Path,
    source: &str,
    warnings: bool,
    apis: &'static [ApiEntry],
) -> Result<(Lua, Arc<HashMap<String, String>>), Error> {
    let lua = new_sandboxed()?;
    let base = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    // A missing `.env` yields an empty map; an unreadable or malformed one is
    // an error — silently running without the intended variables is worse.
    let dotenv = Arc::new(match fs::read_to_string(base.join(".env")) {
        Ok(contents) => crate::dotenv::parse(&contents)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
        Err(e) => {
            return Err(Error::Io(std::io::Error::new(
                e.kind(),
                format!("{}: {e}", base.join(".env").display()),
            )));
        }
    });

    let ctx = RegisterCtx {
        dotenv: dotenv.clone(),
        base,
        warnings,
    };
    for api in all_apis(apis) {
        (api.register)(&lua, &ctx)?;
        taku_api::steps::register_constructors(&lua, api.steps)?;
    }

    store_docs(&lua, source)?;
    lua.load(source)
        .set_name(format!("@{}", path.to_string_lossy()))
        .exec()?;
    Ok((lua, dotenv))
}

/// `task`, `import`, and the formatter builtins `fmt`/`raw`.
fn register_builtins(lua: &Lua, ctx: &RegisterCtx) -> mlua::Result<()> {
    register_task(lua, ctx.warnings)?;
    register_import(lua, ctx.base.clone())?;

    // raw("..."): a value the step executor passes through without formatting.
    let raw = lua.create_function(|lua, s: mlua::String| {
        let t = lua.create_table()?;
        t.set("__raw", s)?;
        Ok(t)
    })?;
    lua.globals().set("raw", raw)?;

    // fmt("..."): manual formatting inside function-steps. Task vars come
    // from the live ctx.vars table the executor publishes in the registry.
    let dotenv = ctx.dotenv.clone();
    let fmt = lua.create_function(move |lua, template: String| {
        let vars = match lua.named_registry_value::<Table>(crate::exec::VARS_KEY) {
            Ok(t) => crate::exec::table_to_vars(lua, &t)?,
            Err(_) => HashMap::new(),
        };
        crate::exec::format_step(&template, &vars, &dotenv)
            .map_err(|e| mlua::Error::external(format!("fmt: {e}")))
    })?;
    lua.globals().set("fmt", fmt)?;
    Ok(())
}

/// Scans `source` for `---` doc blocks and merges them into the docs registry
/// table, so `task()` can attach a doc to its spec when it runs.
fn store_docs(lua: &Lua, source: &str) -> mlua::Result<()> {
    let docs: Table = match lua.named_registry_value(DOCS_KEY) {
        Ok(t) => t,
        Err(_) => {
            let t = lua.create_table()?;
            lua.set_named_registry_value(DOCS_KEY, &t)?;
            t
        }
    };
    let mut found = HashMap::new();
    crate::taskdef::scan_docs(source, &mut found);
    for (name, doc) in found {
        docs.set(name, doc)?;
    }
    Ok(())
}

fn new_sandboxed() -> Result<Lua, Error> {
    let libs = StdLib::COROUTINE | StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
    let lua = Lua::new_with(libs, LuaOptions::default())?;

    let globals = lua.globals();
    for name in ["dofile", "loadfile"] {
        globals.set(name, Value::Nil)?;
    }

    Ok(lua)
}

struct ImportState {
    dir_stack: Vec<PathBuf>,
    imported: HashSet<PathBuf>,
}

fn register_import(lua: &Lua, base_dir: PathBuf) -> mlua::Result<()> {
    let state = Rc::new(RefCell::new(ImportState {
        dir_stack: vec![base_dir],
        imported: HashSet::new(),
    }));

    let import = lua.create_function(move |lua, rel: String| {
        let current = state
            .borrow()
            .dir_stack
            .last()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("."));
        let target = current.join(&rel);
        let canonical = fs::canonicalize(&target)
            .map_err(|e| mlua::Error::external(format!("import('{rel}'): {e}")))?;

        if !state.borrow_mut().imported.insert(canonical.clone()) {
            return Ok(());
        }

        let source = fs::read_to_string(&canonical)
            .map_err(|e| mlua::Error::external(format!("import('{rel}'): {e}")))?;
        let dir = canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| current.clone());

        // docs first, so `task()` calls in the imported file can find theirs
        store_docs(lua, &source)?;
        state.borrow_mut().dir_stack.push(dir);
        let result = lua
            .load(&source)
            .set_name(format!("@{}", canonical.to_string_lossy()))
            .exec();
        state.borrow_mut().dir_stack.pop();
        result
    })?;
    lua.globals().set("import", import)?;
    Ok(())
}

/// `task("name <param=default>: dep1 dep2", { steps... })` — the second
/// argument is always a table of steps
fn register_task(lua: &Lua, warnings: bool) -> mlua::Result<()> {
    let tasks = lua.create_table()?;
    lua.set_named_registry_value(TASKS_KEY, &tasks)?;

    let task = lua.create_function(move |lua, (header, steps): (String, Table)| {
        let parsed = crate::taskdef::parse_header(&header).map_err(mlua::Error::external)?;

        let spec = lua.create_table()?;
        spec.set("name", parsed.name.as_str())?;
        spec.set("steps", steps)?;
        spec.set("deps", lua.create_sequence_from(parsed.deps)?)?;
        let params = lua.create_table()?;
        for (i, p) in parsed.params.iter().enumerate() {
            let entry = lua.create_table()?;
            entry.set("name", p.name.as_str())?;
            entry.set("default", p.default.as_deref())?;
            params.set(i + 1, entry)?;
        }
        spec.set("params", params)?;

        let docs: Table = lua.named_registry_value(DOCS_KEY)?;
        if let Some(doc) = docs.get::<Option<String>>(parsed.name.as_str())? {
            spec.set("doc", doc)?;
        }

        let tasks: Table = lua.named_registry_value(TASKS_KEY)?;
        if warnings && tasks.contains_key(parsed.name.as_str())? {
            eprintln!(
                "taku: warning: task '{}' is defined more than once; the last definition wins",
                parsed.name
            );
        }
        tasks.set(parsed.name.as_str(), spec)?;
        Ok(())
    })?;
    lua.globals().set("task", task)?;

    Ok(())
}

pub(crate) fn find_takufile() -> Option<PathBuf> {
    let candidate = env::current_dir().ok()?.join(TAKUFILE);
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_hides_dangerous_libs_but_keeps_the_apis() {
        let (lua, _env) =
            build_state(Path::new("Takufile.lua"), "", true, crate::test_apis()).unwrap();
        let globals = lua.globals();
        for name in ["io", "os", "package", "debug", "dofile", "loadfile"] {
            let value: Value = globals.get(name).unwrap();
            assert!(
                value.is_nil(),
                "{name} should not be reachable in the sandbox"
            );
        }
        let expected = all_apis(crate::test_apis()).flat_map(|api| {
            api.globals.iter().copied().chain(
                api.steps
                    .iter()
                    .filter(|def| !matches!(def.arg, Arg::Hidden))
                    .map(|def| def.tag),
            )
        });
        for name in expected {
            let value: Value = globals.get(name).unwrap();
            assert!(!value.is_nil(), "{name} API should be present");
        }
    }

    #[test]
    fn import_runs_each_file_at_most_once() {
        let dir = std::env::temp_dir().join(format!("taku-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("child.lua"), "count = (count or 0) + 1\n").unwrap();
        let main = dir.join("Takufile.lua");
        let src = "import('child.lua')\nimport('child.lua')\nassert(count == 1, 'imported twice')";
        std::fs::write(&main, src).unwrap();

        let result = build_state(&main, src, true, crate::test_apis());
        std::fs::remove_dir_all(&dir).unwrap();
        result.unwrap();
    }
}
