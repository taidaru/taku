//! Simple built-in operators

use std::io::{IsTerminal as _, Write as _};

use taku_api::steps::{Arg, StepCtx, StepDef, Stream};

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    globals: &[],
    register: |_, _| Ok(()),
    steps: &[
        StepDef::simple("echo", Arg::Str, |_, t, ctx| {
            emit(ctx, &ctx.fmt_value(t.get(1)?)?);
            Ok(())
        }),
        StepDef::simple("confirm", Arg::Str, |_, t, ctx| {
            let msg = ctx.fmt_value(t.get(1)?)?;
            // --yes, or no terminal to ask on (CI): auto-confirm
            if ctx.yes || !std::io::stdin().is_terminal() {
                emit(ctx, &format!("{msg} [y/N] yes (auto)"));
                return Ok(());
            }
            confirm(&msg)
        }),
    ],
};

/// Print an operator line through the task's output sink so it honours the
/// task prefix, `--json`, and `--quiet` like command output does. Falls back to
/// stdout when no sink is installed (tests, `capture`).
fn emit(ctx: &StepCtx, text: &str) {
    match ctx.output {
        Some(sink) => text
            .split('\n')
            .for_each(|line| sink.line(Stream::Stdout, line)),
        None => println!("{text}"),
    }
}

fn confirm(message: &str) -> mlua::Result<()> {
    print!("{message} [y/N] ");
    std::io::stdout().flush().ok();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| mlua::Error::external(format!("confirm: {e}")))?;
    if matches!(answer.trim(), "y" | "Y" | "yes") {
        Ok(())
    } else {
        Err(mlua::Error::external("cancelled"))
    }
}
