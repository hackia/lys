use crate::init::init;
use crate::run::run;
use clap::{Parser, Subcommand};
use crate::build::build;
use crate::seal::seal;
use crate::share::share;

pub mod init;
pub mod run;
pub mod build;
pub mod share;
pub mod seal;
#[derive(Parser)]
#[command(name = "lys")]
#[command(version, about = "Planpkg: ethical, portable, living packages")]
struct Lys {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init { name: String },
    Build,
    Run,
    Seal,
    Share,
}

fn main() {
    let lys = Lys::parse();

    match lys.command {
        Command::Init { name } => init(name.as_str()),
        Command::Build => build(),
        Command::Run => run(),
        Command::Seal => seal(),
        Command::Share => share(),
    }
}
