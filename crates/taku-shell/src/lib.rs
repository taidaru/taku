use std::fmt::Display;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Arc;

use mlua::{Lua, Table, Value};

#[derive(Default)]
pub struct Opts {
    pub stdin: Option<Vec<u8>>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
}

pub struct Capture {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait Shell: Send + Sync {
    fn run(&self, argv: &[String], opts: &Opts) -> mlua::Result<i32>;
    fn capture(&self, argv: &[String], opts: &Opts) -> mlua::Result<Capture>;
}

pub struct Local;

impl Local {
    fn command(argv: &[String], opts: &Opts) -> Command {
        // Callers must pass a non-empty argv (enforced at the Lua boundary by
        // `parse_argv`); this guards the invariant for direct Rust callers.
        debug_assert!(!argv.is_empty(), "argv must be non-empty");
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        if let Some(cwd) = &opts.cwd {
            command.current_dir(cwd);
        }
        command.envs(opts.env.iter().map(|(k, v)| (k, v)));
        command
    }
}

fn err<E: Display>(op: &str, argv: &[String], e: E) -> mlua::Error {
    mlua::Error::external(format!("sh.{op}({}): {e}", argv.join(" ")))
}

impl Shell for Local {
    fn run(&self, argv: &[String], opts: &Opts) -> mlua::Result<i32> {
        let mut command = Local::command(argv, opts);
        let status = match &opts.stdin {
            None => command.status().map_err(|e| err("run", argv, e))?,
            Some(data) => {
                command.stdin(Stdio::piped());
                let mut child = command.spawn().map_err(|e| err("run", argv, e))?;
                child
                    .stdin
                    .take()
                    .ok_or_else(|| err("run", argv, "could not open child stdin"))?
                    .write_all(data)
                    .map_err(|e| err("run", argv, e))?;
                child.wait().map_err(|e| err("run", argv, e))?
            }
        };
        Ok(status.code().unwrap_or(-1))
    }

    fn capture(&self, argv: &[String], opts: &Opts) -> mlua::Result<Capture> {
        let mut command = Local::command(argv, opts);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        command.stdin(if opts.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let mut child = command.spawn().map_err(|e| err("capture", argv, e))?;
        if let Some(data) = &opts.stdin {
            child
                .stdin
                .take()
                .ok_or_else(|| err("capture", argv, "could not open child stdin"))?
                .write_all(data)
                .map_err(|e| err("capture", argv, e))?;
        }
        let out = child
            .wait_with_output()
            .map_err(|e| err("capture", argv, e))?;
        Ok(Capture {
            code: out.status.code().unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }
}

pub fn capture_table(lua: &Lua, out: Capture) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("code", out.code)?;
    t.set("stdout", lua.create_string(out.stdout)?)?;
    t.set("stderr", lua.create_string(out.stderr)?)?;
    Ok(t)
}

pub fn parse_argv(value: Value) -> mlua::Result<Vec<String>> {
    match value {
        Value::Table(t) => {
            let argv: Vec<String> = t.sequence_values::<String>().collect::<mlua::Result<_>>()?;
            if argv.is_empty() {
                return Err(mlua::Error::external("sh: argument list is empty"));
            }
            Ok(argv)
        }
        Value::String(_) => Err(mlua::Error::external(
            "sh: a command is a list of arguments, e.g. { \"cargo\", \"build\" } \
             (for a shell, run it explicitly: { \"sh\", \"-c\", \"...\" })",
        )),
        other => Err(mlua::Error::external(format!(
            "sh: command must be a list of strings, got {}",
            other.type_name()
        ))),
    }
}

fn parse_opts(opts: Option<Table>) -> mlua::Result<Opts> {
    let mut out = Opts::default();
    if let Some(t) = opts {
        out.stdin = t
            .get::<Option<mlua::String>>("stdin")?
            .map(|s| s.as_bytes().to_vec());
        out.cwd = t.get("cwd")?;
        if let Some(env) = t.get::<Option<Table>>("env")? {
            for pair in env.pairs::<String, String>() {
                out.env.push(pair?);
            }
            out.env.sort();
        }
    }
    Ok(out)
}

pub fn register(lua: &Lua, shell: Arc<dyn Shell>) -> mlua::Result<()> {
    let sh = lua.create_table()?;

    let s = shell.clone();
    sh.set(
        "run",
        lua.create_function(move |_, (cmd, opts): (Value, Option<Table>)| {
            s.run(&parse_argv(cmd)?, &parse_opts(opts)?)
        })?,
    )?;

    sh.set(
        "capture",
        lua.create_function(move |lua, (cmd, opts): (Value, Option<Table>)| {
            capture_table(lua, shell.capture(&parse_argv(cmd)?, &parse_opts(opts)?)?)
        })?,
    )?;

    lua.globals().set("sh", sh)?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use mlua::Lua;

    fn lua() -> Lua {
        let lua = Lua::new();
        register(&lua, Arc::new(Local)).unwrap();
        lua
    }

    fn run(src: &str) {
        lua().load(src).exec().unwrap();
    }

    #[test]
    fn argv_is_not_run_through_a_shell() {
        run(r#"
            local r = sh.capture({ "printf", "%s", "$HOME" })
            assert(r.code == 0, "exit " .. r.code)
            assert(r.stdout == "$HOME", "got: " .. r.stdout)
        "#);
    }

    #[test]
    fn an_explicit_shell_still_works_as_an_escape_hatch() {
        run(r#"
            local r = sh.capture({ "sh", "-c", 'printf %s "$FOO"' }, { env = { FOO = "bar" } })
            assert(r.stdout == "bar", "got: " .. r.stdout)
        "#);
    }

    #[test]
    fn stdin_is_fed_to_the_command() {
        run(r#"
            local r = sh.capture({ "cat" }, { stdin = "hello\nworld" })
            assert(r.stdout == "hello\nworld", "got: " .. r.stdout)
        "#);
    }

    #[test]
    fn env_and_cwd_apply() {
        run(r#"
            local r = sh.capture({ "printenv", "TAKU_TEST" }, { env = { TAKU_TEST = "xyz" } })
            assert(r.code == 0 and r.stdout == "xyz\n", "env got: " .. r.stdout)
            local p = sh.capture({ "pwd" }, { cwd = "/" })
            assert(p.stdout == "/\n", "cwd got: " .. p.stdout)
        "#);
    }

    #[test]
    fn nonzero_exit_is_reported_not_raised() {
        run(r#"
            local r = sh.capture({ "false" })
            assert(r.code ~= 0, "false should exit non-zero")
        "#);
    }

    #[test]
    fn a_string_command_is_rejected_with_a_hint() {
        let err = lua().load(r#"sh.run("cargo build")"#).exec().unwrap_err();
        assert!(err.to_string().contains("list of arguments"));
    }

    #[test]
    fn empty_argv_is_rejected() {
        let err = lua().load("sh.run({})").exec().unwrap_err();
        assert!(err.to_string().contains("argument list is empty"));
    }
}
