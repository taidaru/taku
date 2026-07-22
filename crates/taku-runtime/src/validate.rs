//! Field-schema checks for table steps: unknown key, wrong type, missing
//! required field, missing positional argument. The schema comes from each
//! step's [`StepDef`] (declared in its API crate); this module only turns a
//! violation into a diagnostic with a code frame and a `-`/`+` edit, using the
//! source map for positions.

use mlua::{Table, Value};
use taku_api::steps::{Field, FieldKind, Positional, TAG};

use crate::diagnostic::{Diagnostic, Edit};
use crate::error::Error;
use crate::srcmap::{Site, StepSite};

fn accepts(kind: FieldKind, v: &Value) -> bool {
    match kind {
        FieldKind::Str => v.is_string(),
        FieldKind::Num => v.is_number() || v.is_integer(),
        FieldKind::Bool => v.is_boolean(),
        FieldKind::Table => v.is_table(),
        FieldKind::StrOrTable => v.is_string() || v.is_table(),
    }
}

fn kind_word(kind: FieldKind) -> &'static str {
    match kind {
        FieldKind::Str => "string",
        FieldKind::Num => "number",
        FieldKind::Bool => "boolean",
        FieldKind::Table => "table",
        FieldKind::StrOrTable => "string or table",
    }
}

/// Validates a table step against its schema. An empty schema (no fields, no
/// positional) means the step takes no options, so nothing is checked.
pub(crate) fn check(
    t: &Table,
    fields: &[Field],
    positional: Option<&Positional>,
    site: Option<&StepSite>,
) -> Result<(), Error> {
    if fields.is_empty() && positional.is_none() {
        return Ok(());
    }
    // Unknown keys and wrong types.
    for pair in t.pairs::<Value, Value>() {
        let (k, v) = pair?;
        let Value::String(k) = k else { continue };
        let key = k.to_string_lossy().to_string();
        if key == TAG {
            continue;
        }
        match fields.iter().find(|f| f.name == key) {
            None => return Err(unknown_field(&key, fields, site)),
            Some(f) if !accepts(f.kind, &v) => return Err(wrong_type(&key, f.kind, &v, site)),
            _ => {}
        }
    }
    // A required first element (satisfied by `[1]` or its named-field alias) and
    // required named fields. If anything is missing, the suggestion adds *all*
    // of them at once, so the proposed step is complete and valid.
    let positional_missing = match positional {
        Some(pos) => {
            t.get::<Value>(1)?.is_nil()
                && match pos.field {
                    Some(name) => t.get::<Value>(name)?.is_nil(),
                    None => true,
                }
        }
        None => false,
    };
    let mut missing_fields = Vec::new();
    for f in fields.iter().filter(|f| f.required) {
        if t.get::<Value>(f.name)?.is_nil() {
            missing_fields.push(f.name);
        }
    }
    if positional_missing || !missing_fields.is_empty() {
        return Err(incomplete_step(
            positional.filter(|_| positional_missing),
            &missing_fields,
            site,
        ));
    }
    Ok(())
}

fn field_site<'a>(site: Option<&'a StepSite>, name: &str) -> Option<&'a Site> {
    site?.fields.iter().find(|(n, _)| n == name).map(|(_, s)| s)
}

fn task(diag: Diagnostic) -> Error {
    Error::Task(Box::new(diag))
}

fn unknown_field(key: &str, fields: &[Field], site: Option<&StepSite>) -> Error {
    let mut diag = Diagnostic::error(format!("unknown field '{key}'"));
    let fsite = field_site(site, key);
    if let Some(fs) = fsite {
        diag = diag.frame(fs.frame());
    }
    let names: Vec<String> = fields.iter().map(|f| f.name.to_string()).collect();
    if let Some(close) = crate::taskdef::closest(key, &names) {
        let msg = format!("did you mean '{close}'?");
        match fsite {
            // Replace within the field's own span, so a duplicate token
            // elsewhere on the line (a task name, another field) is untouched.
            Some(fs) => diag = diag.help_edit(msg, edit(fs, replace_in_span(fs, key, close))),
            None => diag = diag.help(msg),
        }
    }
    task(diag)
}

fn wrong_type(key: &str, kind: FieldKind, value: &Value, site: Option<&StepSite>) -> Error {
    let mut diag = Diagnostic::error(format!(
        "field '{key}' must be a {}, got {}",
        kind_word(kind),
        value.type_name()
    ));
    let fsite = field_site(site, key);
    if let Some(fs) = fsite {
        diag = diag.frame(fs.frame());
    }
    // For a quoted value that should be a number, suggest dropping the quotes.
    if kind == FieldKind::Num
        && let Value::String(s) = value
        && let Some(fs) = fsite
    {
        let raw = s.to_string_lossy();
        let after = replace_in_span(fs, &format!("\"{raw}\""), &raw);
        diag = diag.help_edit(
            format!("remove the quotes, '{key}' expects a number"),
            edit(fs, after),
        );
    }
    task(diag)
}

