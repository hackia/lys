use crate::build::IMAGES_DIR;
use anyhow::{Error, anyhow};
use build::{build_img, run_command};
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
    Pull {
        os: String,
        user: String,
        hostname: String,
    },
    Push {
        os: String,
        user: String,
        hostname: String,
    },
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
        Command::Pull { os, user, hostname } => pull(os.as_str(), user.as_str(), hostname.as_str()),
        Command::Clone {} => todo!(),
        Command::Prune {} => todo!(),
        Command::Ps {} => todo!(),
        Command::List {} => todo!(),
        Command::Push { os, user, hostname } => push(os.as_str(), user.as_str(), hostname.as_str()),
    }
}

fn pull(os: &str, username: &str, hostname: &str) -> Result<(), Error> {
    if run_command(
        "rsync",
        &[
            "-a",
            "-v",
            "-z",
            "--delete",
            format!("{username}@{hostname}:{IMAGES_DIR}/{os}.img").as_str(),
            format!("{IMAGES_DIR}/{os}.img").as_str(),
        ],
    )? {
        return Ok(());
    }
    Err(anyhow!("failed to pull image"))
}

fn push(os: &str, username: &str, hostname: &str) -> Result<(), Error> {
    if run_command(
        "rsync",
        &[
            "-a",
            "-v",
            "-z",
            "--delete",
            format!("{IMAGES_DIR}/{os}.img").as_str(),
            format!("{username}@{hostname}:{IMAGES_DIR}/{os}.img").as_str(),
        ],
    )? {
        return Ok(());
    }
    Err(anyhow!("failed to pull image"))
}
