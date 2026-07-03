use std::collections::HashMap;
use std::sync::Arc;

use mlua::{Function, Lua, Value};

use crate::host::Host;

type RemoteRegister = fn(&Lua, Arc<Host>) -> mlua::Result<()>;

/// The APIs `ssh.on` reroutes to the remote host. To make an API remotable:
/// implement its backend trait for `Host` in `remote.rs`, then add a row here.
pub(crate) const REMOTE_APIS: &[(&str, RemoteRegister)] = &[
    ("sh", |lua, h| taku_shell::register(lua, h)),
    ("fs", |lua, h| taku_fs::register(lua, h)),
    ("net", |lua, h| taku_net::register(lua, h)),
    ("env", |lua, h| taku_env::register(lua, h)),
];

pub(crate) fn on(
    lua: &Lua,
    target: Value,
    body: Function,
    dotenv: Arc<HashMap<String, String>>,
) -> mlua::Result<()> {
    let host = Arc::new(Host::from_value(target)?.with_dotenv(dotenv));
    let globals = lua.globals();

    let mut saved = Vec::with_capacity(REMOTE_APIS.len());
    for (name, _) in REMOTE_APIS {
        saved.push((*name, globals.get::<Value>(*name)?));
    }

    let mut result = Ok(());
    for (_, register) in REMOTE_APIS {
        result = register(lua, host.clone());
        if result.is_err() {
            break;
        }
    }
    let result = result.and_then(|()| body.call::<()>(()));

    // Restore the local globals even if the body failed. A restore error here
    // (setting a table entry) is practically impossible and non-fatal — each task
    // rebuilds its Lua state — so we deliberately ignore it rather than mask the
    // body's own error.
    for (name, value) in saved {
        let _ = globals.set(name, value);
    }
    result
}
