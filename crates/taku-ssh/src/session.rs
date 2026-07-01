use std::collections::HashMap;
use std::sync::Arc;

use mlua::{Function, Lua, Value};

use crate::host::Host;

pub(crate) fn on(
    lua: &Lua,
    target: Value,
    body: Function,
    dotenv: Arc<HashMap<String, String>>,
) -> mlua::Result<()> {
    let host = Arc::new(Host::from_value(target)?.with_dotenv(dotenv));
    let globals = lua.globals();

    let saved: [(&str, Value); 4] = [
        ("sh", globals.get("sh")?),
        ("fs", globals.get("fs")?),
        ("net", globals.get("net")?),
        ("env", globals.get("env")?),
    ];

    taku_shell::register(lua, host.clone())?;
    taku_fs::register(lua, host.clone())?;
    taku_net::register(lua, host.clone())?;
    taku_env::register(lua, host)?;

    let result = body.call::<()>(());

    // Restore the local globals even if the body failed. A restore error here
    // (setting a table entry) is practically impossible and non-fatal — each task
    // rebuilds its Lua state — so we deliberately ignore it rather than mask the
    // body's own error.
    for (name, value) in saved {
        let _ = globals.set(name, value);
    }
    result
}
