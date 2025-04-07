use std::process::Command;

use anyhow::{Error, Ok};
use serde::Deserialize;

pub const LYS_CONF: &str = "lys.toml";

pub mod dependencies;
#[derive(Debug, Deserialize)]
pub struct Config {
    pub package: Package,
}
#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: String,
    pub description: String,
    pub version: String,
    pub authors: Vec<String>,
    pub dependencies: Vec<String>,
}

pub fn build(name: &str, archive: &str) -> Result<(), Error> {
    if archive.starts_with("https") {
        Command::new("sudo")
            .arg("chroot")
            .arg(name)
            .arg("wget")
            .arg(archive)
            .current_dir(".")
            .spawn()?
            .wait()?;
        return Ok(());
    }
    Ok(())
}
