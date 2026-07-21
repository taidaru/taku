use std::collections::HashMap;
use std::fmt::Display;
use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use mlua::{Lua, Table, Value};
use taku_api::steps::{Arg, StepCtx, StepDef};

#[derive(Default)]
pub struct Opts {
    pub stdin: Option<Vec<u8>>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    /// Kill the child and error once this elapses.
    pub timeout: Option<Duration>,
}

pub struct Capture {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn command(argv: &[String], opts: &Opts) -> Command {
    // Callers must pass a non-empty argv (enforced at the Lua boundary by
    // `parse_argv`).
    debug_assert!(!argv.is_empty(), "argv must be non-empty");
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    if let Some(cwd) = &opts.cwd {
        command.current_dir(cwd);
    }
    command.envs(opts.env.iter().map(|(k, v)| (k, v)));
    command
}

fn err<E: Display>(op: &str, argv: &[String], e: E) -> mlua::Error {
    mlua::Error::external(format!("cmd.{op}({}): {e}", argv.join(" ")))
}

fn feed(stdin: &mut ChildStdin, data: &[u8]) -> io::Result<()> {
    match stdin.write_all(data).and_then(|()| stdin.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        res => res,
    }
}

/// Feeds `data` to a piped child stdin from its own thread. Writing stdin to
/// completion before reading any output would deadlock as soon as the child
/// fills its stdout pipe, so the feed and the wait must proceed concurrently.
fn spawn_feeder<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    stdin: Option<ChildStdin>,
    data: Option<&'scope [u8]>,
) -> Option<std::thread::ScopedJoinHandle<'scope, io::Result<()>>> {
    let (mut stdin, data) = stdin.zip(data)?;
    Some(scope.spawn(move || {
        let res = feed(&mut stdin, data);
        drop(stdin); // close the pipe so the child sees EOF
        res
    }))
}

fn finish<T>(
    out: io::Result<T>,
    feeder: Option<std::thread::ScopedJoinHandle<'_, io::Result<()>>>,
) -> io::Result<T> {
    let fed = feeder.map_or(Ok(()), |f| f.join().expect("stdin feeder thread panicked"));
    out.and_then(|v| fed.map(|()| v))
}

pub fn wait_with_input(mut child: Child, data: Option<&[u8]>) -> io::Result<Output> {
    let stdin = child.stdin.take();
    std::thread::scope(|scope| {
        let feeder = spawn_feeder(scope, stdin, data);
        finish(child.wait_with_output(), feeder)
    })
}

pub fn wait_status_with_input(mut child: Child, data: Option<&[u8]>) -> io::Result<ExitStatus> {
    let stdin = child.stdin.take();
    std::thread::scope(|scope| {
        let feeder = spawn_feeder(scope, stdin, data);
        finish(child.wait(), feeder)
    })
}

/// full pipe can't block the wait.
fn spawn_reader<'scope, R: Read + Send + 'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    reader: Option<R>,
) -> Option<std::thread::ScopedJoinHandle<'scope, io::Result<Vec<u8>>>> {
    let mut reader = reader?;
    Some(scope.spawn(move || {
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        Ok(buf)
    }))
}

