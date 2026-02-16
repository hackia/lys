use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "silexium", version, about = "Silexium API service")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Serve(ServeArgs),
    Key(KeyArgs),
    Ingest(IngestArgs),
}

#[derive(Parser, Debug)]
pub struct ServeArgs {
    #[arg(long, default_value = "0.0.0.0")]
    pub host: String,
    #[arg(long, default_value_t = 8080)]
    pub port: u16,
    #[arg(long)]
    pub db: Option<PathBuf>,
    #[arg(long)]
    pub server_key: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub struct IngestArgs {
    #[arg(long)]
    pub file: PathBuf,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

#[derive(Parser, Debug)]
pub struct KeyArgs {
    #[command(subcommand)]
    pub command: KeyCommand,
    #[arg(long)]
    pub db: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum KeyCommand {
    Add(KeyAddArgs),
}

#[derive(Parser, Debug)]
pub struct KeyAddArgs {
    #[arg(long)]
    pub role: String,
    #[arg(long)]
    pub key: PathBuf,
    #[arg(long)]
    pub key_id: Option<String>,
}

pub fn resolve_db_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    let data_home = xdg_data_home()?;
    Ok(data_home.join("silexium").join("silexium.db"))
}

pub fn resolve_server_key(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    if let Ok(val) = env::var("SILEXIUM_SERVER_KEY") {
        return Ok(PathBuf::from(val));
    }
    Err(anyhow!(
        "server key missing: pass --server-key or set SILEXIUM_SERVER_KEY"
    ))
}

fn xdg_data_home() -> Result<PathBuf> {
    if let Ok(val) = env::var("XDG_DATA_HOME") {
        if !val.is_empty() {
            return Ok(PathBuf::from(val));
        }
    }
    let home = env::var("HOME").context("HOME is not set")?;
    Ok(Path::new(&home).join(".local").join("share"))
}
