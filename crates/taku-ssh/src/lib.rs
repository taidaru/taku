mod exec;
mod host;
mod remote;
mod session;
mod tunnel;
mod util;

use std::collections::HashMap;
use std::sync::Arc;

use mlua::{Function, Lua, Value};

use host::Host;
use taku_shell::{Opts, Shell, parse_argv};

pub use util::ASKPASS_PASSWORD_ENV;

pub fn register(lua: &Lua, dotenv: Arc<HashMap<String, String>>) -> mlua::Result<()> {
    let ssh = lua.create_table()?;

    ssh.set(
        "run",
        lua.create_function(|_, (target, cmd): (Value, Value)| {
            Host::from_value(target)?.run(&parse_argv(cmd)?, &Opts::default())
        })?,
    )?;

    ssh.set(
        "capture",
        lua.create_function(|lua, (target, cmd): (Value, Value)| {
            let out = Host::from_value(target)?.capture(&parse_argv(cmd)?, &Opts::default())?;
            taku_shell::capture_table(lua, out)
        })?,
    )?;

    ssh.set(
        "on",
        lua.create_function(move |lua, (target, body): (Value, Function)| {
            session::on(lua, target, body, dotenv.clone())
        })?,
    )?;

    lua.globals().set("ssh", ssh)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::Lua;

    fn lua_with_stubs() -> Lua {
        let lua = Lua::new();
        for name in ["sh", "fs", "net"] {
            let t = lua.create_table().unwrap();
            t.set("marker", lua.create_function(|_, ()| Ok("local")).unwrap())
                .unwrap();
            lua.globals().set(name, t).unwrap();
        }
        super::register(&lua, std::sync::Arc::new(std::collections::HashMap::new())).unwrap();
        lua
    }

    #[test]
    fn ssh_on_reroutes_sh_fs_net_and_restores_them() {
        let lua = lua_with_stubs();
        lua.load(
            r#"
            local orig = { sh = sh, fs = fs, net = net }
            local swapped = true
            ssh.on("user@host", function()
                swapped = (sh ~= orig.sh) and (fs ~= orig.fs) and (net ~= orig.net)
            end)
            assert(swapped, "sh/fs/net were not rerouted inside ssh.on")
            assert(sh == orig.sh and fs == orig.fs and net == orig.net,
                   "sh/fs/net were not restored after ssh.on")
        "#,
        )
        .exec()
        .unwrap();
    }

    #[test]
    fn ssh_on_restores_globals_even_when_block_errors() {
        let lua = lua_with_stubs();
        lua.load(
            r#"
            local orig = { sh = sh, fs = fs, net = net }
            local ok = pcall(function()
                ssh.on("user@host", function() error("boom") end)
            end)
            assert(not ok, "error inside the block should propagate")
            assert(sh == orig.sh and fs == orig.fs and net == orig.net,
                   "globals must be restored after a failing block")
        "#,
        )
        .exec()
        .unwrap();
    }
}
