use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use mlua::{Lua, LuaOptions, StdLib, Table, Value};
use taku_api::steps::{Arg, Field, FieldKind, Positional, StepDef, TAG};
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
            // `invoke "t"`, `invoke("t", {p = v})`, and the curried sugar
            // `invoke "t" { p = v }` — the last one calls the step table, so
            // it carries a __call that attaches the vars.
            arg: Arg::Custom(|lua, tag| {
                lua.create_function(move |lua, (name, vars): (String, Option<Table>)| {
                    let t = lua.create_table()?;
                    t.set(TAG, tag)?;
                    t.set(1, name)?;
                    match vars {
                        Some(vars) => t.set("vars", vars)?,
                        None => {
                            let call = lua.create_function(|_, (this, vars): (Table, Table)| {
                                this.set("vars", vars)?;
                                Ok(this)
                            })?;
                            set_call(lua, &t, call)?;
                        }
                    }
                    Ok(t)
                })
            }),
            run: |_, _, _| Err(mlua::Error::external("invoke is handled by the runtime")),
            fields: &[],
            positional: None,
        },
        StepDef {
            tag: "serve",
            // `serve "cmd"`, `serve { "cmd", ready = ... }`, and the curried
            // sugar `serve "cmd" { ready = ... }` (merges the options in).
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
                        let call = lua.create_function(|_, (this, opts): (Table, Table)| {
                            for pair in opts.pairs::<Value, Value>() {
                                let (k, v) = pair?;
                                this.set(k, v)?;
                            }
                            Ok(this)
                        })?;
                        set_call(lua, &t, call)?;
                        Ok(t)
                    }
                })
            }),
            run: |_, _, _| Err(mlua::Error::external("serve is handled by the runtime")),
            // The command / api / web / options shape is validated by serve itself.
            fields: &[],
            positional: None,
        },
        StepDef {
            tag: "unchanged",
            arg: Arg::Table,
            run: |_, _, _| Err(mlua::Error::external("unchanged is handled by the runtime")),
            fields: &[Field {
                name: "outputs",
                kind: FieldKind::StrOrTable,
                required: false,
            }],
            positional: Some(Positional {
                what: "glob pattern",
                suggest: "src/**/*",
                help: "add a glob pattern as the first element",
                field: None,
            }),
        },
    ],
};

/// Makes a step table callable (`__call`) so the curried sugar
/// `verb "arg" { ... }` works: Lua calls the constructor's result with the
/// trailing table, and the handler folds it into the step.
fn set_call(lua: &Lua, t: &Table, call: mlua::Function) -> mlua::Result<()> {
    let mt = lua.create_table()?;
    mt.set("__call", call)?;
    t.set_metatable(Some(mt))?;
    Ok(())
}

pub(crate) fn all_apis(
    apis: &'static [ApiEntry],
) -> impl Iterator<Item = &'static ApiEntry> + Clone {
    std::iter::once(&RUNTIME_API).chain(apis.iter())
}

/// Whether a state should print load-time warnings. Only the planner's state
/// does (`On`); workers rebuild the same state per task and would just repeat
/// them. `json` selects the renderer.
#[derive(Clone, Copy)]
pub(crate) enum Warnings {
    On { json: bool },
    Off,
}

/// Builds the canonical Lua state. The parsed `.env` map is returned alongside
/// for the step executor.
pub(crate) fn build_state(
    path: &Path,
    source: &str,
    warnings: Warnings,
    apis: &'static [ApiEntry],
) -> Result<(Lua, Arc<HashMap<String, String>>), Error> {
    let lua = new_sandboxed()?;
    // Source-position map, filled here for the main file and by `import` for
    // each imported file, then read by the executor to frame step errors.
    let mut sources = crate::srcmap::Sources::default();
    sources.add(&crate::srcmap::display(path), source);
    lua.set_app_data(sources);
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
        warnings: matches!(warnings, Warnings::On { .. }),
    };
    for api in all_apis(apis) {
        (api.register)(&lua, &ctx)?;
        taku_api::steps::register_constructors(&lua, api.steps)?;
    }

    store_docs(&lua, source)?;
    lua.load(source)
        .set_name(format!("@{}", path.to_string_lossy()))
        .exec()
        .map_err(|e| enrich_undefined_global(&lua, e))?;
    if let Warnings::On { json } = warnings {
        emit_load_warnings(&lua, json);
    }
    Ok((lua, dotenv))
}

