//! Builds a [`Diagnostic`] from the errors the runtime raises. Every error type
//! is turned into the data model in exactly one place here; the renderers never
//! learn about error *kinds*.

use std::path::{Path, PathBuf};

use crate::diagnostic::{Diagnostic, Frame};
use crate::error::Error;
use crate::srcmap::display;

/// Turns any runtime [`Error`] into its [`Diagnostic`]. This is the single place
/// each top-level error kind is shaped; the renderers stay type-agnostic.
pub(crate) fn from_error(err: &Error) -> Diagnostic {
    match err {
        Error::TakufileNotFound => {
            Diagnostic::error("no Takufile.lua found in the current directory")
                .help("run 'taku init' to create one")
        }
        Error::UnknownCommand {
            name, available, ..
        } => unknown_task(name, available),
        Error::DependencyCycle(path) => {
            Diagnostic::error("dependency cycle detected").note(path.join(" -> "))
        }
        Error::Lua(e) => from_lua(e),
        Error::Task(diag) => (**diag).clone(),
        Error::TaskFailed(message) => Diagnostic::error(message.clone()),
        Error::Io(e) => Diagnostic::error(e.to_string()),
        Error::Dotenv(e) => Diagnostic::error(format!(".env: {e}")),
    }
}

/// The CLI-level "bad --jobs value" diagnostic.
pub(crate) fn bad_jobs(value: &str) -> Diagnostic {
    Diagnostic::error(format!("--jobs must be a positive integer, got '{value}'"))
        .help("omit --jobs to use the number of CPUs")
}

/// The CLI-level "no such subcommand" diagnostic (`taku build`). Unlike the
/// task list, this header has no leading space.
pub(crate) fn unknown_command(name: &str) -> Diagnostic {
    Diagnostic::error(format!("unknown command '{name}'"))
        .context("available commands:\n  init\n  run   [alias: r]\n  list  [alias: ls]")
        .help(format!("did you mean 'run {name}'?"))
}

fn unknown_task(name: &str, available: &[String]) -> Diagnostic {
    let mut diag = Diagnostic::error(format!("task '{name}' does not exist"));
    if !available.is_empty() {
        let mut block = String::from(" available tasks:");
        for task in available {
            block.push_str(&format!("\n   {task}"));
        }
        diag = diag.context(block);
    }
    if let Some(close) = crate::taskdef::closest(name, available) {
        diag = diag.help(format!("did you mean '{close}'?"));
    }
    diag
}

struct Locus {
    path: PathBuf,
    line: usize,
    msg_start: usize,
}

/// Converts an `mlua::Error` — a Lua load/runtime failure or an effect's
/// external error — into a [`Diagnostic`] with a code frame pointing at the
/// offending line.
pub(crate) fn from_lua(err: &mlua::Error) -> Diagnostic {
    let full = err.to_string();
    let (head, traceback) = split_traceback(&full);

    let is_syntax = matches!(innermost(err), mlua::Error::SyntaxError { .. });

    let (locus, detail) = match find_locus(head) {
        Some(locus) => {
            let detail = head[locus.msg_start..].trim().to_string();
            (Some(locus), detail)
        }
        None => (
            locus_from_traceback(traceback.as_deref()),
            clean_message(head),
        ),
    };

    let token = if is_syntax {
        near_token(head)
    } else {
        named_symbol(&detail)
    };
    let frame = locus.as_ref().and_then(|l| build_frame(l, token));

    // An effect may attach a structured `Diag` (note/help) to its error; keep
    // the recovered frame and layer the extra lines on top.
    let payload = structured(err);
    let (message, cause) = if let Some(p) = &payload {
        (p.message.clone(), None)
    } else if is_syntax {
        (
            "invalid syntax".to_string(),
            Some(format!("caused by Lua: {detail}")),
        )
    } else if let Some(global) = undefined_global(&detail) {
        // A bare call to an undefined name — usually a typo'd step constructor.
        (
            format!("undefined global '{global}'"),
            Some("caused by Lua: attempt to call a nil value".to_string()),
        )
    } else {
        (detail.clone(), None)
    };
    let suppress_frame = payload.as_ref().is_some_and(|p| p.no_frame);
    let mut diag = Diagnostic::error(message);
    if let Some(frame) = frame
        && !suppress_frame
    {
        diag = diag.frame(frame);
    }
    if let Some(cause) = cause {
        diag = diag.cause(cause);
    }
    if let Some(p) = payload {
        if let Some(note) = p.note {
            diag = diag.note(note);
        }
        if let Some(help) = p.help {
            diag = diag.help(help);
        }
    }
    diag
}

