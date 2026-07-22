//! The machine-facing renderer: one JSON object per diagnostic (JSON Lines).
//! Every line in `--json` mode carries an `event` discriminator
//! (`diagnostic`, `task`, `summary`, `output`) so the stream is uniform.
//! Hand-rolled — the schema is small and the project avoids heavy deps.

use crate::diagnostic::{Diagnostic, Frame, Level, Note, Renderer};
use taku_api::steps::json_string;

pub(crate) struct JsonRenderer;

impl Renderer for JsonRenderer {
    fn render(&self, diag: &Diagnostic) -> String {
        let level = match diag.level {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        };
        object(vec![
            ("event", json_string("diagnostic")),
            ("level", json_string(level)),
            ("message", json_string(&diag.message)),
            ("frames", array(&diag.frames, frame)),
            ("causes", array(&diag.causes, |c| json_string(c))),
            ("notes", array(&diag.notes, note)),
            ("helps", array(&diag.helps, help)),
        ])
    }
}

/// A code frame: `column`/`length` describe the caret span (where the problem
/// is); the presentational `-->` column is left out.
fn frame(f: &Frame) -> String {
    object(vec![
        ("file", json_string(&f.file)),
        ("line", f.line.to_string()),
        ("column", f.span.0.to_string()),
        ("length", f.span.1.to_string()),
    ])
}

fn note(n: &Note) -> String {
    let mut fields = vec![("text", json_string(&n.text))];
    if let Some(fr) = &n.frame {
        fields.push(("frame", frame(fr)));
    }
    object(fields)
}

fn help(h: &crate::diagnostic::Help) -> String {
    let mut fields = vec![("message", json_string(&h.message))];
    if let Some(e) = &h.edit {
        fields.push((
            "edit",
            object(vec![
                ("line", e.line.to_string()),
                ("before", json_string(&e.before)),
                ("after", json_string(&e.after)),
            ]),
        ));
    }
    object(fields)
}

/// Joins `key: value` pairs into a JSON object (absent optional keys are simply
/// omitted, keeping the common case compact).
fn object(fields: Vec<(&str, String)>) -> String {
    let inner: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}:{v}", json_string(k)))
        .collect();
    format!("{{{}}}", inner.join(","))
}

fn array<T>(items: &[T], each: impl Fn(&T) -> String) -> String {
    let inner: Vec<String> = items.iter().map(each).collect();
    format!("[{}]", inner.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{Edit, Frame};

    #[test]
    fn every_line_has_an_event_and_a_clean_shape() {
        let diag = Diagnostic::error("bad \"quote\"\nand newline")
            .frame(Frame::at("Takufile.lua", 8).col(5).source("x").caret(5, 3))
            .cause("caused by Lua: boom")
            .help_edit(
                "did you mean 'y'?",
                Edit {
                    line: 8,
                    before: "x".into(),
                    after: "y".into(),
                },
            );
        let out = JsonRenderer.render(&diag);
        assert!(
            out.starts_with(r#"{"event":"diagnostic","level":"error""#),
            "{out}"
        );
        assert!(out.contains(r#"bad \"quote\"\nand newline"#), "{out}");
        assert!(
            out.contains(r#""frames":[{"file":"Takufile.lua","line":8,"column":5,"length":3}]"#),
            "{out}"
        );
        assert!(
            out.contains(r#""helps":[{"message":"did you mean 'y'?","edit":{"line":8,"before":"x","after":"y"}}]"#),
            "{out}"
        );
    }

    #[test]
    fn help_without_edit_omits_the_key() {
        let out = JsonRenderer.render(&Diagnostic::warning("x").help("do y"));
        assert!(out.contains(r#""helps":[{"message":"do y"}]"#), "{out}");
    }
}