/// A load-time nil-call to an undefined global (a typo'd step constructor) gets
/// a did-you-mean hint + edit, using the registered globals as the candidate set.
fn enrich_undefined_global(lua: &Lua, e: mlua::Error) -> Error {
    let diag = crate::diagnostic::from_lua(&e);
    let bad = diag
        .message
        .strip_prefix("undefined global '")
        .and_then(|r| r.strip_suffix('\''));
    if let Some(bad) = bad {
        let names: Vec<String> = lua
            .globals()
            .pairs::<String, Value>()
            .filter_map(Result::ok)
            .map(|(k, _)| k)
            .collect();
        if let Some(close) = crate::taskdef::closest(bad, &names)
            && let Some(frame) = diag.frames.first()
        {
            let edit = crate::diagnostic::Edit {
                line: frame.line,
                before: frame.source.clone(),
                after: frame.source.replacen(bad, close, 1),
            };
            let msg = format!("did you mean '{close}'?");
            return Error::Task(Box::new(diag.help_edit(msg, edit)));
        }
    }
    Error::Lua(e)
}

/// `task`, `import`, and the formatter builtins `fmt`/`raw`.
fn register_builtins(lua: &Lua, ctx: &RegisterCtx) -> mlua::Result<()> {
    register_task(lua)?;
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
    /// The files currently mid-import, to detect `a → b → a` cycles (the
    /// `imported` dedup alone would silently no-op the re-entry).
    chain: Vec<PathBuf>,
}

fn register_import(lua: &Lua, base_dir: PathBuf) -> mlua::Result<()> {
    let state = Rc::new(RefCell::new(ImportState {
        dir_stack: vec![base_dir],
        imported: HashSet::new(),
        chain: Vec::new(),
    }));

    let import = lua.create_function(move |lua, rel: String| {
        let current = state
            .borrow()
            .dir_stack
            .last()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("."));
        let target = current.join(&rel);
        let canonical = fs::canonicalize(&target).map_err(|e| {
            let note = if e.kind() == std::io::ErrorKind::NotFound {
                "file does not exist".to_string()
            } else {
                e.to_string()
            };
            taku_api::Diag::new(format!("cannot import '{rel}'"))
                .note(note)
                .into_lua()
        })?;
        // `.lua` text only — even through a symlink, the resolved target must
        // be a Lua file; nothing else is loadable as a Takufile fragment.
        if canonical.extension().and_then(|e| e.to_str()) != Some("lua") {
            return Err(mlua::Error::external(format!(
                "import('{rel}'): only .lua files can be imported"
            )));
        }

        // A file re-entered while still loading is an import cycle.
        if state.borrow().chain.contains(&canonical) {
            let mut path: Vec<String> = state
                .borrow()
                .chain
                .iter()
                .map(|p| crate::srcmap::display(p))
                .collect();
            path.push(crate::srcmap::display(&canonical));
            return Err(taku_api::Diag::new("import cycle detected")
                .note(path.join(" -> "))
                .no_frame()
                .into_lua());
        }
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
        if let Some(mut sources) = lua.app_data_mut::<crate::srcmap::Sources>() {
            sources.add(&crate::srcmap::display(&canonical), &source);
        }
        state.borrow_mut().dir_stack.push(dir);
        state.borrow_mut().chain.push(canonical.clone());
        let result = lua
            .load(&source)
            .set_name(format!("@{}", canonical.to_string_lossy()))
            .exec();
        state.borrow_mut().chain.pop();
        state.borrow_mut().dir_stack.pop();
        result
    })?;
    lua.globals().set("import", import)?;
    Ok(())
}

