//! A load-time scan of a Takufile that records where each task, step, and field
//! sits in the source — the positions Lua itself never exposes for table
//! entries. The diagnostic layer turns a [`Site`] into a code frame.
//!
//! The scanner is a small Lua lexer: it tracks strings (`"…"`, `'…'`,
//! `[[…]]`, `[==[…]==]`), comments (`--`, `--[[…]]`), and bracket nesting so
//! that a `{`, `,`, or quote *inside* a step never confuses the structure.

use std::collections::HashMap;
use std::path::Path;

/// A path shown in diagnostics: relative to the current dir when possible.
pub(crate) fn display(path: &Path) -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| path.strip_prefix(cwd).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// One code frame's worth of source: a span on a single line, with that line's
/// full text for rendering.
#[derive(Debug, Clone)]
pub(crate) struct Site {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub width: usize,
    pub text: String,
}

impl Site {
    /// A code frame for this span. Step frames show only `:line` (no column).
    pub(crate) fn frame(&self) -> crate::diagnostic::Frame {
        crate::diagnostic::Frame::at(self.file.clone(), self.line)
            .source(self.text.clone())
            .caret(self.col, self.width)
    }
}

/// A step and, when it is a table, the spans of its `key = value` fields (for
/// field-level frames and edits).
#[derive(Debug, Clone)]
pub(crate) struct StepSite {
    pub site: Site,
    pub fields: Vec<(String, Site)>,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskSite {
    pub def: Site,
    pub steps: Vec<StepSite>,
    pub params: Vec<(String, Site)>,
    /// The `{ … }` body text, for scanning `${param}` usage.
    pub body: String,
}

/// The load-time source map: last definition per name (what the executor reads)
/// plus every definition in order (for the "defined N times" warning).
#[derive(Default)]
pub(crate) struct Sources {
    pub map: HashMap<String, TaskSite>,
    pub all: Vec<(String, TaskSite)>,
}

impl Sources {
    /// Merges one file's tasks (later definitions win in `map`).
    pub(crate) fn add(&mut self, file: &str, source: &str) {
        for (name, site) in scan_all(file, source) {
            self.map.insert(name.clone(), site.clone());
            self.all.push((name, site));
        }
    }
}

/// Every task definition in source order (duplicates preserved).
pub(crate) fn scan_all(file: &str, source: &str) -> Vec<(String, TaskSite)> {
    Scanner::new(file, source).run()
}

struct Scanner<'a> {
    file: &'a str,
    src: &'a [u8],
    text: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> Scanner<'a> {
    fn new(file: &'a str, text: &'a str) -> Self {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Scanner {
            file,
            src: text.as_bytes(),
            text,
            line_starts,
        }
    }

