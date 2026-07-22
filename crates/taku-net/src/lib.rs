pub mod http;

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::OnceLock;
use std::time::Duration;

use mlua::Value;
use sha2::{Digest, Sha256};
use taku_api::ext;
use taku_api::steps::{Arg, Field, FieldKind, Positional, StepDef};
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
    // Half-close so a peer that reads until EOF sees the request end and replies.
    let _ = sock.shutdown(Shutdown::Write);
    let mut buf = Vec::new();
    match sock.read_to_end(&mut buf) {
        Ok(_) => Ok(buf),
        // A read timeout after some bytes arrived: return what we received
        // rather than discarding a response the peer did send.
        Err(e)
            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
                && !buf.is_empty() =>
        {
            Ok(buf)
        }
        Err(e) => Err(ext(&ctx, e)),
    }
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
    let digest = match copy_hashed(&mut body, &mut file) {
        Ok(digest) => digest,
        Err(e) => {
            // A partial transfer must not be left looking downloaded
            drop(file);
            let _ = std::fs::remove_file(path);
            return Err(ext(&ctx, e));
        }
    };
    if let Some(expected) = sha256
        && !digest.eq_ignore_ascii_case(expected)
    {
        // A file that failed verification must not be left looking downloaded.
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(
            taku_api::Diag::new(format!("checksum mismatch for '{url}'"))
                .note(format!("expected {expected}, got {digest}"))
                .into_lua(),
        );
    }
    Ok(())
}

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    globals: &["net"],
    register: |lua, _ctx| register(lua),
    steps: &[StepDef {
        tag: "download",
        arg: Arg::Table,
        run: |_, t, ctx| {
            let url = ctx.fmt_field_or_first(t, "url")?;
            let to = ctx.fmt_value(t.get("to")?)?;
            let sha: Option<String> = match t.get::<Value>("sha256")? {
                Value::Nil => None,
                v => Some(ctx.fmt_value(v)?),
            };
            download(&url, &to, sha.as_deref())
        },
        // `url` may be positional or a field; `to` is required.
        fields: &[
            Field {
                name: "url",
                kind: FieldKind::Str,
                required: false,
            },
            Field {
                name: "to",
                kind: FieldKind::Str,
                required: true,
            },
            Field {
                name: "sha256",
                kind: FieldKind::Str,
                required: false,
            },
        ],
        positional: Some(Positional {
            what: "url",
            suggest: "https://...",
            help: "add it as the first element or the 'url' field",
            field: Some("url"),
        }),
    }],
};

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
    use std::io::{self, Read};

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

    struct FailAfter {
        left: usize,
    }
    impl Read for FailAfter {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.left == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "boom"));
            }
            let n = self.left.min(buf.len());
            buf[..n].fill(b'x');
            self.left -= n;
            Ok(n)
        }
    }

    #[test]
    fn download_removes_file_on_midstream_error() {
        let path = std::env::temp_dir().join(format!("taku-dl-{}.tmp", std::process::id()));
        let mut file = std::fs::File::create(&path).unwrap();
        let err = copy_hashed(&mut FailAfter { left: 1024 }, &mut file).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        drop(file);
        let _ = std::fs::remove_file(&path);
        assert!(!path.exists(), "truncated file must not survive");
    }
}
