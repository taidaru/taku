use std::path::{Path, PathBuf};

use crate::report::Style;

struct Locus {
    path: PathBuf,
    line: usize,
    msg_start: usize,
}

pub(crate) fn render(err: &mlua::Error, style: &Style) -> String {
    let full = err.to_string();
    let Some(locus) = find_locus(&full) else {
        return full;
    };
    let Ok(source) = std::fs::read_to_string(&locus.path) else {
        return full;
    };
    let Some(src_line) = source.lines().nth(locus.line - 1) else {
        return full;
    };

    let is_syntax = matches!(innermost(err), mlua::Error::SyntaxError { .. });
    let label = if is_syntax {
        "syntax error"
    } else {
        "runtime error"
    };

    let (message, traceback) = split_traceback(&full[locus.msg_start..]);
    let message = message.trim();

    let (col, width) = caret_span(src_line, is_syntax.then(|| near_token(&full)).flatten());

    let mut out = String::new();
    out.push_str(label);
    out.push('\n');

    let display = display_path(&locus.path);
    let gutter = " ".repeat(locus.line.to_string().len());
    let bar = style.cyan("|");

    out.push_str(&format!(
        "{gutter}{arrow} {display}:{line}:{col}\n",
        arrow = style.cyan("-->"),
        line = locus.line,
    ));
    out.push_str(&format!("{gutter} {bar}\n"));
    out.push_str(&format!(
        "{num} {bar} {src_line}\n",
        num = style.dim(&locus.line.to_string()),
    ));
    let carets = style.red(&"^".repeat(width.max(1)));
    let pad = " ".repeat(col.saturating_sub(1));
    if message.is_empty() {
        out.push_str(&format!("{gutter} {bar} {pad}{carets}\n"));
    } else {
        out.push_str(&format!("{gutter} {bar} {pad}{carets} {message}\n"));
    }

    if let Some(frames) = traceback {
        out.push_str(&format!("{gutter} {bar}\n"));
        out.push_str("stack traceback:\n");
        for frame in frames {
            out.push_str(&format!("   {}\n", style.dim(frame)));
        }
    }

    out.pop();
    out
}

fn innermost(err: &mlua::Error) -> &mlua::Error {
    match err {
        mlua::Error::CallbackError { cause, .. } => innermost(cause),
        mlua::Error::WithContext { cause, .. } => innermost(cause),
        other => other,
    }
}

fn find_locus(msg: &str) -> Option<Locus> {
    let mut search = 0;
    while let Some(rel) = msg[search..].find(':') {
        let colon = search + rel;
        let after = colon + 1;
        let digits = msg[after..].bytes().take_while(u8::is_ascii_digit).count();
        let digits_end = after + digits;
        if digits > 0 && msg.as_bytes().get(digits_end) == Some(&b':') {
            let line_start = msg[..colon].rfind('\n').map_or(0, |n| n + 1);
            if let Some(path) = resolve_path(&msg[line_start..colon])
                && let Ok(line) = msg[after..digits_end].parse::<usize>()
                && line > 0
            {
                return Some(Locus {
                    path,
                    line,
                    msg_start: digits_end + 1,
                });
            }
        }
        search = colon + 1;
    }
    None
}

fn resolve_path(prefix: &str) -> Option<PathBuf> {
    let trimmed = prefix.trim();
    let mut candidates = vec![trimmed];
    for pre in ["runtime error: ", "syntax error: "] {
        if let Some(rest) = trimmed.strip_prefix(pre) {
            candidates.push(rest.trim());
        }
    }
    if let Some(tok) = trimmed.rsplit(char::is_whitespace).next() {
        candidates.push(tok);
    }
    candidates
        .into_iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .map(Path::to_path_buf)
}

fn split_traceback(text: &str) -> (&str, Option<Vec<&str>>) {
    let Some(idx) = text.find("stack traceback:") else {
        return (text, None);
    };
    let head = &text[..idx];
    let tail = &text["stack traceback:".len() + idx..];
    let frames: Vec<&str> = tail
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "[C]: in ?")
        .collect();
    let frames = (!frames.is_empty()).then_some(frames);
    (head, frames)
}

fn near_token(msg: &str) -> Option<&str> {
    let start = msg.find("near '")? + "near '".len();
    let end = msg[start..].find('\'')? + start;
    Some(&msg[start..end])
}

fn caret_span(src_line: &str, token: Option<&str>) -> (usize, usize) {
    if let Some(tok) = token.filter(|t| !t.is_empty())
        && let Some(byte) = src_line.find(tok)
    {
        let col = src_line[..byte].chars().count() + 1;
        return (col, tok.chars().count());
    }
    let indent = src_line.chars().take_while(|c| c.is_whitespace()).count();
    (indent + 1, 1)
}

fn display_path(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> Style {
        Style::for_test(false)
    }

    fn write_temp(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("taku-diag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn runtime_locus_renders_snippet_and_caret() {
        let path = write_temp("rt.lua", "task('x', function()\n    boom()\nend)\n");
        let msg = format!(
            "{}:2: attempt to call a nil value (global 'boom')",
            path.display()
        );
        let err = mlua::Error::RuntimeError(msg);
        let out = render(&err, &plain());

        assert!(out.starts_with("runtime error\n"), "{out}");
        assert!(
            out.contains(&format!("--> {}:2:5", path.display())) || out.contains("-->"),
            "{out}"
        );
        assert!(out.contains("2 | "), "gutter+line missing:\n{out}");
        assert!(out.contains("boom()"), "source line missing:\n{out}");
        assert!(out.contains('^'), "caret missing:\n{out}");
        assert!(out.contains("attempt to call a nil value"), "{out}");
    }

    #[test]
    fn syntax_near_token_positions_the_caret() {
        let path = write_temp("syn.lua", "local s = \"unterminated\n");
        let msg = format!(
            "{}:1: unfinished string near '\"unterminated'",
            path.display()
        );
        let err = mlua::Error::SyntaxError {
            message: msg,
            incomplete_input: false,
        };
        let out = render(&err, &plain());

        assert!(out.starts_with("syntax error\n"), "{out}");
        let caret_line = out.lines().find(|l| l.contains('^')).unwrap();
        let caret_col = caret_line.find('^').unwrap();
        let bar_col = caret_line.find('|').unwrap();
        assert!(
            caret_col > bar_col + 10,
            "caret not aligned to token:\n{out}"
        );
    }

    #[test]
    fn missing_locus_falls_back_to_raw_message() {
        let err = mlua::Error::RuntimeError("something broke with no location".into());
        assert_eq!(render(&err, &plain()), err.to_string());
    }

    #[test]
    fn plain_style_emits_no_escapes() {
        let path = write_temp("noesc.lua", "task('x', function()\n    boom()\nend)\n");
        let msg = format!("{}:2: boom", path.display());
        let err = mlua::Error::RuntimeError(msg);
        assert!(!render(&err, &plain()).contains('\x1b'));
    }
}
