//! `serve`: long-lived service processes, allowed only as a task's last step.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mlua::{Table, Value};
use taku_api::steps::{OutputSink, Stream};

use crate::error::Error;
use crate::exec::Ctx;

/// Streams a service's piped stdout/stderr under the task prefix from detached
/// readers that end when the child's pipes close (i.e. when it is killed).
fn stream_service(child: &mut Child, sink: &OutputSink) {
    fn reader<R: Read + Send + 'static>(reader: Option<R>, sink: OutputSink, stream: Stream) {
        let Some(reader) = reader else { return };
        let mut reader = BufReader::new(reader);
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        while matches!(buf.last(), Some(b'\n' | b'\r')) {
                            buf.pop();
                        }
                        sink.line(stream, &String::from_utf8_lossy(&buf));
                    }
                }
            }
        });
    }
    reader(child.stdout.take(), sink.clone(), Stream::Stdout);
    reader(child.stderr.take(), sink.clone(), Stream::Stderr);
}

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

/// Kill every running service in place (without draining) so an in-flight
/// `wait_ready` sees its child die on the next `try_wait` and returns fast.
/// The reaping happens later in `kill_all`.
pub(crate) fn kill_running(services: &Services) {
    for service in services.lock().unwrap().iter_mut() {
        let _ = service.child.kill();
    }
}

pub(crate) fn any_running(services: &Services) -> bool {
    !services.lock().unwrap().is_empty()
}

pub(crate) fn reap_failure(services: &Services, json: bool, quiet: bool) -> Option<String> {
    let mut services = services.lock().unwrap();
    let mut failure = None;
    services.retain_mut(|service| match service.child.try_wait() {
        Ok(Some(status)) if status.success() => {
            crate::report::service_event(quiet, json, &service.name, "exited", None);
            false
        }
        Ok(Some(status)) => {
            failure = Some(format!(
                "service '{}' {}",
                service.name,
                status_phrase(status)
            ));
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
        if has_service_key(t)? {
            return Err(Error::Task(Box::new(
                crate::diagnostic::Diagnostic::error(
                    "serve: 'api'/'web' cannot be combined with a command string",
                )
                .help("use either a command form or { api = {...}, web = {...} }, not both"),
            )));
        }
        let task: String = spec.get("name")?;
        spawn(&task, t, ctx)?;
        return wait_ready(
            &task,
            t.get::<Option<Table>>("ready")?,
            &ctx.services,
            ctx.json,
            ctx.quiet,
        );
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
    // Spawn every service first, then wait for readiness — so a service whose
    // health check depends on a sibling isn't blocked by that sibling not yet
    // being started.
    for (name, svc) in &entries {
        spawn(name, svc, ctx)?;
    }
    for (name, svc) in &entries {
        wait_ready(
            name,
            svc.get::<Option<Table>>("ready")?,
            &ctx.services,
            ctx.json,
            ctx.quiet,
        )?;
    }
    Ok(())
}

/// A table-valued key that isn't a command option — i.e. a named service — so
/// the command form and the `{ api = {...} }` form aren't mixed.
fn has_service_key(t: &Table) -> Result<bool, Error> {
    for pair in t.pairs::<Value, Value>() {
        let (k, v) = pair?;
        if let (Value::String(k), Value::Table(_)) = (&k, &v) {
            let key = k.to_string_lossy();
            if !matches!(key.as_ref(), "ready" | "cwd" | "env") && key != taku_api::steps::TAG {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn spawn(name: &str, t: &Table, ctx: &mut Ctx) -> Result<(), Error> {
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
    // A service outlives the step, so its output streams through detached
    // readers holding an owned sink labelled with the service name.
    let sink = ctx.output.as_ref().map(|s| OutputSink {
        label: name.to_string(),
        ..s.clone()
    });
    if let Some(sink) = &sink {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        for (k, v) in sink.color_env() {
            if !envs.iter().any(|(ek, _)| ek == *k) {
                cmd.env(k, v);
            }
        }
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::TaskFailed(format!("serve '{name}': {}: {e}", argv[0])))?;
    if let Some(sink) = sink {
        stream_service(&mut child, &sink);
    }
    crate::report::service_event(ctx.quiet, ctx.json, name, "started", Some(child.id()));
    ctx.services.lock().unwrap().push(Service {
        name: name.to_string(),
        child,
    });
    Ok(())
}

/// How a finished service exited, phrased for the diagnostic: `exited with
/// status 1` or `terminated by SIGINT`.
fn status_phrase(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exited with status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            let name = match sig {
                1 => "SIGHUP".to_string(),
                2 => "SIGINT".to_string(),
                3 => "SIGQUIT".to_string(),
                6 => "SIGABRT".to_string(),
                9 => "SIGKILL".to_string(),
                15 => "SIGTERM".to_string(),
                n => format!("signal {n}"),
            };
            return format!("terminated by {name}");
        }
    }
    "exited abnormally".to_string()
}

/// Rejects negative/NaN/overflowing input instead of letting `from_secs_f64` panic.
fn duration(name: &str, secs: f64) -> Result<Duration, Error> {
    // The range rejects NaN/infinity and caps below `from_secs_f64`'s overflow.
    if !(0.0..=1e15).contains(&secs) {
        return Err(Error::TaskFailed(format!(
            "serve '{name}': ready.timeout must be a non-negative number of seconds, got {secs}"
        )));
    }
    Ok(Duration::from_secs_f64(secs))
}

/// `ready = { timeout = secs }` waits that long;
/// `http = "http://host:port/path"` polls until the endpoint answers 2xx
/// (any other status just isn't ready yet), with `timeout` as the cap
/// (default 30s). No `ready` at all: ready immediately.
fn wait_ready(
    name: &str,
    ready: Option<Table>,
    services: &Services,
    json: bool,
    quiet: bool,
) -> Result<(), Error> {
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
            "service '{name}' {}",
            status_phrase(status)
        )))
    };

    let Some(url) = http else {
        let secs = timeout.ok_or_else(|| {
            Error::TaskFailed(format!("serve '{name}': ready needs timeout or http"))
        })?;
        let deadline = Instant::now() + duration(name, secs)?;
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

    let cap = timeout.unwrap_or(30.0);
    let deadline = Instant::now() + duration(name, cap)?;
    loop {
        if let Some(status) = died(services) {
            return fail(status);
        }
        // Bound each connect/read by the time left (capped so the loop keeps
        // re-checking died()/the deadline) — otherwise a filtered or slow host
        // could block far past the configured timeout.
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::TaskFailed(format!(
                "service '{name}' timed out after {cap}s waiting to become ready"
            )));
        }
        if http_ready(&url, remaining.min(Duration::from_secs(2))) {
            crate::report::service_event(quiet, json, name, "ready", None);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Plain HTTP/1.0 GET over a TCP stream; ready means a 2xx status. Anything
/// else — connection refused, 5xx from a warming-up server — is "not ready
/// yet", never an error; the poll just continues until the timeout.
fn http_ready(url: &str, timeout: Duration) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let addr = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    let Ok(mut addrs) = addr.to_socket_addrs() else {
        return false;
    };
    let Some(target) = addrs.next() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&target, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
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
