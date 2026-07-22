//! The human-facing renderer: turns a [`Diagnostic`] into rustc-style code
//! frames and coloured labels. All layout lives here; the model carries no
//! formatting.

use crate::diagnostic::{Diagnostic, Edit, Frame, Level, Note, Renderer};
use crate::report::Style;

/// SGR codes, kept in one place so the palette is easy to read and tweak.
const LABEL_ERROR: &str = "1;31"; // bold red
const LABEL_WARNING: &str = "1;33"; // bold yellow
const LABEL_INFO: &str = "2"; // dim
const BAR: &str = "1;34"; // bold blue — `-->`, `|`, `=`
const NUM: &str = "2;1"; // dim bold — gutter line numbers
const CARET_ERROR: &str = "1;31"; // bold red — carets under an error
const CARET_WARNING: &str = "1;33"; // bold yellow — carets under a warning
const NOTE: &str = "2;1"; // dim bold — `note:`
const HELP: &str = "1;36"; // bold cyan — the whole `help:` line
const ADD: &str = "1;32"; // bold green — a `+` edit line
const REMOVE: &str = "1;31"; // bold red — a `-` edit line

pub(crate) struct AnsiRenderer {
    style: Style,
}

impl AnsiRenderer {
    pub(crate) fn new(style: Style) -> Self {
        AnsiRenderer { style }
    }
}

impl Renderer for AnsiRenderer {
    fn render(&self, diag: &Diagnostic) -> String {
        let s = &self.style;
        let (label, code) = match diag.level {
            Level::Error => ("error", LABEL_ERROR),
            Level::Warning => ("warning", LABEL_WARNING),
            Level::Info => ("info", LABEL_INFO),
        };

        // Carets take the severity's colour — yellow for warnings so they don't
        // read as an error.
        let caret = match diag.level {
            Level::Warning => CARET_WARNING,
            _ => CARET_ERROR,
        };

        let mut out = String::new();
        out.push_str(&format!(
            "{} {}\n",
            s.sgr(code, &format!("{label}:")),
            diag.message
        ));

        for frame in &diag.frames {
            frame_block(&mut out, s, frame, caret);
        }
        // Causes hang off the last frame's gutter (or a default gutter when the
        // error has no locus, e.g. an incomplete-input syntax error at EOF).
        if !diag.causes.is_empty() {
            let w = diag.frames.last().map_or(2, |f| gutter(f.line));
            for cause in &diag.causes {
                out.push_str(&format!("{}{} {cause}\n", pad(w + 2), s.sgr(BAR, "=")));
            }
        }

        for note in &diag.notes {
            note_block(&mut out, s, note, caret);
        }

        for help in &diag.helps {
            out.push('\n');
            out.push_str(&format!(
                "{}\n",
                s.sgr(HELP, &format!("help: {}", help.message))
            ));
            if let Some(edit) = &help.edit {
                edit_block(&mut out, s, edit);
            }
        }

        out.pop(); // trailing newline; the caller prints with a newline
        out
    }
}

/// A code frame: the `-->` header, the source line, and the carets under it.
fn frame_block(out: &mut String, s: &Style, frame: &Frame, caret: &str) {
    let w = gutter(frame.line);
    let bar = s.sgr(BAR, "|");

    let loc = match frame.col {
        Some(col) => format!("{}:{}:{col}", frame.file, frame.line),
        None => format!("{}:{}", frame.file, frame.line),
    };
    out.push_str(&format!("{}{} {loc}\n", pad(w + 1), s.sgr(BAR, "-->")));
    out.push_str(&format!("{}{bar}\n", pad(w + 2)));

    let num = s.sgr(NUM, &format!("{:>w$}", frame.line));
    out.push_str(&format!(" {num} {bar} {}\n", frame.source));

    let (col, width) = frame.span;
    let carets = s.sgr(caret, &"^".repeat(width));
    let gap = pad(col.saturating_sub(1));
    match &frame.label {
        Some(label) => out.push_str(&format!("{}{bar} {gap}{carets} {label}\n", pad(w + 2))),
        None => out.push_str(&format!("{}{bar} {gap}{carets}\n", pad(w + 2))),
    }
    out.push_str(&format!("{}{bar}\n", pad(w + 2)));
}