/// TODO!
fn wait_deadline(
    child: &mut Child,
    deadline: Instant,
    timeout: Duration,
) -> io::Result<ExitStatus> {
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("timed out after {}s", timeout.as_secs_f64()),
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_status_with_timeout(
    mut child: Child,
    timeout: Duration,
    data: Option<&[u8]>,
) -> io::Result<ExitStatus> {
    let stdin = child.stdin.take();
    let deadline = Instant::now() + timeout;
    std::thread::scope(|scope| {
        let feeder = spawn_feeder(scope, stdin, data);
        finish(wait_deadline(&mut child, deadline, timeout), feeder)
    })
}

fn wait_output_with_timeout(
    mut child: Child,
    timeout: Duration,
    data: Option<&[u8]>,
) -> io::Result<Output> {
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let deadline = Instant::now() + timeout;
    std::thread::scope(|scope| {
        let feeder = spawn_feeder(scope, stdin, data);
        let out_h = spawn_reader(scope, stdout);
        let err_h = spawn_reader(scope, stderr);
        let status = wait_deadline(&mut child, deadline, timeout)?;
        let stdout = out_h.map_or(Ok(Vec::new()), |h| {
            h.join().expect("stdout reader panicked")
        })?;
        let stderr = err_h.map_or(Ok(Vec::new()), |h| {
            h.join().expect("stderr reader panicked")
        })?;
        finish(Ok(()), feeder)?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    })
}

pub fn run(argv: &[String], opts: &Opts) -> mlua::Result<i32> {
    // `cmd.run` takes a literal argv, so echoing it back is safe.
    run_ctx(argv, opts, &format!("cmd.run({})", argv.join(" ")))
}

/// Steps resolve their argv from `${...}` placeholders that may carry secrets, so
/// they pass a redacted label here rather than leaking the resolved command.
fn run_ctx(argv: &[String], opts: &Opts, ctx: &str) -> mlua::Result<i32> {
    let mut command = command(argv, opts);
    if opts.stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mkerr = |e: io::Error| mlua::Error::external(format!("{ctx}: {e}"));
    let child = command.spawn().map_err(mkerr)?;
    let status = match opts.timeout {
        Some(timeout) => wait_status_with_timeout(child, timeout, opts.stdin.as_deref()),
        None => wait_status_with_input(child, opts.stdin.as_deref()),
    }
    .map_err(mkerr)?;
    Ok(status.code().unwrap_or(-1))
}

pub fn capture(argv: &[String], opts: &Opts) -> mlua::Result<Capture> {
    let mut command = command(argv, opts);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command.stdin(if opts.stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let child = command.spawn().map_err(|e| err("capture", argv, e))?;
    let out = match opts.timeout {
        Some(timeout) => wait_output_with_timeout(child, timeout, opts.stdin.as_deref()),
        None => wait_with_input(child, opts.stdin.as_deref()),
    }
    .map_err(|e| err("capture", argv, e))?;
    Ok(Capture {
        code: out.status.code().unwrap_or(-1),
        stdout: out.stdout,
        stderr: out.stderr,
    })
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
                return Err(mlua::Error::external("cmd: argument list is empty"));
            }
            Ok(argv)
        }
        Value::String(_) => Err(mlua::Error::external(
            "cmd: a command is a list of arguments, e.g. { \"cargo\", \"build\" } \
             (for a shell, run it explicitly: { \"sh\", \"-c\", \"...\" })",
        )),
        other => Err(mlua::Error::external(format!(
            "cmd: command must be a list of strings, got {}",
            other.type_name()
        ))),
    }
}

fn parse_timeout(secs: Option<f64>) -> mlua::Result<Option<Duration>> {
    match secs {
        None => Ok(None),
        // Cap at 1e15 s so `from_secs_f64` can't overflow and
        // panic; the range also rejects NaN and infinity.
        Some(s) if !(0.0..=1e15).contains(&s) => Err(mlua::Error::external(format!(
            "timeout must be a non-negative number of seconds, got {s}"
        ))),
        Some(s) => Ok(Some(Duration::from_secs_f64(s))),
    }
}

fn parse_opts(dotenv: &HashMap<String, String>, opts: Option<Table>) -> mlua::Result<Opts> {
    // Same precedence as the cmd/argv/pipe steps (`step_opts`): inherited env
    // < `.env` (fills unset only) < explicit `env=`.
    let mut env: Vec<(String, String)> = dotenv
        .iter()
        .filter(|(k, _)| std::env::var_os(k).is_none())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.sort();
    let mut out = Opts {
        env,
        ..Opts::default()
    };
    if let Some(t) = opts {
        out.stdin = t
            .get::<Option<mlua::String>>("stdin")?
            .map(|s| s.as_bytes().to_vec());
        out.cwd = t.get("cwd")?;
        if let Some(step_env) = t.get::<Option<Table>>("env")? {
            let mut extra = Vec::new();
            for pair in step_env.pairs::<String, String>() {
                extra.push(pair?);
            }
            // sort, so the env is deterministic; extend so explicit env wins.
            extra.sort();
            out.env.extend(extra);
        }
        out.timeout = parse_timeout(t.get::<Option<f64>>("timeout")?)?;
    }
    Ok(out)
}

