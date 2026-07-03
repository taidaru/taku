use std::process::Output;

pub(crate) use taku_api::ext;

pub(crate) fn shq(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

pub(crate) fn remote_failure(ctx: &str, out: &Output) -> mlua::Error {
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stderr = stderr.trim();
    if stderr.is_empty() {
        mlua::Error::external(format!("{ctx}: remote command failed (exit {code})"))
    } else {
        mlua::Error::external(format!("{ctx}: {stderr} (exit {code})"))
    }
}

pub const ASKPASS_PASSWORD_ENV: &str = "TAKU_ASKPASS_PASSWORD";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quoting_escapes_single_quotes() {
        assert_eq!(shq("plain"), "'plain'");
        assert_eq!(shq("a b"), "'a b'");
        assert_eq!(shq("it's"), "'it'\\''s'");
    }
}
