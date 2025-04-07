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
    Init {
        name: String,
        os: String,
        e: String,
        tmp: String,
    },
    Build {
        name: String,
        archive: String,
    },
    Run,
}

fn main() -> Result<(), Error> {
    let lys = Lys::parse();
    match lys.command {
        Command::Init { name, os, e, tmp } => {
            init(os.as_str(), name.as_str(), e.eq(&"yes"), tmp.eq(&"yes"))
        }
        Command::Build { name, archive } => build(name.as_str(), archive.as_str()),
        Command::Run => run(),
    }
}