/// The global name in an "attempt to call a nil value (global 'x')" message.
fn undefined_global(detail: &str) -> Option<&str> {
    detail
        .strip_prefix("attempt to call a nil value (global '")?
        .strip_suffix("')")
}

/// Recovers a [`taku_api::Diag`] an effect attached to its error, if any.
fn structured(err: &mlua::Error) -> Option<taku_api::Diag> {
    if let mlua::Error::ExternalError(arc) = innermost(err) {
        return arc.downcast_ref::<taku_api::Diag>().cloned();
    }
    None
}

fn build_frame(locus: &Locus, token: Option<&str>) -> Option<Frame> {
    let source = std::fs::read_to_string(&locus.path).ok()?;
    let line = source.lines().nth(locus.line - 1)?.to_string();
    let (col, width) = caret_span(&line, token);
    Some(
        Frame::at(display(&locus.path), locus.line)
            .col(col)
            .source(line)
            .caret(col, width),
    )
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
// one. With no token, the caret spans the whole statement — from the first
// non-blank column to the end of the line, minus a trailing comma.
fn caret_span(src_line: &str, token: Option<&str>) -> (usize, usize) {
    if let Some(tok) = token.filter(|t| !t.is_empty())
        && let Some(byte) = src_line.find(tok)
    {
        let col = src_line[..byte].chars().count() + 1;
        return (col, tok.chars().count());
    }
    let indent = src_line.chars().take_while(|c| c.is_whitespace()).count();
    let content = src_line.trim_end().trim_end_matches(',').trim_end();
    let width = content.chars().count().saturating_sub(indent).max(1);
    (indent + 1, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{AnsiRenderer, Renderer};
    use crate::report::Style;

    fn render(err: &mlua::Error) -> String {
        AnsiRenderer::new(Style::for_test(false)).render(&from_lua(err))
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
        let out = render(&mlua::Error::RuntimeError(msg));

        assert!(out.starts_with("error: undefined global 'boom'"), "{out}");
        assert!(
            out.contains("= caused by Lua: attempt to call a nil value"),
            "{out}"
        );
        assert!(out.contains("-->"), "{out}");
        assert!(out.contains("2 | "), "gutter+line missing:\n{out}");
        assert!(out.contains("boom()"), "source line missing:\n{out}");
        assert!(out.contains('^'), "caret missing:\n{out}");
    }

    #[test]
    fn syntax_error_headlines_invalid_syntax_with_a_cause() {
        let path = write_temp("syn.lua", "local s = \"unterminated\n");
        let msg = format!(
            "{}:1: unfinished string near '\"unterminated'",
            path.display()
        );
        let err = mlua::Error::SyntaxError {
            message: msg,
            incomplete_input: false,
        };
        let out = render(&err);

        assert!(out.starts_with("error: invalid syntax\n"), "{out}");
        assert!(out.contains("= caused by Lua: unfinished string"), "{out}");
    }

    #[test]
    fn runtime_caret_points_at_the_named_symbol() {
        let path = write_temp("sym.lua", "task('x', function()\n    boom()\nend)\n");
        let msg = format!(
            "{}:2: attempt to call a nil value (global 'boom')",
            path.display()
        );
        let diag = from_lua(&mlua::Error::RuntimeError(msg));
        let frame = &diag.frames[0];
        assert_eq!(frame.span, (5, 4), "caret under `boom`");
    }

    #[test]
    fn external_error_keeps_its_message_via_traceback_frame() {
        let path = write_temp("ext.lua", "task('x', { desc = 'x' })\n");
        let msg = format!(
            "task('x'): spec table has no `run` function\nstack traceback:\n\t{}:1: in main chunk",
            path.display()
        );
        let diag = from_lua(&mlua::Error::RuntimeError(msg));
        assert!(
            diag.message.contains("spec table has no `run` function"),
            "real message dropped: {}",
            diag.message
        );
        assert_eq!(diag.frames.len(), 1, "locus recovered from traceback");
    }

    #[test]
    fn missing_locus_still_shows_the_message() {
        let err = mlua::Error::RuntimeError("something broke with no location".into());
        let diag = from_lua(&err);
        assert_eq!(diag.message, "something broke with no location");
        assert!(diag.frames.is_empty());
    }
}
