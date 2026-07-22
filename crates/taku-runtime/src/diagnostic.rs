/// Severity of a [`Diagnostic`]. `Error` is fatal, `Warning` prints and the run
/// continues, `Info` is a bare informational line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Info,
}

/// A code frame: one source line with a caret span under it. The renderer draws
/// the `--> file:line`, the `|` gutter, the line, and the carets.
#[derive(Debug, Clone)]
pub struct Frame {
    pub file: String,
    pub line: usize,
    /// Column shown after the line in the `-->` header, when known.
    pub col: Option<usize>,
    pub source: String,
    /// 1-based caret start column and width.
    pub span: (usize, usize),
    /// Message printed after the carets.
    pub label: Option<String>,
}

impl Frame {
    pub fn at(file: impl Into<String>, line: usize) -> Self {
        Frame {
            file: file.into(),
            line,
            col: None,
            source: String::new(),
            span: (1, 1),
            label: None,
        }
    }
    pub fn col(mut self, col: usize) -> Self {
        self.col = Some(col);
        self
    }
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
    pub fn caret(mut self, start: usize, width: usize) -> Self {
        self.span = (start.max(1), width.max(1));
        self
    }
}

/// A secondary block under the headline. A labelled note renders `note: <text>`
/// (dim label); an unlabelled one renders its `text` verbatim (the indented
/// "available tasks:" list). Either may carry its own code frame — as the
/// "also defined here" frames of a redefinition warning do.
#[derive(Debug, Clone)]
pub struct Note {
    /// `Some("note")` → `note: <text>`; `None` → `text` printed verbatim.
    pub label: Option<String>,
    pub text: String,
    pub frame: Option<Frame>,
}

/// A `help:` line, optionally carrying a suggested one-line `-`/`+` edit.
#[derive(Debug, Clone)]
pub struct Help {
    pub message: String,
    pub edit: Option<Edit>,
}

/// A single-line replacement suggestion, rendered as a `line -`/`line +` diff.
#[derive(Debug, Clone)]
pub struct Edit {
    pub line: usize,
    pub before: String,
    pub after: String,
}

/// The single data model every error, warning, and info flows through. It holds
/// no rendering logic and no colour — a `Renderer` turns it into text.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub frames: Vec<Frame>,
    /// `= caused by ...` lines, rendered inside the frame gutter.
    pub causes: Vec<String>,
    pub notes: Vec<Note>,
    pub helps: Vec<Help>,
}

impl Diagnostic {
    fn new(level: Level, message: impl Into<String>) -> Self {
        Diagnostic {
            level,
            message: message.into(),
            frames: Vec::new(),
            causes: Vec::new(),
            notes: Vec::new(),
            helps: Vec::new(),
        }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Level::Error, message)
    }
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Level::Warning, message)
    }
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(Level::Info, message)
    }
    pub fn frame(mut self, frame: Frame) -> Self {
        self.frames.push(frame);
        self
    }
    pub fn cause(mut self, cause: impl Into<String>) -> Self {
        self.causes.push(cause.into());
        self
    }
    pub fn note(mut self, text: impl Into<String>) -> Self {
        self.notes.push(Note {
            label: Some("note".into()),
            text: text.into(),
            frame: None,
        });
        self
    }
    pub fn note_frame(mut self, text: impl Into<String>, frame: Frame) -> Self {
        self.notes.push(Note {
            label: Some("note".into()),
            text: text.into(),
            frame: Some(frame),
        });
        self
    }
    /// An unlabelled block printed verbatim (e.g. the "available tasks:" list).
    pub fn context(mut self, text: impl Into<String>) -> Self {
        self.notes.push(Note {
            label: None,
            text: text.into(),
            frame: None,
        });
        self
    }
    pub fn help(mut self, message: impl Into<String>) -> Self {
        self.helps.push(Help {
            message: message.into(),
            edit: None,
        });
        self
    }
    pub fn help_edit(mut self, message: impl Into<String>, edit: Edit) -> Self {
        self.helps.push(Help {
            message: message.into(),
            edit: Some(edit),
        });
        self
    }
}

/// Turns a [`Diagnostic`] into output text. Implementors know nothing about
/// error *types* — only the data tree in front of them.
pub(crate) trait Renderer {
    fn render(&self, diag: &Diagnostic) -> String;
}

mod ansi;
mod json;

pub(crate) use ansi::AnsiRenderer;
pub(crate) use json::JsonRenderer;

mod convert;

pub(crate) use convert::{bad_jobs, from_error, from_lua, unknown_command};

/// Chooses the renderer for the requested output format.
pub(crate) fn renderer(json: bool, style: crate::report::Style) -> Box<dyn Renderer> {
    if json {
        Box::new(JsonRenderer)
    } else {
        Box::new(AnsiRenderer::new(style))
    }
}
