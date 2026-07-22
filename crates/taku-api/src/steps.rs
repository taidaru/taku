//! The data-step contract.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

use mlua::{Function, Lua, Table, Value};

pub const TAG: &str = "__step";

thread_local! {
    /// The current task's output sink, so module effects (`cmd.run`) reached
    /// from a `function(ctx)` step stream under the same prefix as data-steps.
    /// Thread-local because each task runs on its own worker thread.
    static SINK: RefCell<Option<OutputSink>> = const { RefCell::new(None) };
}

/// Installs the current task's sink for the running worker thread.
pub fn set_sink(sink: Option<OutputSink>) {
    SINK.with(|s| *s.borrow_mut() = sink);
}

/// Runs `f` with the current task's sink, if any.
pub fn with_sink<R>(f: impl FnOnce(Option<&OutputSink>) -> R) -> R {
    SINK.with(|s| f(s.borrow().as_ref()))
}

/// Serialises every prefixed write across all worker threads and both streams,
/// so a stdout line and a stderr line never interleave mid-line at the terminal
/// (where `2>&1` merges the two fds, per-fd locks are not enough).
static OUTPUT_LOCK: Mutex<()> = Mutex::new(());

/// Which of a child's streams a line came from.
#[derive(Clone, Copy)]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Prefixes a task's child output with `<task> │ ` (or emits a JSON `output`
/// event) so parallel tasks stay legible. Owned + `Clone` so a long-lived
/// service can keep its own copy alive across the whole run.
#[derive(Clone)]
pub struct OutputSink {
    pub label: String,
    /// Char width the `│` column aligns to (the longest task name).
    pub width: usize,
    /// SGR colour code for the prefix, or `None` for no colour.
    pub color: Option<u8>,
    pub json: bool,
    /// `--quiet`: drop all child output.
    pub quiet: bool,
}

impl OutputSink {
    /// Env vars that keep a child's own colours alive when its stdout is now a
    /// pipe (cargo, npm, …) — but only when the prefix itself is coloured.
    /// Empty otherwise, so callers apply nothing.
    pub fn color_env(&self) -> &'static [(&'static str, &'static str)] {
        if self.color.is_some() {
            &[("CLICOLOR_FORCE", "1"), ("FORCE_COLOR", "1")]
        } else {
            &[]
        }
    }

    /// Writes one already-split line. The target stream is locked for the whole
    /// write so lines from parallel worker threads never interleave mid-line.
    pub fn line(&self, stream: Stream, content: &str) {
        if self.quiet {
            return;
        }
        // Poisoning is irrelevant — the guard protects no invariant, only write
        // ordering — so recover the guard rather than propagate a panic.
        let _guard = OUTPUT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if self.json {
            let name = match stream {
                Stream::Stdout => "stdout",
                Stream::Stderr => "stderr",
            };
            let mut out = std::io::stdout().lock();
            let _ = writeln!(
                out,
                "{{\"event\":\"output\",\"task\":{},\"stream\":\"{name}\",\"line\":{}}}",
                json_string(&self.label),
                json_string(content),
            );
            let _ = out.flush();
            return;
        }
        let pad = self.width.saturating_sub(self.label.chars().count());
        let prefix = format!("{}{} \u{2502}", self.label, " ".repeat(pad));
        let prefix = match self.color {
            Some(code) => format!("\x1b[{code}m{prefix}\x1b[0m"),
            None => prefix,
        };
        // Flush under the lock so the two fds stay ordered when merged at a tty.
        match stream {
            Stream::Stdout => {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{prefix} {content}");
                let _ = out.flush();
            }
            Stream::Stderr => {
                let mut out = std::io::stderr().lock();
                let _ = writeln!(out, "{prefix} {content}");
                let _ = out.flush();
            }
        }
    }
}

