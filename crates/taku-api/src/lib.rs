//! Shared plumbing for taku's Lua API crates.

/// Context-prefixed external error — the shared `"<ctx>: <cause>"` shape every
/// API reports failures in.
pub fn ext<E: std::fmt::Display>(ctx: &str, e: E) -> mlua::Error {
    mlua::Error::external(format!("{ctx}: {e}"))
}

/// Builds an API table and installs it as the `$global` Lua global: one entry
/// per `method => closure` pair. The closures stay literal at the call site;
/// the macro emits only the mechanical `create_function` + table wiring.
#[macro_export]
macro_rules! lua_api {
    ($lua:expr, global = $global:literal { $($method:ident => $func:expr),+ $(,)? }) => {{
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
