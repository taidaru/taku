pub mod http;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

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
        http::get(agent(), url).map_err(|e| ext(&format!("net.http_get({url})"), e))
    }

    fn download(&self, url: &str, path: &str) -> mlua::Result<()> {
        let ctx = format!("net.download({url} -> {path})");
        let mut body = http::get_reader(agent(), url).map_err(|e| ext(&ctx, e))?;
        let mut file = std::fs::File::create(path).map_err(|e| ext(&ctx, e))?;
        std::io::copy(&mut body, &mut file).map_err(|e| ext(&ctx, e))?;
        Ok(())
    }
}

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    global: "net",
    register: |lua, _ctx| register(lua, Arc::new(Local)),
};

taku_api::lua_api! {
    pub fn register(global = "net", backend: Net as n) {
        tcp_request => |lua, (host, port, data): (String, u16, mlua::String)| {
            let resp = n.tcp_request(&host, port, &data.as_bytes())?;
            lua.create_string(resp)
        },
        http_get => |lua, url: String| lua.create_string(n.http_get(&url)?),
        download => |_, (url, path): (String, String)| n.download(&url, &path),
    }
}
