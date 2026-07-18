//! Simple built-in operators

use std::io::Write as _;

use taku_api::steps::{Arg, StepDef};

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    globals: &[],
    register: |_, _| Ok(()),
    steps: &[
        StepDef {
            tag: "echo",
            arg: Arg::Str,
            run: |_, t, ctx| {
                println!("{}", ctx.fmt_value(t.get(1)?)?);
                Ok(())
            },
        },
        StepDef {
            tag: "confirm",
            arg: Arg::Str,
            run: |_, t, ctx| confirm(&ctx.fmt_value(t.get(1)?)?),
        },
    ],
};

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
