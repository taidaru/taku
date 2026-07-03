use std::path::{Path, PathBuf};

use crate::report::Style;

/// Source lines of context shown above and below the offending line.
const CONTEXT: usize = 2;

struct Locus {
    path: PathBuf,
    line: usize,
    msg_start: usize,
}

pub(crate) fn render(err: &mlua::Error, style: &Style) -> String {
    let full = err.to_string();
    let (head, traceback) = split_traceback(&full);

    let is_syntax = matches!(innermost(err), mlua::Error::SyntaxError { .. });
    let label = if is_syntax {
        "syntax error"
    } else {
        "runtime error"
    };

    let (locus, message) = match find_locus(head) {
        Some(locus) => {
            let message = head[locus.msg_start..].trim().to_string();
            (Some(locus), message)
        }
        None => (
            locus_from_traceback(traceback.as_deref()),
            clean_message(head),
        ),
    };

    let window = locus.as_ref().and_then(|l| {
        let source = std::fs::read_to_string(&l.path).ok()?;
        let lines: Vec<String> = source.lines().map(str::to_string).collect();
        (l.line >= 1 && l.line <= lines.len()).then_some(lines)
    });

    let mut out = String::new();
    out.push_str(label);
    out.push('\n');

    let bar = style.cyan("|");

    if let (Some(locus), Some(lines)) = (&locus, &window) {
        let src_line = &lines[locus.line - 1];
        let token = if is_syntax {
            near_token(head)
        } else {
            named_symbol(&message)
        };
        let (col, width) = caret_span(src_line, token);

        let first = locus.line.saturating_sub(CONTEXT).max(1);
        let last = (locus.line + CONTEXT).min(lines.len());
        let gutter_w = last.to_string().len();
        let gutter = " ".repeat(gutter_w);

        let display = display_path(&locus.path);
        out.push_str(&format!(
            "{gutter}{arrow} {display}:{line}:{col}\n",
            arrow = style.cyan("-->"),
            line = locus.line,
        ));
        out.push_str(&format!("{gutter} {bar}\n"));

        for n in first..=last {
            let num = format!("{n:>gutter_w$}");
            out.push_str(&format!(
                "{num} {bar} {text}\n",
                num = style.dim(&num),
                text = lines[n - 1],
            ));
            if n == locus.line {
                let carets = style.red(&"^".repeat(width.max(1)));
                let pad = " ".repeat(col.saturating_sub(1));
                if message.is_empty() {
                    out.push_str(&format!("{gutter} {bar} {pad}{carets}\n"));
                } else {
                    out.push_str(&format!("{gutter} {bar} {pad}{carets} {message}\n"));
                }
            }
        }

        if let Some(frames) = &traceback {
            out.push_str(&format!("{gutter} {bar}\n"));
            render_traceback(&mut out, style, frames);
        }
    } else {
        if !message.is_empty() {
            out.push_str(&format!("  {message}\n"));
        }
        if let Some(frames) = &traceback {
            out.push('\n');
            render_traceback(&mut out, style, frames);
        }
    }

    out.pop();
    out
}

fn render_traceback(out: &mut String, style: &Style, frames: &[&str]) {
    out.push_str("stack traceback:\n");
    for frame in frames {
        out.push_str(&format!("   {}\n", style.dim(frame)));
    }
}

fn clean_message(head: &str) -> String {
    let trimmed = head.trim();
    for pre in ["runtime error: ", "syntax error: "] {
        if let Some(rest) = trimmed.strip_prefix(pre) {
            return rest.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn locus_from_traceback(frames: Option<&[&str]>) -> Option<Locus> {
    frames?.iter().find_map(|frame| find_locus(frame))
}

fn named_symbol(msg: &str) -> Option<&str> {
    for kind in ["global", "local", "field", "method", "upvalue", "constant"] {
        let open = format!("({kind} '");
        if let Some(i) = msg.find(&open) {
            let start = i + open.len();
            if let Some(rel) = msg[start..].find('\'') {
                return Some(&msg[start..start + rel]);
            }
        }
    }
    None
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

// Best-effort: the token is looked up by its *first* occurrence in the line
// (Lua reports only line numbers), so a repeated token may caret the wrong
// one. Falls back to the first non-blank column.
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
    fn runtime_caret_points_at_the_named_symbol() {
        let path = write_temp("sym.lua", "task('x', function()\n    boom()\nend)\n");
        let msg = format!(
            "{}:2: attempt to call a nil value (global 'boom')",
            path.display()
        );
        let err = mlua::Error::RuntimeError(msg);
        let out = render(&err, &plain());

        let caret_line = out.lines().find(|l| l.contains('^')).unwrap();
        assert_eq!(
            caret_line.matches('^').count(),
            4,
            "width of `boom`:\n{out}"
        );
        let src_line = out.lines().find(|l| l.contains("boom()")).unwrap();
        assert_eq!(
            caret_line.find('^').unwrap(),
            src_line.find("boom").unwrap(),
            "caret not under boom:\n{out}"
        );
    }

    #[test]
    fn external_error_keeps_its_message_via_traceback_frame() {
        let path = write_temp("ext.lua", "task('x', { desc = 'x' })\n");
        let msg = format!(
            "task('x'): spec table has no `run` function\nstack traceback:\n\t{}:1: in main chunk",
            path.display()
        );
        let err = mlua::Error::RuntimeError(msg);
        let out = render(&err, &plain());

        assert!(
            out.contains("spec table has no `run` function"),
            "real message dropped:\n{out}"
        );
        let caret_line = out.lines().find(|l| l.contains('^')).unwrap();
        assert!(
            caret_line.contains("spec table has no `run` function"),
            "message not attached to the caret:\n{out}"
        );
    }

    #[test]
    fn context_lines_surround_the_offending_line() {
        let path = write_temp("ctx.lua", "before()\nboom()\nafter()\n");
        let msg = format!(
            "{}:2: attempt to call a nil value (global 'boom')",
            path.display()
        );
        let err = mlua::Error::RuntimeError(msg);
        let out = render(&err, &plain());

        assert!(out.contains("before()"), "missing line above:\n{out}");
        assert!(out.contains("after()"), "missing line below:\n{out}");
    }

    #[test]
    fn missing_locus_still_shows_the_message() {
        let err = mlua::Error::RuntimeError("something broke with no location".into());
        let out = render(&err, &plain());
        assert!(out.starts_with("runtime error\n"), "{out}");
        assert!(out.contains("something broke with no location"), "{out}");
    }

    #[test]
    fn plain_style_emits_no_escapes() {
        let path = write_temp("noesc.lua", "task('x', function()\n    boom()\nend)\n");
        let msg = format!("{}:2: boom", path.display());
        let err = mlua::Error::RuntimeError(msg);
        assert!(!render(&err, &plain()).contains('\x1b'));
    }
}