/// One diagnostic for every missing required part of a step, whose edit adds all
/// of them at once so the suggestion is a complete, valid step — e.g. `mv {}` →
/// `mv { "src", to = "" }`.
fn incomplete_step(
    positional: Option<&Positional>,
    missing: &[&str],
    site: Option<&StepSite>,
) -> Error {
    let (message, help) = match (positional, missing.first()) {
        (Some(pos), _) => (format!("missing {}", pos.what), pos.help.to_string()),
        (None, Some(field)) => (
            format!("missing required field '{field}'"),
            format!("add a '{field}' field with the destination path"),
        ),
        (None, None) => unreachable!("incomplete_step called with nothing missing"),
    };
    let mut diag = Diagnostic::error(message);
    if let Some(s) = site {
        diag = diag.frame(s.site.frame());
        // `"src"` for the positional, `field = ""` for each required field.
        let front = positional.map(|p| format!("\"{}\"", p.suggest));
        let back: Vec<String> = missing.iter().map(|f| format!("{f} = \"\"")).collect();
        match complete_edit(&s.site, front, back) {
            Some(after) => diag = diag.help_edit(help, edit(&s.site, after)),
            None => diag = diag.help(help),
        }
    } else {
        diag = diag.help(help);
    }
    task(diag)
}

/// An `Edit` replacing the whole source line, given the new line text.
fn edit(site: &Site, after: String) -> Edit {
    Edit {
        line: site.line,
        before: site.text.clone(),
        after,
    }
}

/// The byte range of a span (step or field) within its source line, from the
/// caret column/width — so edits land inside the right `{ … }`, not an
/// enclosing table on a single-line task, and replacements hit the right token.
fn span_bytes(site: &Site) -> (usize, usize) {
    let byte = |chars| {
        site.text
            .char_indices()
            .nth(chars)
            .map_or(site.text.len(), |(b, _)| b)
    };
    (byte(site.col - 1), byte(site.col - 1 + site.width))
}

/// Rebuilds the line with `replacement` substituted for the site's span.
fn splice(site: &Site, replacement: String) -> String {
    let (s, e) = span_bytes(site);
    format!("{}{replacement}{}", &site.text[..s], &site.text[e..])
}

/// Replaces the first `from` with `to`, but only within the site's own span.
fn replace_in_span(site: &Site, from: &str, to: &str) -> String {
    let (s, e) = span_bytes(site);
    splice(site, site.text[s..e].replacen(from, to, 1))
}

/// Rebuilds a step's `{ … }` with `front` prepended and `back` appended to the
/// existing contents, keeping the existing entries verbatim.
fn complete_edit(site: &Site, front: Option<String>, back: Vec<String>) -> Option<String> {
    let (s, e) = span_bytes(site);
    let step = &site.text[s..e];
    let open = step.find('{')?;
    let close = step.rfind('}')?;
    if close <= open {
        return None;
    }
    let inner = step[open + 1..close].trim().trim_end_matches(',').trim();
    let mut parts: Vec<String> = front.into_iter().collect();
    if !inner.is_empty() {
        parts.push(inner.to_string());
    }
    parts.extend(back);
    let new_step = format!(
        "{}{{ {} }}{}",
        &step[..open],
        parts.join(", "),
        &step[close + 1..]
    );
    Some(splice(site, new_step))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(text: &str, col: usize, width: usize) -> Site {
        Site {
            file: "f.lua".into(),
            line: 1,
            col,
            width,
            text: text.into(),
        }
    }

    #[test]
    fn complete_edit_lands_in_the_step_brace_not_an_enclosing_table() {
        // `task "b" { mv {} }` — the step `mv {}` spans cols 12..=16.
        let s = site(r#"task "b" { mv {} }"#, 12, 5);
        assert_eq!(
            complete_edit(&s, Some("\"src\"".into()), vec!["to = \"\"".into()]).unwrap(),
            r#"task "b" { mv { "src", to = "" } }"#
        );
    }

    #[test]
    fn complete_edit_keeps_existing_entries() {
        let s = site(r#"    download { url = "x" },"#, 5, 22);
        assert_eq!(
            complete_edit(&s, None, vec!["to = \"\"".into()]).unwrap(),
            r#"    download { url = "x", to = "" },"#
        );
        // positional present, only the field added
        let s2 = site(r#"mv { "a" }"#, 1, 10);
        assert_eq!(
            complete_edit(&s2, None, vec!["to = \"\"".into()]).unwrap(),
            r#"mv { "a", to = "" }"#
        );
    }

    #[test]
    fn replace_in_span_ignores_duplicates_outside_the_field() {
        // task name also contains "outupts" — only the field is fixed.
        let line = r#"task "outupts" { unchanged { "x", outupts = "t" } }"#;
        let field = site(line, 34, 18); // the `outupts = "t"` field span
        assert_eq!(
            replace_in_span(&field, "outupts", "outputs"),
            r#"task "outupts" { unchanged { "x", outputs = "t" } }"#
        );
    }
}
