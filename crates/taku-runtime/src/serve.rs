//! `serve`: long-lived service processes, allowed only as a task's last step.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::{Table, Value};

use crate::error::Error;
use crate::exec::Ctx;

/// The run-scoped registry of started services.
pub(crate) type Services = Arc<Mutex<Vec<Service>>>;

pub(crate) struct Service {
    name: String,
    child: Child,
}

pub(crate) fn kill_all(services: &Services) {
    for mut service in services.lock().unwrap().drain(..) {
        let _ = service.child.kill();
        let _ = service.child.wait();
    }
}

pub(crate) fn any_running(services: &Services) -> bool {
    !services.lock().unwrap().is_empty()
}

pub(crate) fn reap_failure(services: &Services) -> Option<String> {
    let mut services = services.lock().unwrap();
    let mut failure = None;
    services.retain_mut(|service| match service.child.try_wait() {
        Ok(Some(status)) if status.success() => {
            println!("taku: service '{}' exited", service.name);
            false
        }
        Ok(Some(status)) => {
            failure = Some(format!("service '{}' {status}", service.name));
            false
        }
        _ => true,
    });
    failure
}

/// `serve { "cmd", ... }` — one service named after the task;
/// `serve { api = {...}, web = {...} }` — several, by key.
pub(crate) fn run(spec: &Table, t: &Table, ctx: &mut Ctx) -> Result<(), Error> {
    if !t.get::<Value>(1)?.is_nil() {
        let task: String = spec.get("name")?;
        return start(&task, t, ctx);
    }
    let mut entries: Vec<(String, Table)> = Vec::new();
    for pair in t.pairs::<Value, Value>() {
        let (k, v) = pair?;
        if let (Value::String(k), Value::Table(svc)) = (&k, v) {
            let key = k.to_string_lossy().to_string();
            if key != taku_api::steps::TAG {
                entries.push((key, svc));
            }
        }
    }
    if entries.is_empty() {
        return Err(Error::TaskFailed(
            "serve: expected a command string or named service tables".to_string(),
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, svc) in entries {
        start(&name, &svc, ctx)?;
    }
    Ok(())
}

fn start(name: &str, t: &Table, ctx: &mut Ctx) -> Result<(), Error> {
    let template: String = t
        .get::<Option<String>>(1)?
        .ok_or_else(|| Error::TaskFailed(format!("serve '{name}': missing command string")))?;
    let command = ctx.format(&template)?;
    let argv = shlex::split(&command)
        .filter(|a| !a.is_empty())
        .ok_or_else(|| Error::TaskFailed(format!("serve '{name}': bad command: {template}")))?;

    // same semantics as command steps: relative to the process cwd, as-is
    let cwd = match t.get::<Option<String>>("cwd")? {
        Some(c) => Some(PathBuf::from(ctx.format(&c)?)),
        None => None,
    };
    // same precedence as command steps: inherited env < .env < explicit env=
    let mut envs: Vec<(String, String)> = ctx
        .dotenv
        .iter()
        .filter(|(k, _)| std::env::var_os(k).is_none())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    envs.sort();
    if let Some(extra) = t.get::<Option<Table>>("env")? {
        for pair in extra.pairs::<String, String>() {
            let (k, v) = pair?;
            envs.push((k, ctx.format(&v)?));
        }
    }

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    if let Some(cwd) = &cwd {
        cmd.current_dir(cwd);
    }
    cmd.envs(envs.iter().map(|(k, v)| (k, v)));
    let child = cmd
        .spawn()
        .map_err(|e| Error::TaskFailed(format!("serve '{name}': {}: {e}", argv[0])))?;
    println!("taku: service '{name}' started (pid {})", child.id());
    ctx.services.lock().unwrap().push(Service {
        name: name.to_string(),
        child,
    });

    wait_ready(name, t.get::<Option<Table>>("ready")?, &ctx.services)
}

/// `ready = { timeout = secs }` waits that long;
/// `http = "http://host:port/path"` polls until the endpoint answers 2xx
/// (any other status just isn't ready yet), with `timeout` as the cap
/// (default 30s). No `ready` at all: ready immediately.
fn wait_ready(name: &str, ready: Option<Table>, services: &Services) -> Result<(), Error> {
    let Some(ready) = ready else { return Ok(()) };
    let timeout = ready.get::<Option<f64>>("timeout")?;
    let http: Option<String> = ready.get("http")?;

    let died = |services: &Services| -> Option<ExitStatus> {
        let mut services = services.lock().unwrap();
        let service = services.iter_mut().find(|s| s.name == name)?;
        let status = service.child.try_wait().ok().flatten()?;
        services.retain(|s| s.name != name);
        Some(status)
    };
    let fail = |status: ExitStatus| {
        Err(Error::TaskFailed(format!(
            "serve '{name}': {status} before becoming ready"
        )))
    };

    let Some(url) = http else {
        let secs = timeout.ok_or_else(|| {
            Error::TaskFailed(format!("serve '{name}': ready needs timeout or http"))
        })?;
        let deadline = Instant::now() + Duration::from_secs_f64(secs);
        loop {
            if let Some(status) = died(services) {
                return fail(status);
            }
            if Instant::now() >= deadline {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    let deadline = Instant::now() + Duration::from_secs_f64(timeout.unwrap_or(30.0));
    loop {
        if let Some(status) = died(services) {
            return fail(status);
        }
        if http_ready(&url) {
            println!("taku: service '{name}' ready");
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::TaskFailed(format!(
                "serve '{name}': not ready before the timeout"
            )));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Plain HTTP/1.0 GET over a TCP stream; ready means a 2xx status. Anything
/// else — connection refused, 5xx from a warming-up server — is "not ready
/// yet", never an error; the poll just continues until the timeout.
fn http_ready(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let addr = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    let Ok(mut stream) = TcpStream::connect(&addr) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    if stream
        .write_all(format!("GET /{path} HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes())
        .is_err()
    {
        return false;
    }
    // status line: "HTTP/1.x NNN ..."
    let mut buf = [0u8; 32];
    let Ok(n) = stream.read(&mut buf) else {
        return false;
    };
    String::from_utf8_lossy(&buf[..n])
        .split_whitespace()
        .nth(1)
        .is_some_and(|code| code.starts_with('2'))
}
