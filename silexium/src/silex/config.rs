use anyhow::{Context, Result, anyhow};
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
    #[doc = "Serve the API server"]
    Serve(ServeArgs),
    #[doc = "Manage server keys"]
    Key(KeyArgs),
    #[doc = "Ingest a release manifest"]
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
    Revoke(KeyRevokeArgs),
    Rotate(KeyRotateArgs),
    List(KeyListArgs),
    RevokeExpired,
}

#[derive(Parser, Debug)]
pub struct KeyAddArgs {
    #[arg(long)]
    pub role: String,
    #[arg(long)]
    pub key: PathBuf,
    #[arg(long)]
    pub key_id: Option<String>,
    #[arg(long)]
    pub expires_at: String,
}

#[derive(Parser, Debug)]
pub struct KeyRevokeArgs {
    #[arg(long)]
    pub key_id: String,
    #[arg(long)]
    pub revoked_at: Option<String>,
}

#[derive(Parser, Debug)]
pub struct KeyRotateArgs {
    #[arg(long)]
    pub role: String,
    #[arg(long)]
    pub old_key_id: String,
    #[arg(long)]
    pub new_key: PathBuf,
    #[arg(long)]
    pub new_key_id: Option<String>,
    #[arg(long)]
    pub expires_at: String,
}

#[derive(Parser, Debug)]
pub struct KeyListArgs {
    #[arg(long)]
    pub json: bool,
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
