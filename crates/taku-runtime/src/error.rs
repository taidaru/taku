use std::fmt;
use std::path::PathBuf;

use crate::diagnostic::Diagnostic;
use crate::state::TAKUFILE;

#[derive(Debug)]
pub enum Error {
    TakufileNotFound,
    UnknownCommand {
        name: String,
        takufile: PathBuf,
        available: Vec<String>,
    },
    DependencyCycle(Vec<String>),
    TaskFailed(String),
    /// A task failed with a fully-formed diagnostic, built in the worker and
    /// rendered once on the main thread. Boxed to keep `Error` small.
    Task(Box<Diagnostic>),
    Io(std::io::Error),
    Lua(mlua::Error),
    Dotenv(dotenvy::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::TakufileNotFound => write!(f, "no {TAKUFILE} found in the current directory"),
            Error::UnknownCommand {
                name,
                takufile,
                available,
            } => {
                write!(f, "unknown command '{name}' in {}", takufile.display())?;
                if available.is_empty() {
                    write!(f, " (no commands are defined)")
                } else {
                    write!(f, "\n  available commands: {}", available.join(", "))
                }
            }
            Error::DependencyCycle(path) => {
                write!(f, "dependency cycle: {}", path.join(" -> "))
            }
            Error::TaskFailed(message) => write!(f, "{message}"),
            Error::Task(diag) => write!(f, "{}", diag.message),
            Error::Io(e) => write!(f, "{e}"),
            Error::Lua(e) => write!(f, "{e}"),
            Error::Dotenv(e) => write!(f, ".env: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        Error::Lua(e)
    }
}

impl From<dotenvy::Error> for Error {
    fn from(e: dotenvy::Error) -> Self {
        Error::Dotenv(e)
    }
}
