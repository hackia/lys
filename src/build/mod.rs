use crate::build::dependencies::check_dependencies;
use anyhow::{Error, anyhow};
use serde::Deserialize;
use std::fs::read_to_string;
use std::path::Path;

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

pub fn build() -> Result<(), Error> {
    if Path::new(LYS_CONF).exists() {
        let content = read_to_string(LYS_CONF)?;
        if let Ok(config) = toml::from_str::<Config>(content.as_str()) {
            if check_dependencies(
                config.package.name.to_string(),
                config.package.dependencies.to_vec(),
            )
            .is_ok()
            {
                return Ok(());
            }
            return Err(anyhow!(
                "failed to get {} dependencies",
                config.package.name.as_str()
            ));
        }
        return Err(anyhow!("failed to parse {LYS_CONF}"));
    }
    Err(anyhow!("{LYS_CONF} not found"))
}
