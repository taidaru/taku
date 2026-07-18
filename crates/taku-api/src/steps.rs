//! The data-step contract.

use std::collections::HashMap;

use mlua::{Function, Lua, Table, Value};

pub const TAG: &str = "__step";

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

#[derive(Clone, Copy)]
pub struct StepDef {
    pub tag: &'static str,
    pub arg: Arg,
    pub run: StepFn,
}

/// Execution context handed to step handlers.
pub struct StepCtx<'a> {
    pub vars: &'a mut HashMap<String, String>,
    pub dotenv: &'a HashMap<String, String>,
    pub formatter: FmtFn,
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
                StepDef {
                    tag: "rm",
                    arg: Arg::Str,
                    run: noop,
                },
                StepDef {
                    tag: "cp",
                    arg: Arg::Table,
                    run: noop,
                },
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