/// Options shared by the `cmd`/`argv`/`pipe` steps: `cwd`, `env = {...}`,
/// `allow_fail`, `timeout` (seconds)
fn step_opts(t: Option<&Table>, ctx: &StepCtx) -> mlua::Result<(Opts, bool)> {
    // The child inherits the process env; `.env` only fills what the real
    // environment leaves unset (same precedence as `env.get`), so a `.env`
    // entry never silently overrides an inherited var like PATH.
    let mut env: Vec<(String, String)> = ctx
        .dotenv
        .iter()
        .filter(|(k, _)| std::env::var_os(k).is_none())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.sort();
    let mut opts = Opts {
        env,
        ..Opts::default()
    };
    let mut allow_fail = false;
    if let Some(t) = t {
        if let Some(cwd) = t.get::<Option<String>>("cwd")? {
            opts.cwd = Some(ctx.fmt(&cwd)?);
        }
        if let Some(step_env) = t.get::<Option<Table>>("env")? {
            let mut extra = Vec::new();
            for pair in step_env.pairs::<String, Value>() {
                let (k, v) = pair?;
                extra.push((k, ctx.fmt_value(v)?));
            }
            extra.sort();
            opts.env.extend(extra);
        }
        allow_fail = t.get::<Option<bool>>("allow_fail")?.unwrap_or(false);
        opts.timeout = parse_timeout(t.get::<Option<f64>>("timeout")?)?;
    }
    Ok((opts, allow_fail))
}

fn tokenize(line: &str, template: &str) -> mlua::Result<Vec<String>> {
    let argv = shlex::split(line).ok_or_else(|| {
        mlua::Error::external(format!("unbalanced quotes in command: {template}"))
    })?;
    if argv.is_empty() {
        return Err(mlua::Error::external(format!("empty command: {template}")));
    }
    Ok(argv)
}

/// format the template, tokenize, run. A non-zero exit fails the step unless
/// `allow_fail`; the error names the template, never the resolved argv —
/// resolved env values may hold secrets.
fn cmd_step(t: &Table, ctx: &mut StepCtx) -> mlua::Result<()> {
    let template: String = t.get(1)?;
    let (opts, allow_fail) = step_opts(Some(t), ctx)?;
    let argv = tokenize(&ctx.fmt(&template)?, &template)?;
    let code = run_ctx(&argv, &opts, &format!("command: {template}"))?;
    if code != 0 && !allow_fail {
        return Err(mlua::Error::external(format!(
            "command failed (exit {code}): {template}"
        )));
    }
    Ok(())
}

/// `argv{ "prog", "arg with spaces", ... }` — elements are formatted
/// individually and never re-tokenized.
fn argv_step(t: &Table, ctx: &mut StepCtx) -> mlua::Result<()> {
    let (opts, allow_fail) = step_opts(Some(t), ctx)?;
    let mut argv = Vec::new();
    for v in t.sequence_values::<Value>() {
        argv.push(ctx.fmt_value(v?)?);
    }
    if argv.is_empty() {
        return Err(mlua::Error::external("argv{}: empty argument list"));
    }
    let code = run_ctx(&argv, &opts, "argv command")?;
    if code != 0 && !allow_fail {
        return Err(mlua::Error::external(format!(
            "command failed (exit {code})"
        )));
    }
    Ok(())
}

/// Kills and reaps any still-running children when the pipeline errors early
/// (a mid-spawn failure or a timeout) — `Child`'s own Drop is a no-op on Unix,
/// so without this an unfinished stage would be orphaned for the process life.
struct KillOnDrop<'a>(Vec<(&'a String, Child)>);

