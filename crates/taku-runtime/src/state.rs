use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use mlua::{Function, Lua, LuaOptions, StdLib, Table, Value};

use crate::error::Error;

pub(crate) const TAKUFILE: &str = "Takufile.lua";

pub(crate) const TASKS_KEY: &str = "taku.tasks";

pub(crate) fn build_state(path: &Path, source: &str) -> Result<Lua, Error> {
    let lua = new_sandboxed()?;
    let base = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    // A missing/unreadable `.env` yields an empty map; a malformed one is an error.
    let dotenv = Arc::new(match fs::read_to_string(base.join(".env")) {
        Ok(contents) => taku_env::parse_dotenv(&contents)?,
        Err(_) => HashMap::new(),
    });

    register_import(&lua, base)?;
    register_task(&lua)?;

    taku_fs::register(&lua, Arc::new(taku_fs::Local))?;
    taku_net::register(&lua, Arc::new(taku_net::Local))?;
    taku_shell::register(&lua, Arc::new(taku_shell::Local))?;
    taku_ssh::register(&lua, dotenv.clone())?;
    taku_env::register(&lua, Arc::new(taku_env::Local::with_dotenv(dotenv)))?;

    lua.load(source)
        .set_name(format!("@{}", path.to_string_lossy()))
        .exec()?;
    Ok(lua)
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

fn register_import(lua: &Lua, base_dir: PathBuf) -> Result<(), Error> {
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

fn register_task(lua: &Lua) -> Result<(), Error> {
    let tasks = lua.create_table()?;
    lua.set_named_registry_value(TASKS_KEY, &tasks)?;

    let task = lua.create_function(|lua, (name, def): (String, Value)| {
        let spec = lua.create_table()?;

        match def {
            Value::Function(run) => {
                spec.set("run", run)?;
                spec.set("deps", lua.create_table()?)?;
            }
            Value::Table(def) => {
                let run: Option<Function> = def.get("run")?;
                let run = run.ok_or_else(|| {
                    mlua::Error::external(format!(
                        "task('{name}'): spec table has no `run` function"
                    ))
                })?;
                spec.set("run", run)?;
                let deps: Option<Table> = def.get("deps")?;
                spec.set("deps", deps.map_or_else(|| lua.create_table(), Ok)?)?;
                let desc: Option<String> = def.get("desc")?;
                if let Some(desc) = desc {
                    spec.set("desc", desc)?;
                }
            }
            other => {
                return Err(mlua::Error::external(format!(
                    "task('{name}'): second argument must be a function or a spec table, got {}",
                    other.type_name()
                )));
            }
        }

        let tasks: Table = lua.named_registry_value(TASKS_KEY)?;
        tasks.set(name, spec)?;
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
        let src = r#"
            for _, name in ipairs({ "io", "os", "package", "debug", "dofile", "loadfile" }) do
                assert(_G[name] == nil, name .. " should not be reachable in the sandbox")
            end
            for _, name in ipairs({ "fs", "sh", "net", "ssh", "env", "task", "import" }) do
                assert(_G[name] ~= nil, name .. " API should be present")
            end
        "#;
        build_state(Path::new("Takufile.lua"), src).unwrap();
    }

    #[test]
    fn import_runs_each_file_at_most_once() {
        let dir = std::env::temp_dir().join(format!("taku-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("child.lua"), "count = (count or 0) + 1\n").unwrap();
        let main = dir.join("Takufile.lua");
        let src = "import('child.lua')\nimport('child.lua')\nassert(count == 1, 'imported twice')";
        std::fs::write(&main, src).unwrap();

        let result = build_state(&main, src);
        std::fs::remove_dir_all(&dir).unwrap();
        result.unwrap();
    }
}
