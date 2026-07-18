pub mod http;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::OnceLock;
use std::time::Duration;

use sha2::{Digest, Sha256};
use taku_api::ext;
use ureq::Agent;

const TIMEOUT: Duration = Duration::from_secs(30);

fn agent() -> &'static Agent {
    static AGENT: OnceLock<Agent> = OnceLock::new();
    AGENT.get_or_init(http::local_agent)
}

pub fn tcp(host: &str, port: u16, data: &[u8]) -> mlua::Result<Vec<u8>> {
    let ctx = format!("net.tcp({host}:{port})");
    let mut sock = TcpStream::connect((host, port)).map_err(|e| ext(&ctx, e))?;
    let _ = sock.set_read_timeout(Some(TIMEOUT));
    let _ = sock.set_write_timeout(Some(TIMEOUT));
    sock.write_all(data).map_err(|e| ext(&ctx, e))?;
    let mut buf = Vec::new();
    sock.read_to_end(&mut buf).map_err(|e| ext(&ctx, e))?;
    Ok(buf)
}

pub fn get(url: &str) -> mlua::Result<Vec<u8>> {
    http::get(agent(), url).map_err(|e| ext(&format!("net.get({url})"), e))
}

/// Streams `reader` into `writer`, returning the hex SHA-256 of everything
/// copied.
fn copy_hashed(reader: &mut impl Read, writer: &mut impl Write) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        writer.write_all(&buf[..n])?;
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub fn download(url: &str, path: &str, sha256: Option<&str>) -> mlua::Result<()> {
    let ctx = format!("net.download({url} -> {path})");
    let mut body = http::get_reader(agent(), url).map_err(|e| ext(&ctx, e))?;
    let mut file = std::fs::File::create(path).map_err(|e| ext(&ctx, e))?;
    let digest = copy_hashed(&mut body, &mut file).map_err(|e| ext(&ctx, e))?;
    if let Some(expected) = sha256
        && !digest.eq_ignore_ascii_case(expected)
    {
        // A file that failed verification must not be left looking downloaded.
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(ext(
            &ctx,
            format!("sha256 mismatch: expected {expected}, got {digest}"),
        ));
    }
    Ok(())
}

pub fn register(lua: &mlua::Lua) -> mlua::Result<()> {
    taku_api::lua_api!(lua, global = "net" {
        tcp => |lua, (host, port, data): (String, u16, mlua::String)| {
            taku_api::require_runtime("net.tcp")?;
            lua.create_string(tcp(&host, port, &data.as_bytes())?)
        },
        get => |lua, url: String| {
            taku_api::require_runtime("net.get")?;
            lua.create_string(get(&url)?)
        },
        download => |_, (url, path, sha256): (String, String, Option<String>)| {
            taku_api::require_runtime("net.download")?;
            download(&url, &path, sha256.as_deref())
        },
    })
}

#[cfg(test)]
mod tests {
    use super::copy_hashed;

    #[test]
    fn copy_hashed_matches_known_digest() {
        let mut out = Vec::new();
        let digest = copy_hashed(&mut "abc".as_bytes(), &mut out).unwrap();
        assert_eq!(out, b"abc");
        // sha256("abc"), the FIPS 180-2 test vector.
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
