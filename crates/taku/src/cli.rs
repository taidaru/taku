use std::num::NonZeroUsize;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "taku", version, about, long_about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Emit all output as JSON (one object per line)
    #[arg(long, global = true)]
    pub json: bool,

    /// Print only errors — suppress warnings, progress markers, and command output
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init,
    #[command(visible_alias = "r")]
    Run {
        task: String,

        /// Maximum number of tasks to run in parallel (at least 1)
        #[arg(short, long, value_name = "N")]
        jobs: Option<NonZeroUsize>,

        /// Set a task parameter declared in its header (repeatable)
        #[arg(long, value_name = "KEY=VAL")]
        vars: Vec<String>,

        /// Answer yes to every `confirm` step
        #[arg(short, long)]
        yes: bool,

        /// Rebuild even when an `unchanged` guard says nothing changed
        #[arg(short, long)]
        force: bool,

        /// Print why an `unchanged` guard skipped or rebuilt
        #[arg(long)]
        explain: bool,

        /// Print the plan without executing it (templates stay unresolved)
        #[arg(short = 'n', long)]
        dry_run: bool,
    },
    #[command(visible_alias = "ls")]
    List,
}