/// `task "name <param=default>: dep1 dep2" { steps... }` or `task(header, steps)`
fn register_task(lua: &Lua) -> mlua::Result<()> {
    let tasks = lua.create_table()?;
    lua.set_named_registry_value(TASKS_KEY, &tasks)?;

    let task = lua.create_function(
        move |lua, (header, steps): (String, Option<Table>)| match steps {
            Some(steps) => register_spec(lua, &header, steps).map(|()| Value::Nil),
            None => {
                let f = lua
                    .create_function(move |lua, steps: Table| register_spec(lua, &header, steps))?;
                Ok(Value::Function(f))
            }
        },
    )?;
    lua.globals().set("task", task)?;

    Ok(())
}

fn register_spec(lua: &Lua, header: &str, steps: Table) -> mlua::Result<()> {
    let parsed = crate::taskdef::parse_header(header).map_err(mlua::Error::external)?;

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
    tasks.set(parsed.name.as_str(), spec)?;
    Ok(())
}

/// Load-time warnings the planner prints once (redefinitions, unused params).
/// Run on the main thread, so they render with the default style.
fn emit_load_warnings(lua: &Lua, json: bool) {
    use crate::diagnostic::Diagnostic;
    use std::collections::{HashMap, HashSet};

    let Some(sources) = lua.app_data_ref::<crate::srcmap::Sources>() else {
        return;
    };
    let style = crate::report::Style::init();
    let show = |d: &Diagnostic| {
        eprintln!("{}", crate::diagnostic::renderer(json, style).render(d));
    };

    // A task defined more than once: primary frame + "also defined here" notes.
    let mut by_name: HashMap<&str, Vec<&crate::srcmap::TaskSite>> = HashMap::new();
    for (name, site) in &sources.all {
        by_name.entry(name).or_default().push(site);
    }
    let mut reported = HashSet::new();
    for (name, _) in &sources.all {
        if !reported.insert(name.as_str()) {
            continue;
        }
        let sites = &by_name[name.as_str()];
        if sites.len() > 1 {
            let mut diag =
                Diagnostic::warning(format!("task '{name}' is defined {} times", sites.len()))
                    .frame(sites[0].def.frame());
            for site in &sites[1..] {
                diag = diag.note_frame("also defined here", site.def.frame());
            }
            show(&diag.help("only the last definition wins — remove or rename the others"));
        }
    }

    // A declared parameter referenced nowhere in the task body — neither as a
    // `${name}` template nor by name inside a `function(ctx)` step
    // (`ctx.vars.name`, `ctx.vars["name"]`, …). A whole-word check keeps
    // `profiles` from counting as a use of `profile`.
    for site in sources.map.values() {
        for (param, psite) in &site.params {
            if !references_param(&site.body, param) {
                show(
                    &Diagnostic::warning(format!("parameter '{param}' is never used"))
                        .frame(psite.frame())
                        .help(format!(
                            "remove the parameter or reference it as '${{{param}}}' in a step"
                        )),
                );
            }
        }
    }

    // A second `unchanged` guard in a task shares the one per-task cache with the
    // first, and the first guard's skip stops the task before the second runs —
    // only one guard per task is meaningful.
    if let Ok(tasks) = lua.named_registry_value::<Table>(TASKS_KEY) {
        for (name, spec) in tasks.pairs::<String, Table>().flatten() {
            let Ok(steps) = spec.get::<Table>("steps") else {
                continue;
            };
            let guards: Vec<usize> = steps
                .sequence_values::<Value>()
                .enumerate()
                .filter(|(_, step)| {
                    matches!(step, Ok(Value::Table(t))
                        if t.get::<Option<String>>(TAG).ok().flatten().as_deref() == Some("unchanged"))
                })
                .map(|(i, _)| i)
                .collect();
            if guards.len() >= 2 {
                let mut diag = Diagnostic::warning(format!(
                    "task '{name}' has {} 'unchanged' guards",
                    guards.len()
                ));
                if let Some(step) = sources
                    .map
                    .get(&name)
                    .and_then(|ts| ts.steps.get(guards[1]))
                {
                    diag = diag.frame(step.site.frame());
                }
                show(&diag.help("use a single 'unchanged' guard per task — they share one cache"));
            }
        }
    }
}

/// Whether a task body mentions `param` — as a `${param}` template placeholder
/// or as a whole-word identifier (a `function(ctx)` step reading `ctx.vars`).
/// The word-boundary check keeps `profiles` from counting as a use of `profile`.
fn references_param(body: &str, param: &str) -> bool {
    let is_boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric() && c != '_');
    body.match_indices(param).any(|(i, _)| {
        is_boundary(body[..i].chars().next_back())
            && is_boundary(body[i + param.len()..].chars().next())
    })
}

