//! Registration contracts for taku's Lua APIs.

pub mod steps;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Shared registration inputs, constructed once per Lua state by the runtime.
pub struct RegisterCtx {
    /// The parsed project `.env`.
    pub dotenv: Arc<HashMap<String, String>>,
    /// Directory of the Takufile (for path-resolving builtins like `import`).
    pub base: PathBuf,
    /// Whether this state should print load-time warnings (planner state
    /// only; worker states stay quiet).
    pub warnings: bool,
}

#[derive(Clone, Copy)]
pub struct ApiEntry {
    /// Globals `register` installs (for the runtime's sandbox test).
    pub globals: &'static [&'static str],
    pub register: fn(&mlua::Lua, &RegisterCtx) -> mlua::Result<()>,
    /// Data-steps this API executes; the runtime registers their bare-verb
    /// constructors and dispatches on the step tag.
    pub steps: &'static [steps::StepDef],
}

/// Context-prefixed external error — the shared `"<ctx>: <cause>"` shape every
/// API reports failures in.
pub fn ext<E: std::fmt::Display>(ctx: &str, e: E) -> mlua::Error {
    mlua::Error::external(format!("{ctx}: {e}"))
}

/// A structured error an effect can raise so the runtime renders it with a
/// `note:`/`help:` line instead of a bare message. The runtime downcasts it out
/// of the `mlua::Error` and keeps the code frame it recovers from the traceback.
#[derive(Debug, Clone)]
pub struct Diag {
    pub message: String,
    pub note: Option<String>,
    pub help: Option<String>,
    /// Suppress the code frame the runtime would otherwise recover from the
    /// traceback (e.g. an import cycle, where the snippet adds no value).
    pub no_frame: bool,
}

impl Diag {
    pub fn new(message: impl Into<String>) -> Self {
        Diag {
            message: message.into(),
            note: None,
            help: None,
            no_frame: false,
        }
    }
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }
    pub fn no_frame(mut self) -> Self {
        self.no_frame = true;
        self
    }
    /// Wraps this into an `mlua::Error` for a step/effect to return.
    pub fn into_lua(self) -> mlua::Error {
        mlua::Error::external(self)
    }
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Diag {}

std::thread_local! {
    /// Load vs runtime phase. Thread-local because each task body runs in its
    /// own worker thread with its own Lua state.
    static RUNTIME: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The runtime flips this on around a task body; everything else is load phase.
pub fn set_runtime(on: bool) {
    RUNTIME.with(|c| c.set(on));
}

pub fn is_runtime() -> bool {
    RUNTIME.with(|c| c.get())
}

/// Guard for effectful API closures: queries run any time, effects only while
/// a task is executing.
pub fn require_runtime(what: &str) -> mlua::Result<()> {
    if is_runtime() {
        Ok(())
    } else {
        Err(mlua::Error::external(format!(
            "{what}: only available while a task is running (not at load time)"
        )))
    }
}

/// Builds an API table and installs it as the `$global` Lua global: one entry
/// per `method => closure` pair. The closures stay literal at the call site;
/// the macro emits only the mechanical `create_function` + table wiring.
#[macro_export]
macro_rules! lua_api {
    // `$method:tt`, not `ident`: a Lua method may be a Rust keyword (`cmd.try`).
    ($lua:expr, global = $global:literal { $($method:tt => $func:expr),+ $(,)? }) => {{
        let lua = $lua;
        let tbl = lua.create_table()?;
        $( tbl.set(stringify!($method), lua.create_function($func)?)?; )+
        lua.globals().set($global, tbl)
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn lua_api_builds_the_global_and_dispatches() {
        let lua = mlua::Lua::new();
        let greeting = String::from("hello");
        let register = |lua: &mlua::Lua| -> mlua::Result<()> {
            crate::lua_api!(lua, global = "greeter" {
                greet => move |_, name: String| Ok(format!("{greeting} {name}")),
                version => |_, (): ()| Ok(7),
            })
        };
        register(&lua).unwrap();
        lua.load(
            r#"
            assert(greeter.greet("taku") == "hello taku")
            assert(greeter.version() == 7)
        "#,
        )
        .exec()
        .unwrap();
    }
}