impl Drop for KillOnDrop<'_> {
    fn drop(&mut self) {
        for (_, child) in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `pipe{ "cmd1", "cmd2", ... }` — a Rust-managed pipeline; pipefail always.
fn pipe_step(t: &Table, ctx: &mut StepCtx) -> mlua::Result<()> {
    let (opts, allow_fail) = step_opts(Some(t), ctx)?;
    let mut templates = Vec::new();
    for v in t.sequence_values::<String>() {
        templates.push(v?);
    }
    if templates.is_empty() {
        return Err(mlua::Error::external("pipe{}: no commands"));
    }

    let mut children = KillOnDrop(Vec::new());
    let mut prev_stdout = None;
    for (i, template) in templates.iter().enumerate() {
        let argv = tokenize(&ctx.fmt(template)?, template)?;
        let mut cmd = command(&argv, &opts);
        if let Some(out) = prev_stdout.take() {
            cmd.stdin(Stdio::from(out));
        }
        if i + 1 < templates.len() {
            cmd.stdout(Stdio::piped());
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| mlua::Error::external(format!("{template}: {e}")))?;
        prev_stdout = child.stdout.take();
        children.0.push((template, child));
    }

    // One deadline for the whole pipeline (not per stage).
    let deadline = opts.timeout.map(|t| Instant::now() + t);
    let mut failed = None;
    for (template, child) in &mut children.0 {
        let status = match (deadline, opts.timeout) {
            (Some(deadline), Some(timeout)) => wait_deadline(child, deadline, timeout),
            _ => child.wait(),
        }
        .map_err(|e| mlua::Error::external(format!("{template}: {e}")))?;
        if !status.success() && failed.is_none() {
            failed = Some(format!(
                "pipeline command failed (exit {}): {template}",
                status.code().unwrap_or(-1)
            ));
        }
    }
    match failed {
        Some(msg) if !allow_fail => Err(mlua::Error::external(msg)),
        _ => Ok(()),
    }
}

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    globals: &["cmd"],
    register: |lua, ctx| register(lua, ctx.dotenv.clone()),
    steps: &[
        StepDef {
            tag: "cmd",
            arg: Arg::Hidden,
            run: |_, t, ctx| cmd_step(t, ctx),
        },
        StepDef {
            tag: "argv",
            arg: Arg::Table,
            run: |_, t, ctx| argv_step(t, ctx),
        },
        StepDef {
            tag: "pipe",
            arg: Arg::Table,
            run: |_, t, ctx| pipe_step(t, ctx),
        },
    ],
};

