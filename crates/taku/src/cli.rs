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

        #[arg(short, long, value_name = "N")]
        jobs: Option<usize>,
    },
    #[command(visible_alias = "ls")]
    List,
}