/// Looks for a `Takufile.lua` in the current directory, then walks up through
/// parent directories — so `taku` can run from anywhere inside a project, like
/// `git` or `cargo`. Relative paths then resolve against the file's directory.
pub(crate) fn find_takufile() -> Option<PathBuf> {
    let mut dir = env::current_dir().ok()?;
    loop {
        let candidate = dir.join(TAKUFILE);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_param_matches_templates_and_vars_access() {
        // template use
        assert!(references_param(r#"{ "build ${profile}" }"#, "profile"));
        // function-step access via ctx.vars — must not be flagged unused
        assert!(references_param(
            r#"{ function(ctx) print(ctx.vars.profile) end }"#,
            "profile"
        ));
        assert!(references_param(
            r#"{ function(ctx) return ctx.vars["profile"] end }"#,
            "profile"
        ));
        // genuinely unused
        assert!(!references_param(r#"{ "echo hi" }"#, "profile"));
        // whole-word only: `profiles` is not a use of `profile`
        assert!(!references_param(r#"{ "${profiles}" }"#, "profile"));
    }

    #[test]
    fn sandbox_hides_dangerous_libs_but_keeps_the_apis() {
        let (lua, _env) = build_state(
            Path::new("Takufile.lua"),
            "",
            Warnings::On { json: false },
            crate::test_apis(),
        )
        .unwrap();
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
    fn curried_task_form_registers_with_docs() {
        let src = "--- says hi\ntask \"hi <name=world>\" {\n    echo \"Hello, ${name}!\",\n}\n";
        let (lua, _env) = build_state(
            Path::new("Takufile.lua"),
            src,
            Warnings::On { json: false },
            crate::test_apis(),
        )
        .unwrap();
        let tasks: Table = lua.named_registry_value(TASKS_KEY).unwrap();
        let spec: Table = tasks.get("hi").unwrap();
        assert_eq!(spec.get::<String>("doc").unwrap(), "says hi");
        assert_eq!(spec.get::<Table>("steps").unwrap().raw_len(), 1);
    }

    #[test]
    fn curried_sugar_works_for_invoke_and_serve() {
        let src = r#"
task "hello <name=world>" {}
task "build" { invoke "hello" { name = "alice" } }
task "svc" { serve "sleep 5" { ready = { timeout = 0.1 } } }
"#;
        let (lua, _env) = build_state(
            Path::new("Takufile.lua"),
            src,
            Warnings::On { json: false },
            crate::test_apis(),
        )
        .unwrap();
        let tasks: Table = lua.named_registry_value(TASKS_KEY).unwrap();

        let step: Table = tasks
            .get::<Table>("build")
            .and_then(|t| t.get::<Table>("steps"))
            .and_then(|s| s.get(1))
            .unwrap();
        assert_eq!(step.get::<String>(TAG).unwrap(), "invoke");
        assert_eq!(
            step.get::<Table>("vars")
                .unwrap()
                .get::<String>("name")
                .unwrap(),
            "alice"
        );

        let step: Table = tasks
            .get::<Table>("svc")
            .and_then(|t| t.get::<Table>("steps"))
            .and_then(|s| s.get(1))
            .unwrap();
        assert_eq!(step.get::<String>(TAG).unwrap(), "serve");
        assert_eq!(step.get::<String>(1).unwrap(), "sleep 5");
        assert!(step.get::<Table>("ready").is_ok());
    }

    #[test]
    fn import_runs_each_file_at_most_once() {
        let dir = std::env::temp_dir().join(format!("taku-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("child.lua"), "count = (count or 0) + 1\n").unwrap();
        let main = dir.join("Takufile.lua");
        let src = "import('child.lua')\nimport('child.lua')\nassert(count == 1, 'imported twice')";
        std::fs::write(&main, src).unwrap();

        let result = build_state(&main, src, Warnings::On { json: false }, crate::test_apis());
        std::fs::remove_dir_all(&dir).unwrap();
        result.unwrap();
    }
}