pub fn register(lua: &Lua, dotenv: Arc<HashMap<String, String>>) -> mlua::Result<()> {
    let (d_run, d_try, d_capture) = (dotenv.clone(), dotenv.clone(), dotenv);
    taku_api::lua_api!(lua, global = "cmd" {
        // cmd.run: success or error — a non-zero exit raises.
        run => move |_, (cmd, opts): (Value, Option<Table>)| {
            taku_api::require_runtime("cmd.run")?;
            let argv = parse_argv(cmd)?;
            let code = run(&argv, &parse_opts(&d_run, opts)?)?;
            if code != 0 {
                return Err(mlua::Error::external(format!(
                    "cmd.run({}): exit {code}",
                    argv.join(" ")
                )));
            }
            Ok(())
        },
        // cmd.try: like run, but the exit code is the caller's problem.
        try => move |_, (cmd, opts): (Value, Option<Table>)| {
            taku_api::require_runtime("cmd.try")?;
            run(&parse_argv(cmd)?, &parse_opts(&d_try, opts)?)
        },
        capture => move |lua, (cmd, opts): (Value, Option<Table>)| {
            taku_api::require_runtime("cmd.capture")?;
            capture_table(lua, capture(&parse_argv(cmd)?, &parse_opts(&d_capture, opts)?)?)
        },
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use mlua::Lua;

    fn lua() -> Lua {
        // Tests exercise the API as a task body would: runtime phase on.
        taku_api::set_runtime(true);
        let lua = Lua::new();
        register(&lua, Arc::new(HashMap::new())).unwrap();
        lua
    }

    #[test]
    fn effects_are_rejected_at_load_phase() {
        let lua = lua();
        taku_api::set_runtime(false);
        let err = lua.load(r#"cmd.run({ "true" })"#).exec().unwrap_err();
        taku_api::set_runtime(true);
        assert!(
            err.to_string()
                .contains("only available while a task is running")
        );
    }

    fn run(src: &str) {
        lua().load(src).exec().unwrap();
    }

    #[test]
    fn argv_is_not_run_through_a_shell() {
        run(r#"
            local r = cmd.capture({ "printf", "%s", "$HOME" })
            assert(r.code == 0, "exit " .. r.code)
            assert(r.stdout == "$HOME", "got: " .. r.stdout)
        "#);
    }

    #[test]
    fn an_explicit_shell_still_works_as_an_escape_hatch() {
        run(r#"
            local r = cmd.capture({ "sh", "-c", 'printf %s "$FOO"' }, { env = { FOO = "bar" } })
            assert(r.stdout == "bar", "got: " .. r.stdout)
        "#);
    }

    #[test]
    fn stdin_is_fed_to_the_command() {
        run(r#"
            local r = cmd.capture({ "cat" }, { stdin = "hello\nworld" })
            assert(r.stdout == "hello\nworld", "got: " .. r.stdout)
        "#);
    }

    #[test]
    fn env_and_cwd_apply() {
        run(r#"
            local r = cmd.capture({ "printenv", "TAKU_TEST" }, { env = { TAKU_TEST = "xyz" } })
            assert(r.code == 0 and r.stdout == "xyz\n", "env got: " .. r.stdout)
            local p = cmd.capture({ "pwd" }, { cwd = "/" })
            assert(p.stdout == "/\n", "cwd got: " .. p.stdout)
        "#);
    }

    #[test]
    fn run_raises_on_nonzero_exit() {
        run(r#"cmd.run({ "true" })"#);
        let err = lua().load(r#"cmd.run({ "false" })"#).exec().unwrap_err();
        assert!(err.to_string().contains("exit 1"), "got: {err}");
    }

    #[test]
    fn try_returns_the_exit_code() {
        run(r#"
            assert(cmd.try({ "true" }) == 0)
            assert(cmd.try({ "false" }) == 1)
        "#);
    }

    #[test]
    fn nonzero_exit_is_reported_not_raised_by_capture() {
        run(r#"
            local r = cmd.capture({ "false" })
            assert(r.code ~= 0, "false should exit non-zero")
        "#);
    }

    #[test]
    fn large_stdin_does_not_deadlock_capture() {
        run(r#"
            local big = string.rep("x", 1024 * 1024)
            local r = cmd.capture({ "cat" }, { stdin = big })
            assert(r.code == 0, "exit " .. r.code)
            assert(#r.stdout == #big, "got " .. #r.stdout .. " bytes")
        "#);
    }

    #[test]
    fn child_ignoring_stdin_is_not_an_error() {
        run(r#"
            local big = string.rep("x", 1024 * 1024)
            local r = cmd.capture({ "true" }, { stdin = big })
            assert(r.code == 0, "exit " .. r.code)
        "#);
    }

    #[test]
    fn timeout_kills_a_slow_command() {
        let start = std::time::Instant::now();
        let err = lua()
            .load(r#"cmd.run({ "sleep", "30" }, { timeout = 0.2 })"#)
            .exec()
            .unwrap_err();
        assert!(err.to_string().contains("timed out"), "got: {err}");
        assert!(
            start.elapsed().as_secs() < 5,
            "did not stop near the deadline"
        );
    }

    #[test]
    fn stdin_and_timeout_together_do_not_deadlock() {
        run(r#"
            local r = cmd.capture({ "cat" }, { stdin = "hi", timeout = 5 })
            assert(r.code == 0, "exit " .. r.code)
            assert(r.stdout == "hi", "got: " .. r.stdout)
        "#);
    }

    #[test]
    fn timeout_rejects_negative() {
        let err = lua()
            .load(r#"cmd.try({ "true" }, { timeout = -1 })"#)
            .exec()
            .unwrap_err();
        assert!(err.to_string().contains("non-negative"), "got: {err}");
    }

    #[test]
    fn a_string_command_is_rejected_with_a_hint() {
        let err = lua().load(r#"cmd.run("cargo build")"#).exec().unwrap_err();
        assert!(err.to_string().contains("list of arguments"));
    }

    #[test]
    fn empty_argv_is_rejected() {
        let err = lua().load("cmd.run({})").exec().unwrap_err();
        assert!(err.to_string().contains("argument list is empty"));
    }

    #[test]
    fn dotenv_fills_unset_child_env_for_module_calls() {
        taku_api::set_runtime(true);
        let lua = Lua::new();
        let mut dotenv = HashMap::new();
        dotenv.insert("TAKU_DOTENV_ONLY".to_string(), "from-dotenv".to_string());
        register(&lua, Arc::new(dotenv)).unwrap();
        lua.load(
            r#"
            local r = cmd.capture({ "printenv", "TAKU_DOTENV_ONLY" })
            assert(r.code == 0, "exit " .. r.code)
            assert(r.stdout == "from-dotenv\n", "got: " .. r.stdout)
        "#,
        )
        .exec()
        .unwrap();
    }
}
