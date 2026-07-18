pub mod http;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::OnceLock;
use std::time::Duration;

use taku_api::ext;
use ureq::Agent;

const TIMEOUT: Duration = Duration::from_secs(30);

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(http::local_agent)
}

pub fn tcp_request(host: &str, port: u16, data: &[u8]) -> mlua::Result<Vec<u8>> {
    let ctx = format!("net.tcp_request({host}:{port})");
    let mut sock = TcpStream::connect((host, port)).map_err(|e| ext(&ctx, e))?;
    let _ = sock.set_read_timeout(Some(TIMEOUT));
    let _ = sock.set_write_timeout(Some(TIMEOUT));
    sock.write_all(data).map_err(|e| ext(&ctx, e))?;
    let mut buf = Vec::new();
    sock.read_to_end(&mut buf).map_err(|e| ext(&ctx, e))?;
    Ok(buf)
}

pub fn http_get(url: &str) -> mlua::Result<Vec<u8>> {
    http::get(agent(), url).map_err(|e| ext(&format!("net.http_get({url})"), e))
}

pub fn download(url: &str, path: &str) -> mlua::Result<()> {
    let ctx = format!("net.download({url} -> {path})");
    let mut body = http::get_reader(agent(), url).map_err(|e| ext(&ctx, e))?;
    let mut file = std::fs::File::create(path).map_err(|e| ext(&ctx, e))?;
    std::io::copy(&mut body, &mut file).map_err(|e| ext(&ctx, e))?;
    Ok(())
}

pub fn register(lua: &mlua::Lua) -> mlua::Result<()> {
    taku_api::lua_api!(lua, global = "net" {
        tcp_request => |lua, (host, port, data): (String, u16, mlua::String)| {
            lua.create_string(tcp_request(&host, port, &data.as_bytes())?)
        },
        http_get => |lua, url: String| lua.create_string(http_get(&url)?),
        download => |_, (url, path): (String, String)| download(&url, &path),
    })
}
