use crate::build::build;
use crate::init::init;
use crate::run::run;
use anyhow::Error;
use clap::{Parser, Subcommand};

pub mod build;
pub mod init;
pub mod run;
pub mod seal;
pub mod share;
#[derive(Parser)]
#[command(name = "lys")]
#[command(version, about = "Ethical, portable, living packages")]
struct Lys {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init { name: String, os: String, e: String },
    Build,
    Run,
}

fn main() -> Result<(), Error> {
    let lys = Lys::parse();
    match lys.command {
        Command::Init { name, os, e } => init(name.as_str(), os.as_str(), e.as_str()),
        Command::Build => build(),
        Command::Run => run(),
    }
}
