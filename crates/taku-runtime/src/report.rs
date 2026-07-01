use std::io::IsTerminal;
use std::time::Duration;

#[derive(Clone, Copy)]
pub(crate) struct Style {
    color: bool,
}

impl Style {
    pub(crate) fn init() -> Self {
        Style {
            color: std::io::stderr().is_terminal(),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(color: bool) -> Self {
        Style { color }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
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
    pub(crate) fn cyan(&self, s: &str) -> String {
        self.paint("36", s)
    }

    pub fn error(&self, message: &str) -> String {
        format!("{} {message}", self.red("taku: error:"))
    }
}

pub(crate) fn task_done(style: &Style, name: &str, elapsed: Duration) {
    eprintln!(
        "  {} {name} {}",
        style.green("✓"),
        style.dim(&format_duration(elapsed))
    );
}

pub(crate) fn task_failed(style: &Style, name: &str, elapsed: Duration) {
    eprintln!(
        "  {} {name} {}",
        style.red("✗"),
        style.dim(&format_duration(elapsed))
    );
}

pub(crate) fn summary(style: &Style, tasks: usize, elapsed: Duration) {
    eprintln!(
        "{}",
        style.green(&format!(
            "taku: {tasks} task{} in {}",
            plural(tasks),
            format_duration(elapsed)
        ))
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
        assert!(!plain.error("boom").contains('\x1b'));
    }
}
