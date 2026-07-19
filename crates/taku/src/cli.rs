use std::num::NonZeroUsize;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "taku", version, about, long_about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
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
    },
    #[command(visible_alias = "ls")]
    List,
}
