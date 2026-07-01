pub mod http;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mlua::Lua;
use ureq::Agent;

const TIMEOUT: Duration = Duration::from_secs(30);

fn ext<E: std::fmt::Display>(ctx: &str, e: E) -> mlua::Error {
    mlua::Error::external(format!("{ctx}: {e}"))
}

pub trait Net: Send + Sync {
    fn tcp_request(&self, host: &str, port: u16, data: &[u8]) -> mlua::Result<Vec<u8>>;
    fn http_get(&self, url: &str) -> mlua::Result<Vec<u8>>;
    fn download(&self, url: &str, path: &str) -> mlua::Result<()>;
}

pub struct Local;

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(http::local_agent)
}

impl Net for Local {
    fn tcp_request(&self, host: &str, port: u16, data: &[u8]) -> mlua::Result<Vec<u8>> {
        let ctx = format!("net.tcp_request({host}:{port})");
        let mut sock = TcpStream::connect((host, port)).map_err(|e| ext(&ctx, e))?;
        let _ = sock.set_read_timeout(Some(TIMEOUT));
        let _ = sock.set_write_timeout(Some(TIMEOUT));
        sock.write_all(data).map_err(|e| ext(&ctx, e))?;
        let mut buf = Vec::new();
        sock.read_to_end(&mut buf).map_err(|e| ext(&ctx, e))?;
        Ok(buf)
    }

    fn http_get(&self, url: &str) -> mlua::Result<Vec<u8>> {
        http::get(agent(), url).map_err(|e| ext(url, e))
    }

    fn download(&self, url: &str, path: &str) -> mlua::Result<()> {
        let body = http::get_large(agent(), url).map_err(|e| ext(url, e))?;
        std::fs::write(path, body).map_err(|e| ext(&format!("net.download -> {path}"), e))
    }
}

pub fn register(lua: &Lua, net: Arc<dyn Net>) -> mlua::Result<()> {
    let tbl = lua.create_table()?;

    let n = net.clone();
    tbl.set(
        "tcp_request",
        lua.create_function(
            move |lua, (host, port, data): (String, u16, mlua::String)| {
                let resp = n.tcp_request(&host, port, &data.as_bytes())?;
                lua.create_string(resp)
            },
        )?,
    )?;

    let n = net.clone();
    tbl.set(
        "http_get",
        lua.create_function(move |lua, url: String| lua.create_string(n.http_get(&url)?))?,
    )?;

    tbl.set(
        "download",
        lua.create_function(move |_, (url, path): (String, String)| net.download(&url, &path))?,
    )?;

    lua.globals().set("net", tbl)?;
    Ok(())
}