fn note_block(out: &mut String, s: &Style, note: &Note, caret: &str) {
    match &note.label {
        Some(label) => {
            out.push_str(&format!(
                "{} {}\n",
                s.sgr(NOTE, &format!("{label}:")),
                note.text
            ));
        }
        None => {
            out.push_str(&note.text);
            out.push('\n');
        }
    }
    if let Some(frame) = &note.frame {
        frame_block(out, s, frame, caret);
    }
}

/// A `line -`/`line +` suggestion: the `-` line bold red, the `+` line bold
/// green, under a bar row like a frame.
fn edit_block(out: &mut String, s: &Style, edit: &Edit) {
    let w = gutter(edit.line);
    out.push_str(&format!("{}{}\n", pad(w + 2), s.sgr(BAR, "|")));
    let row = |code: &str, sign: &str, text: &str| {
        format!(
            " {} {}\n",
            s.sgr(NUM, &format!("{:>w$}", edit.line)),
            s.sgr(code, &format!("{sign} {text}")),
        )
    };
    out.push_str(&row(REMOVE, "-", &edit.before));
    out.push_str(&row(ADD, "+", &edit.after));
}

/// Gutter width: the line-number column is at least two wide.
fn gutter(line: usize) -> usize {
    line.to_string().len().max(2)
}

fn pad(n: usize) -> String {
    " ".repeat(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> AnsiRenderer {
        AnsiRenderer::new(Style::for_test(false))
    }

    #[test]
    fn frame_with_cause_matches_the_spec() {
        let src = r#"    unchangd { "src/**/*", outputs = "target" },"#;
        let diag = Diagnostic::error("undefined global 'unchangd'")
            .frame(Frame::at("Takufile.lua", 20).col(5).source(src).caret(5, 8))
            .cause("caused by Lua: attempt to call a nil value")
            .help("did you mean 'unchanged'?");
        let expected = "\
error: undefined global 'unchangd'
   --> Takufile.lua:20:5
    |
 20 |     unchangd { \"src/**/*\", outputs = \"target\" },
    |     ^^^^^^^^
    |
    = caused by Lua: attempt to call a nil value

help: did you mean 'unchanged'?";
        assert_eq!(plain().render(&diag), expected);
    }

    #[test]
    fn context_block_and_help_without_a_frame() {
        let diag = Diagnostic::error("task 'buidl' does not exist")
            .context(" available tasks:\n   api\n   build")
            .help("did you mean 'build'?");
        let expected = "\
error: task 'buidl' does not exist
 available tasks:
   api
   build

help: did you mean 'build'?";
        assert_eq!(plain().render(&diag), expected);
    }

    #[test]
    fn single_digit_line_pads_the_gutter_to_two() {
        let diag = Diagnostic::error("command exited with status 1")
            .frame(
                Frame::at("Takufile.lua", 8)
                    .source("    \"cargo build\",")
                    .caret(5, 13),
            )
            .note("cargo build --profile debug");
        let out = plain().render(&diag);
        assert!(out.contains("   --> Takufile.lua:8\n"), "{out}");
        assert!(out.contains("  8 |     \"cargo build\",\n"), "{out}");
        assert!(out.contains("\nnote: cargo build --profile debug"), "{out}");
    }

    #[test]
    fn plain_style_emits_no_escapes() {
        let diag =
            Diagnostic::warning("something").frame(Frame::at("f.lua", 3).source("x").caret(1, 1));
        assert!(!plain().render(&diag).contains('\x1b'));
    }
}
