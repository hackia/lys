use anyhow::Error;
use build::build_img;
use clap::{Parser, Subcommand};
use init::init;
use run::run;

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
    Run {
        name: String,
    },
    Pull {},
    Push {},
    Clone {},
    Prune {},
    Ps {},
    List {},
}

fn main() -> Result<(), Error> {
    let lys = Lys::parse();
    match lys.command {
        Command::Init { name, os, e, tmp } => {
            let p: String = format!("/mnt/{name}");
            build_img(name.as_str())?;
            init(os.as_str(), p.as_str(), e.eq(&"yes"), tmp.eq(&"yes"))
        }
        Command::Run { name } => run(name.as_str()),
        Command::Pull {} => todo!(),
        Command::Clone {} => todo!(),
        Command::Prune {} => todo!(),
        Command::Ps {} => todo!(),
        Command::List {} => todo!(),
        Command::Push {} => todo!(),
    }
}
