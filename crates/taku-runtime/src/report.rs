use std::io::IsTerminal;
use std::time::Duration;

/// Per-task prefix colours, reused cyclically when tasks outnumber the palette:
/// cyan, yellow, magenta, blue, green, red.
pub(crate) const PALETTE: [u8; 6] = [36, 33, 35, 34, 32, 31];

#[derive(Clone, Copy)]
pub(crate) struct Style {
    color: bool,
}

impl Style {
    pub(crate) fn init() -> Self {
        // any non-empty NO_COLOR disables colour.
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Style {
            color: !no_color && std::io::stderr().is_terminal(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(color: bool) -> Self {
        Style { color }
    }

    /// Whether task-output prefixes on **stdout** should be coloured (command
    /// output goes to stdout, unlike the markers on stderr).
    pub(crate) fn stdout_color() -> bool {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        !no_color && std::io::stdout().is_terminal()
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// Paint with an explicit SGR parameter string, e.g. `"1;34"` for bold blue.
    /// Lets the diagnostic renderer combine bold with a colour without a method
    /// per combination.
    pub(crate) fn sgr(&self, code: &str, s: &str) -> String {
        self.paint(code, s)
    }

    pub(crate) fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    pub(crate) fn green(&self, s: &str) -> String {
        self.paint("32", s)
    }
    pub(crate) fn red(&self, s: &str) -> String {
        self.paint("31", s)
    }
}

use crate::exec::Outcome;

/// A finished task: `✓ build 5ms`, or `- build skipped 0ms` when an `unchanged`
/// guard short-circuited it. The time is dim; nothing carries a task colour.
/// Under `json`, emits one `{"event":"task",...}` line instead.
pub(crate) fn task_done(
    style: &Style,
    name: &str,
    outcome: Outcome,
    elapsed: Duration,
    json: bool,
    quiet: bool,
) {
    if quiet {
        return;
    }
    if json {
        let status = match outcome {
            Outcome::Ran => "ran",
            Outcome::Skipped => "skipped",
        };
        return task_event(name, status, elapsed);
    }
    let dur = style.dim(&format_duration(elapsed));
    match outcome {
        Outcome::Ran => eprintln!("{} {name} {dur}", style.green("✓")),
        Outcome::Skipped => eprintln!("- {name} skipped {dur}"),
    }
}

pub(crate) fn task_failed(style: &Style, name: &str, elapsed: Duration, json: bool, quiet: bool) {
    if quiet {
        return;
    }
    if json {
        return task_event(name, "failed", elapsed);
    }
    eprintln!(
        "{} {name} {}",
        style.red("✗"),
        style.dim(&format_duration(elapsed))
    );
}

/// The closing line, entirely green: `2 tasks done in 6ms`, set off by a blank
/// line from the task markers above it.
pub(crate) fn summary(style: &Style, tasks: usize, elapsed: Duration, json: bool, quiet: bool) {
    if quiet {
        return;
    }
    if json {
        println!(
            "{{\"event\":\"summary\",\"tasks\":{tasks},\"ms\":{}}}",
            elapsed.as_millis()
        );
        return;
    }
    eprintln!(
        "\n{}",
        style.green(&format!(
            "{tasks} task{} done in {}",
            plural(tasks),
            format_duration(elapsed)
        ))
    );
}

/// A service lifecycle line: `service 'api' started (pid 42)` in text, or a
/// `{"event":"service",...}` object under `--json`.
pub(crate) fn service_event(quiet: bool, json: bool, name: &str, status: &str, pid: Option<u32>) {
    if quiet {
        return;
    }
    if json {
        let pid = pid.map_or(String::new(), |p| format!(",\"pid\":{p}"));
        println!(
            "{{\"event\":\"service\",\"name\":{},\"status\":\"{status}\"{pid}}}",
            taku_api::steps::json_string(name)
        );
    } else {
        let pid = pid.map_or(String::new(), |p| format!(" (pid {p})"));
        println!("service '{name}' {status}{pid}");
    }
}

/// A bare informational line, or a `{"event":"info","message":...}` object.
pub(crate) fn info(quiet: bool, json: bool, message: &str) {
    if quiet {
        return;
    }
    if json {
        println!(
            "{{\"event\":\"info\",\"message\":{}}}",
            taku_api::steps::json_string(message)
        );
    } else {
        println!("{message}");
    }
}

fn task_event(name: &str, status: &str, elapsed: Duration) {
    println!(
        "{{\"event\":\"task\",\"name\":{},\"status\":\"{status}\",\"ms\":{}}}",
        taku_api::steps::json_string(name),
        elapsed.as_millis()
    );
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_switch_units_at_one_second() {
        assert_eq!(format_duration(Duration::from_millis(7)), "7ms");
        assert_eq!(format_duration(Duration::from_millis(950)), "950ms");
        assert_eq!(format_duration(Duration::from_millis(1500)), "1.5s");
    }

    #[test]
    fn plain_style_emits_no_escapes() {
        let plain = Style { color: false };
        assert_eq!(plain.green("ok"), "ok");
        assert!(!plain.red("boom").contains('\x1b'));
        assert_eq!(plain.sgr("1;31", "x"), "x");
    }
}