/// Quotes and escapes a string as a JSON string literal.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The Lua shape of a step's bare-verb constructor.
#[derive(Clone, Copy)]
pub enum Arg {
    /// `verb "arg"` → `{ __step = tag, "arg" }`
    Str,
    /// `verb { ... }` → tags and returns the given table
    Table,
    /// No constructor global — the runtime synthesizes the step itself
    /// (e.g. a bare command string dispatches to the `cmd` tag).
    Hidden,
    /// A hand-written constructor for shapes the generic forms can't express.
    Custom(fn(&Lua, &'static str) -> mlua::Result<Function>),
}

pub type StepFn = fn(&Lua, &Table, &mut StepCtx) -> mlua::Result<()>;

pub type FmtFn =
    fn(&str, &HashMap<String, String>, &HashMap<String, String>) -> Result<String, String>;

/// The Lua type a step field accepts, for the runtime's field validator.
#[derive(Clone, Copy, PartialEq)]
pub enum FieldKind {
    Str,
    Num,
    Bool,
    Table,
    /// `outputs = "x"` or `outputs = { ... }`.
    StrOrTable,
}

/// A named `key = value` option a step table accepts.
#[derive(Clone, Copy)]
pub struct Field {
    pub name: &'static str,
    pub kind: FieldKind,
    pub required: bool,
}

/// A step's required first element (`unchanged { "glob", … }`, `mv { "src", … }`),
/// described for the "missing …" diagnostic and its suggested edit. It may also
/// be given as a named field (`mv { src = "…" }`) via `field`, so it counts as
/// present either way.
#[derive(Clone, Copy)]
pub struct Positional {
    /// Noun for the message, e.g. `glob pattern` → "missing glob pattern".
    pub what: &'static str,
    /// Placeholder inserted by the fix-it edit, e.g. `src/**/*`.
    pub suggest: &'static str,
    pub help: &'static str,
    /// A named field that also satisfies this element (`src`, `data`, `url`, …).
    pub field: Option<&'static str>,
}

#[derive(Clone, Copy)]
pub struct StepDef {
    pub tag: &'static str,
    pub arg: Arg,
    pub run: StepFn,
    /// Allowed `key = value` fields — the runtime rejects any others and checks
    /// types. Empty means the step takes no options (no field validation).
    pub fields: &'static [Field],
    /// A required first positional element, if any.
    pub positional: Option<Positional>,
}

impl StepDef {
    /// Convenience for a step with no options and no required positional (no
    /// field validation runs).
    pub const fn simple(tag: &'static str, arg: Arg, run: StepFn) -> Self {
        StepDef {
            tag,
            arg,
            run,
            fields: &[],
            positional: None,
        }
    }
}

/// Execution context handed to step handlers.
pub struct StepCtx<'a> {
    pub vars: &'a mut HashMap<String, String>,
    pub dotenv: &'a HashMap<String, String>,
    pub formatter: FmtFn,
    /// `--yes`: interactive confirmations answer themselves.
    pub yes: bool,
    /// Where a command step streams its child's output line-by-line, or `None`
    /// to inherit the terminal (tests, `capture`).
    pub output: Option<&'a OutputSink>,
}

impl StepCtx<'_> {
    pub fn fmt(&self, template: &str) -> mlua::Result<String> {
        (self.formatter)(template, self.vars, self.dotenv).map_err(mlua::Error::external)
    }

    pub fn fmt_value(&self, value: Value) -> mlua::Result<String> {
        match value {
            Value::String(s) => self.fmt(&s.to_string_lossy()),
            Value::Table(t) => {
                let raw: Option<mlua::String> = t.get("__raw")?;
                match raw {
                    Some(s) => Ok(s.to_string_lossy().to_string()),
                    None => Err(mlua::Error::external("expected a string or raw\"...\"")),
                }
            }
            other => Err(mlua::Error::external(format!(
                "expected a string or raw\"...\", got {}",
                other.type_name()
            ))),
        }
    }

    /// `t[key]`, falling back to `t[1]` — for `write { data, to = ... }`-style
    /// tables where the payload may be positional.
    pub fn fmt_field_or_first(&self, t: &Table, key: &str) -> mlua::Result<String> {
        let v: Value = match t.get::<Value>(key)? {
            Value::Nil => t.get(1)?,
            v => v,
        };
        self.fmt_value(v)
    }
}

pub fn register_constructors(lua: &Lua, defs: &[StepDef]) -> mlua::Result<()> {
    for def in defs {
        let tag = def.tag;
        let ctor = match def.arg {
            Arg::Hidden => continue,
            Arg::Str => lua.create_function(move |lua, arg: Value| {
                let t = lua.create_table()?;
                t.set(TAG, tag)?;
                t.set(1, arg)?;
                Ok(t)
            })?,
            Arg::Table => lua.create_function(move |_, t: Table| {
                t.set(TAG, tag)?;
                Ok(t)
            })?,
            Arg::Custom(build) => build(lua, tag)?,
        };
        lua.globals().set(tag, ctor)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noop(_: &Lua, _: &Table, _: &mut StepCtx) -> mlua::Result<()> {
        Ok(())
    }

    #[test]
    fn constructors_build_tagged_tables_without_effects() {
        let lua = Lua::new();
        register_constructors(
            &lua,
            &[
                StepDef::simple("rm", Arg::Str, noop),
                StepDef::simple("cp", Arg::Table, noop),
            ],
        )
        .unwrap();
        lua.load(
            r#"
            local s = rm "some/path"
            assert(s.__step == "rm" and s[1] == "some/path")
            local c = cp { "a", to = "b" }
            assert(c.__step == "cp" and c[1] == "a" and c.to == "b")
        "#,
        )
        .exec()
        .unwrap();
    }
}