    fn run(&self) -> Vec<(String, TaskSite)> {
        let mut tasks = Vec::new();
        let mut i = 0;
        while i < self.src.len() {
            if let Some(after) = self.skip_trivia(i) {
                i = after;
                continue;
            }
            // A `task` identifier at a word boundary starts a task call.
            if self.word_at(i, b"task")
                && let Some((name, task, next)) = self.parse_task(i + 4)
            {
                tasks.push((name, task));
                i = next;
                continue;
            }
            // Skip a whole identifier so `taskfoo` isn't matched as `task`.
            if is_ident_start(self.src[i]) {
                i += 1;
                while i < self.src.len() && is_ident(self.src[i]) {
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
        tasks
    }

    /// If `i` is at whitespace, a comment, or a string, returns the offset just
    /// past it; otherwise `None`.
    fn skip_trivia(&self, i: usize) -> Option<usize> {
        match self.src.get(i)? {
            b' ' | b'\t' | b'\r' | b'\n' => Some(i + 1),
            _ if self.at_comment(i) => Some(self.skip_comment(i)),
            b'"' | b'\'' => Some(self.skip_quote(i)),
            b'[' if self.long_bracket(i).is_some() => Some(self.skip_long(i)),
            _ => None,
        }
    }

    fn at_comment(&self, i: usize) -> bool {
        self.src.get(i) == Some(&b'-') && self.src.get(i + 1) == Some(&b'-')
    }

    fn skip_comment(&self, i: usize) -> usize {
        // `--[[ … ]]` block comment, else to end of line.
        if self.long_bracket(i + 2).is_some() {
            return self.skip_long(i + 2);
        }
        let mut j = i + 2;
        while j < self.src.len() && self.src[j] != b'\n' {
            j += 1;
        }
        j
    }

    fn skip_quote(&self, i: usize) -> usize {
        let quote = self.src[i];
        let mut j = i + 1;
        while j < self.src.len() {
            match self.src[j] {
                b'\\' => j += 2,
                b if b == quote => return j + 1,
                b'\n' => return j, // unterminated; stop at the line
                _ => j += 1,
            }
        }
        j
    }

    /// If `i` opens a Lua long bracket (`[[` or `[=*[`), returns its level.
    fn long_bracket(&self, i: usize) -> Option<usize> {
        if self.src.get(i) != Some(&b'[') {
            return None;
        }
        let mut j = i + 1;
        while self.src.get(j) == Some(&b'=') {
            j += 1;
        }
        (self.src.get(j) == Some(&b'[')).then_some(j - i - 1)
    }

    fn skip_long(&self, i: usize) -> usize {
        let level = match self.long_bracket(i) {
            Some(l) => l,
            None => return i + 1,
        };
        let mut close = String::from("]");
        close.push_str(&"=".repeat(level));
        close.push(']');
        let start = i + level + 2;
        match self.text[start..].find(&close) {
            Some(off) => start + off + close.len(),
            None => self.src.len(),
        }
    }

    fn word_at(&self, i: usize, word: &[u8]) -> bool {
        if i > 0 && is_ident(self.src[i - 1]) {
            return false;
        }
        self.src[i..].starts_with(word)
            && !self.src.get(i + word.len()).is_some_and(|b| is_ident(*b))
    }

    /// Parses a `task` call starting just after the `task` keyword. Returns the
    /// task's sites and the offset to resume scanning at.
    fn parse_task(&self, mut i: usize) -> Option<(String, TaskSite, usize)> {
        i = self.skip_ws(i);
        if self.src.get(i) == Some(&b'(') {
            i = self.skip_ws(i + 1);
        }
        // The header is a short string literal.
        let (header, hstart, hend) = self.read_string(i)?;
        let header_open = hstart; // byte of the opening quote
        i = self.skip_ws(hend);
        if self.src.get(i) == Some(&b',') {
            i = self.skip_ws(i + 1);
        }
        if self.src.get(i) != Some(&b'{') {
            return None;
        }
        let (entries, close) = self.parse_table(i);
        let name = header[..header.find([' ', '<', ':']).unwrap_or(header.len())]
            .trim()
            .to_string();
        let def = self.header_name_site(&header, header_open);
        let params = self.header_param_sites(&header, header_open);
        let steps = entries.iter().map(|e| self.step_site(*e)).collect();
        let body = self.text[i..(close + 1).min(self.src.len())].to_string();
        let next = close.max(i + 1);
        Some((
            name,
            TaskSite {
                def,
                steps,
                params,
                body,
            },
            next,
        ))
    }

    /// Reads a `"…"`/`'…'`/long string at `i`, returning its unescaped-ish
    /// contents and byte span `[open, after)`.
    fn read_string(&self, i: usize) -> Option<(String, usize, usize)> {
        match self.src.get(i)? {
            b'"' | b'\'' => {
                let end = self.skip_quote(i);
                let inner = &self.text[i + 1..end.saturating_sub(1).max(i + 1)];
                Some((inner.to_string(), i, end))
            }
            b'[' if self.long_bracket(i).is_some() => {
                let level = self.long_bracket(i).unwrap();
                let end = self.skip_long(i);
                let inner = &self.text[i + level + 2..end.saturating_sub(level + 2).max(i)];
                Some((inner.to_string(), i, end))
            }
            _ => None,
        }
    }

    fn skip_ws(&self, mut i: usize) -> usize {
        loop {
            match self.src.get(i) {
                Some(b' ' | b'\t' | b'\r' | b'\n') => i += 1,
                _ if self.at_comment(i) => i = self.skip_comment(i),
                _ => return i,
            }
        }
    }

    /// Walks a `{ … }` table from its opening brace, returning each depth-1
    /// entry's byte span `[start, end)` (trailing whitespace trimmed) and the
    /// offset of the closing brace.
    fn parse_table(&self, open: usize) -> (Vec<(usize, usize)>, usize) {
        let mut entries = Vec::new();
        let mut i = open + 1;
        let mut depth = 0i32;
        let mut start = None;
        let push = |entries: &mut Vec<(usize, usize)>, start: usize, end: usize| {
            let s = self.trim_start(start, end);
            let e = self.trim_end(start, end);
            if e > s {
                entries.push((s, e));
            }
        };
        while i < self.src.len() {
            if let Some(after) = self.skip_trivia(i) {
                // A string is entry content and starts an entry; whitespace and
                // comments are not — a comment must never anchor the step's site.
                let is_string =
                    matches!(self.src[i], b'"' | b'\'') || self.long_bracket(i).is_some();
                if start.is_none() && is_string {
                    start = Some(i);
                }
                i = after;
                continue;
            }
            match self.src[i] {
                b'{' | b'(' | b'[' => {
                    if start.is_none() {
                        start = Some(i);
                    }
                    depth += 1;
                    i += 1;
                }
                b')' | b']' => {
                    depth -= 1;
                    i += 1;
                }
                b'}' if depth == 0 => {
                    if let Some(s) = start.take() {
                        push(&mut entries, s, i);
                    }
                    return (entries, i);
                }
                b'}' => {
                    depth -= 1;
                    i += 1;
                }
                // `,` and `;` are both table-entry separators in Lua.
                b',' | b';' if depth == 0 => {
                    if let Some(s) = start.take() {
                        push(&mut entries, s, i);
                    }
                    i += 1;
                }
                _ if is_ident_start(self.src[i]) => {
                    if start.is_none() {
                        start = Some(i);
                    }
                    // Block keywords nest like brackets, so commas inside a
                    // `function(ctx) … end` body never split the step. (A bare
                    // `do … end` block isn't counted — vanishingly rare in a
                    // task function.)
                    let word_start = i;
                    i += 1;
                    while i < self.src.len() && is_ident(self.src[i]) {
                        i += 1;
                    }
                    match &self.text[word_start..i] {
                        "function" | "if" | "for" | "while" | "repeat" => depth += 1,
                        "end" | "until" => depth -= 1,
                        _ => {}
                    }
                }
                _ => {
                    if start.is_none() {
                        start = Some(i);
                    }
                    i += 1;
                }
            }
        }
        (entries, self.src.len())
    }

    fn trim_start(&self, mut s: usize, e: usize) -> usize {
        while s < e && matches!(self.src[s], b' ' | b'\t' | b'\r' | b'\n') {
            s += 1;
        }
        s
    }
    fn trim_end(&self, s: usize, mut e: usize) -> usize {
        while e > s && matches!(self.src[e - 1], b' ' | b'\t' | b'\r' | b'\n') {
            e -= 1;
        }
        e
    }

    /// Builds a step's site: the whole step, plus its table fields and closing
    /// brace when it has a `{ … }`.
    fn step_site(&self, span: (usize, usize)) -> StepSite {
        let site = self.site(span.0, span.1);
        let mut fields = Vec::new();
        // The step's own table is the first `{` inside its span (a constructor
        // table like `unchanged { … }`, or a bare `{ … }` step).
        if let Some(open) = self.find_brace(span.0, span.1) {
            let (entries, _close) = self.parse_table(open);
            for (fs, fe) in entries {
                if let Some(name) = self.field_name(fs, fe) {
                    fields.push((name, self.site(fs, fe)));
                }
            }
        }
        StepSite { site, fields }
    }

    fn find_brace(&self, s: usize, e: usize) -> Option<usize> {
        let mut i = s;
        while i < e {
            if let Some(after) = self.skip_trivia(i) {
                i = after;
                continue;
            }
            if self.src[i] == b'{' {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// If a table entry is `name = value`, its key.
    fn field_name(&self, s: usize, e: usize) -> Option<String> {
        let mut i = s;
        if !is_ident_start(*self.src.get(i)?) {
            return None;
        }
        while i < e && is_ident(self.src[i]) {
            i += 1;
        }
        let name = &self.text[s..i];
        let j = self.skip_ws(i);
        (self.src.get(j) == Some(&b'=') && self.src.get(j + 1) != Some(&b'='))
            .then(|| name.to_string())
    }

    /// The site of the task name inside its header string.
    fn header_name_site(&self, header: &str, header_open: usize) -> Site {
        // name is the header up to the first space, `<`, or `:`.
        let name_len = header.find([' ', '<', ':']).unwrap_or(header.len());
        let start = header_open + 1; // past the opening quote
        self.site(start, start + name_len)
    }

    fn header_param_sites(&self, header: &str, header_open: usize) -> Vec<(String, Site)> {
        let mut out = Vec::new();
        let base = header_open + 1;
        let bytes = header.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'<' {
                let start = i;
                let end = header[i..]
                    .find('>')
                    .map(|o| i + o + 1)
                    .unwrap_or(header.len());
                let inner = &header[i + 1..end.saturating_sub(1).max(i + 1)];
                let name = inner.split('=').next().unwrap_or(inner).trim();
                if !name.is_empty() {
                    out.push((name.to_string(), self.site(base + start, base + end)));
                }
                i = end;
            } else {
                i += 1;
            }
        }
        out
    }

    /// Converts a byte span to a single-line [`Site`] (first line if it spans
    /// several), carrying that line's full text.
    fn site(&self, start: usize, end: usize) -> Site {
        let line = self.line_of(start);
        let line_start = self.line_starts[line - 1];
        let line_end = self
            .line_starts
            .get(line)
            .map_or(self.src.len(), |&s| s.saturating_sub(1));
        // A char offset (not byte): the caret column and field-edit splicing
        // count characters, so a multi-byte char before the span must not shift it.
        let col = self.text[line_start..start].chars().count() + 1;
        // Clamp the width to the first line.
        let end = end.min(line_end);
        let width = self.text[start..end.max(start)].chars().count().max(1);
        Site {
            file: self.file.to_string(),
            line,
            col,
            width,
            text: self.text[line_start..line_end].to_string(),
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

fn is_ident_start(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphabetic()
}
fn is_ident(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(file: &str, source: &str) -> HashMap<String, TaskSite> {
        let mut s = Sources::default();
        s.add(file, source);
        s.map
    }

    fn task<'a>(m: &'a HashMap<String, TaskSite>, name: &str) -> &'a TaskSite {
        m.get(name).unwrap_or_else(|| panic!("no task {name}"))
    }

    #[test]
    fn one_step_per_line() {
        let src = "task \"build <p=debug>\" {\n    \"cargo build\",\n    rm \"x\",\n}\n";
        let m = scan("Takufile.lua", src);
        let t = task(&m, "build");
        assert_eq!(t.def.line, 1);
        assert_eq!(
            &t.def.text[t.def.col - 1..t.def.col - 1 + t.def.width],
            "build"
        );
        assert_eq!(t.steps.len(), 2);
        assert_eq!(t.steps[0].site.line, 2);
        assert_eq!(t.steps[0].site.text.trim(), "\"cargo build\",");
        // caret spans the string, not the comma
        let s = &t.steps[0].site;
        assert_eq!(&s.text[s.col - 1..s.col - 1 + s.width], "\"cargo build\"");
        assert_eq!(t.steps[1].site.line, 3);
    }

    #[test]
    fn multiline_step_and_fields() {
        let src = "task \"b\" {\n    unchanged {\n        \"src/**/*.rs\",\n        outupts = \"target\",\n    },\n}\n";
        let m = scan("f.lua", src);
        let t = task(&m, "b");
        assert_eq!(t.steps.len(), 1);
        assert_eq!(t.steps[0].site.line, 2); // caret on the first line
        let fields = &t.steps[0].fields;
        assert!(
            fields.iter().any(|(n, s)| n == "outupts" && s.line == 4),
            "{fields:?}"
        );
    }

    #[test]
    fn strings_with_braces_and_commas_do_not_confuse_split() {
        let src = "task \"b\" {\n    \"echo {a,b,c}\",\n    \"second\",\n}\n";
        let m = scan("f.lua", src);
        assert_eq!(task(&m, "b").steps.len(), 2);
    }

    #[test]
    fn a_comment_before_a_step_does_not_anchor_it() {
        let src = "task \"b\" {\n    -- cp { to = \"\" }\n    mv {},\n}\n";
        let m = scan("f.lua", src);
        let t = task(&m, "b");
        assert_eq!(t.steps.len(), 1);
        // the step is `mv {}` on line 3, not the comment on line 2
        assert_eq!(t.steps[0].site.line, 3);
        assert_eq!(t.steps[0].site.text.trim(), "mv {},");
    }

    #[test]
    fn a_function_body_with_commas_stays_one_step() {
        let src = "task \"b\" {\n    function(ctx)\n        local a, b = 1, 2\n        if a then return end\n    end,\n    \"after\",\n}\n";
        let m = scan("f.lua", src);
        let t = task(&m, "b");
        // the function is one step, then "after" — not split on inner commas
        assert_eq!(t.steps.len(), 2);
        assert_eq!(t.steps[0].site.line, 2);
        assert_eq!(t.steps[1].site.text.trim(), "\"after\",");
    }

    #[test]
    fn semicolon_separates_entries() {
        let src = "task \"b\" { \"a\"; \"b\"; \"c\" }\n";
        let m = scan("f.lua", src);
        assert_eq!(task(&m, "b").steps.len(), 3);
    }

    #[test]
    fn comments_and_long_strings_are_skipped() {
        let src = "-- task \"ghost\" {}\ntask \"real\" {\n    --[[ }, ]]\n    [[ literal , }} ]],\n    \"x\",\n}\n";
        let m = scan("f.lua", src);
        assert!(!m.contains_key("ghost"));
        assert_eq!(task(&m, "real").steps.len(), 2);
    }

    #[test]
    fn paren_call_form_and_params() {
        let src = "task(\"deploy <env> <region=eu>: build\", {\n    \"do it\",\n})\n";
        let m = scan("f.lua", src);
        let t = task(&m, "deploy");
        assert_eq!(t.def.line, 1);
        assert_eq!(t.steps.len(), 1);
        let names: Vec<&str> = t.params.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["env", "region"]);
    }

    #[test]
    fn taskfoo_is_not_a_task() {
        let src = "taskfoo = 1\ntask \"real\" { \"x\" }\n";
        let m = scan("f.lua", src);
        assert!(!m.contains_key("taskfoo"));
        assert!(m.contains_key("real"));
    }

    #[test]
    fn caret_column_counts_chars_not_bytes() {
        // A multi-byte comment precedes the step on the same line; `col` must
        // land on the step's character position, not its byte offset.
        let src = "task \"b\" { --[[café]] mv {}, }\n";
        let m = scan("f.lua", src);
        let step = &task(&m, "b").steps[0].site;
        // `col` is a 1-based char offset: it must equal the char index of `mv`,
        // which differs from its byte offset because of the `é` before it.
        let char_idx = step
            .text
            .char_indices()
            .position(|(_, c)| c == 'm')
            .unwrap();
        let byte_idx = step.text.find("mv").unwrap();
        assert_eq!(step.col - 1, char_idx, "col is a char offset");
        assert_ne!(
            char_idx, byte_idx,
            "test needs a multi-byte char before the step"
        );
    }
}
