use crate::crypto::{sign_message, verify_signature};
use crate::utils::ok;
use chrono::Utc;
use clap::{Arg, ArgAction, Command};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, create_dir_all, read_to_string};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use uuid::Uuid;

#[path = "../crypto.rs"]
mod crypto;

pub mod vcs {
    pub enum FileStatus {
        New(std::path::PathBuf),
        Modified(std::path::PathBuf, i64),
        Deleted(std::path::PathBuf, i64),
        Unchanged,
    }
}

#[path = "../utils.rs"]
mod utils;

/// A constant array `INSTALL_HOOKS` that defines the file paths for a sequence of
/// installation hook scripts. These scripts are executed at different stages of
/// the installation process.
///
/// # Elements:
/// - `"uvd/hooks/install/prepare.sh"`: Script executed to prepare for the installation.
/// - `"uvd/hooks/install/pre.sh"`: Script executed before the main installation begins.
/// - `"uvd/hooks/install/install.sh"`: The main script responsible for the actual installation process.
/// - `"uvd/hooks/install/post.sh"`: Script executed after the installation process is complete.
///
/// This array ensures that the hooks are run in the correct order, allowing for
/// a modular and well-structured installation flow.
/// # Usage
/// This constant can be used to retrieve or iterate through the file paths of the
/// install hook scripts to manage uninstallation logic programmatically.
///
/// # Example
/// ```
/// for hook in &INSTALL_HOOKS {
///     println!("Executing install hook: {hook}");
/// }
/// ```
///
/// # Note
/// Ensure all scripts exist and are executable to avoid errors during the uninstallation process.
pub const INSTALL_HOOKS: [&str; 4] = [
    "uvd/hooks/install/prepare.sh",
    "uvd/hooks/install/pre.sh",
    "uvd/hooks/install/install.sh",
    "uvd/hooks/install/post.sh",
];

/// A constant array of strings representing the file paths to the uninstall hook scripts.
///
/// These scripts are executed during the uninstallation process of an application or service.
/// Each script serves a specific purpose in the uninstall lifecycle:
///
/// - `"uvd/hooks/uninstall/prepare.sh"`: A script to prepare the environment for uninstallation.
/// - `"uvd/hooks/uninstall/pre.sh"`: A script to execute any pre-uninstallation steps.
/// - `"uvd/hooks/uninstall/uninstall.sh"`: The main uninstallation script.
/// - `"uvd/hooks/uninstall/post.sh"`: A script to perform any post-uninstallation cleanup or finalization.
///
/// # Usage
/// This constant can be used to retrieve or iterate through the file paths of the
/// uninstall hook scripts to manage uninstallation logic programmatically.
///
/// # Example
/// ```
/// for hook in &UNINSTALL_HOOKS {
///     println!("Executing uninstall hook: {}", hook);
/// }
/// ```
///
/// # Note
/// Ensure all scripts exist and are executable to avoid errors during the uninstallation process.
pub const UNINSTALL_HOOKS: [&str; 4] = [
    "uvd/hooks/uninstall/prepare.sh",
    "uvd/hooks/uninstall/pre.sh",
    "uvd/hooks/uninstall/uninstall.sh",
    "uvd/hooks/uninstall/post.sh",
];

/// A constant array `UPDATE_HOOKS` containing the file paths of upgrade hook scripts.
///
/// These scripts are executed sequentially during the upgrade process to ensure proper preparation,
/// execution, and finalization of the upgrade. The array includes the following hooks:
///
/// - `"uvd/hooks/upgrade/prepare.sh"`: A script for preparing the environment or system before the upgrade.
/// - `"uvd/hooks/upgrade/pre.sh"`: A script that runs pre-upgrade tasks.
/// - `"uvd/hooks/upgrade/upgrade.sh"`: The main script responsible for performing the upgrade.
/// - `"uvd/hooks/upgrade/post.sh"`: A script for post-upgrade cleanup or finalization tasks.
///
/// The paths in this array are relative to the application's root directory, and it is assumed
/// that these scripts are present and executable in their respective locations.
///
/// # Example
/// ```rust
/// for hook in &UPDATE_HOOKS {
///     println!("Executing hook script: {hook}");
/// }
/// ```
pub const UPDATE_HOOKS: [&str; 4] = [
    "uvd/hooks/upgrade/prepare.sh",
    "uvd/hooks/upgrade/pre.sh",
    "uvd/hooks/upgrade/upgrade.sh",
    "uvd/hooks/upgrade/post.sh",
];

pub const PKG_HOOKS: [&str; 2] = ["uvd/hooks/package/dmg.sh", "uvd/hooks/package/exe.sh"];

#[derive(Serialize, Deserialize)]
pub struct Syl {
    pub name: String,
    pub author: String,
    pub description: String,
    pub version: String,
    pub arch: String,
    pub homepage: String,
    pub repository: String,
    pub license: String,
    pub icon: Option<String>,
    pub provides: Vec<String>,
    pub optional: Vec<String>,
    pub depends: Vec<String>,
    pub conflicts: Vec<String>,
    pub replaces: Vec<String>,
    pub output: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct UvdSignature {
    version: u8,
    hash: String,
    timestamp: i64,
    nonce: String,
    block_hash: String,
    signature: String,
}

fn decompress_archive(archive_path: &Path) -> Result<PathBuf, Error> {
    let archive_str = archive_path
        .to_str()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Archive path must be valid UTF-8"))?;
    let extract_dir = match archive_path.file_stem() {
        Some(stem) => archive_path.with_file_name(stem),
        None => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Archive path must have a file name",
            ));
        }
    };

    create_dir_all(&extract_dir)?;

    let extract_str = extract_dir.to_str().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            "Extraction path must be valid UTF-8",
        )
    })?;

    let status = ProcessCommand::new("tar")
        .args(["-xf", archive_str, "-C", extract_str])
        .status()?;

    if !status.success() {
        return Err(Error::new(
            ErrorKind::Other,
            format!(
                "Failed to extract archive {} (tar exit code: {:?})",
                archive_path.display(),
                status.code()
            ),
        ));
    }

    Ok(extract_dir)
}

fn create_hooks() -> Result<(), Error> {
    for hook in &INSTALL_HOOKS {
        if !Path::new(hook).exists() {
            create_dir_all(Path::new(hook).parent().unwrap())?;
            let mut f = File::create(hook)?;
            f.write_all(b"#!/bin/sh")?;
            f.sync_all()?;
        }
    }
    for hook in &UNINSTALL_HOOKS {
        if !Path::new(hook).exists() {
            create_dir_all(Path::new(hook).parent().unwrap())?;
            let mut f = File::create(hook)?;
            f.write_all(b"#!/bin/sh")?;
            f.sync_all()?;
        }
    }
    for hook in &UPDATE_HOOKS {
        if !Path::new(hook).exists() {
            create_dir_all(Path::new(hook).parent().unwrap())?;
            let mut f = File::create(hook)?;
            f.write_all(b"#!/bin/sh")?;
            f.sync_all()?;
        }
    }
    for hook in &PKG_HOOKS {
        if !Path::new(hook).exists() {
            create_dir_all(Path::new(hook).parent().unwrap())?;
            let mut f = File::create(hook)?;
            if hook.ends_with("dmg.sh") {
                f.write_all(
                    b"#!/bin/sh\n# Script to build DMG for macOS\necho \"Building DMG...\"\n",
                )?;
            } else if hook.ends_with("exe.sh") {
                f.write_all(
                    b"#!/bin/sh\n# Script to build EXE for Windows\necho \"Building EXE...\"\n",
                )?;
            } else {
                f.write_all(b"#!/bin/sh")?;
            }
            f.sync_all()?;
        }
    }
    Ok(())
}

pub fn create_uvd() -> Result<(), Error> {
    create_hooks()?;
    let toml = read_to_string("syl.toml")?;
    let syl: Syl = toml::from_str(&toml).expect("Failed to parse syl.toml");

    // Copy icon if specified
    if let Some(icon_path) = &syl.icon {
        if Path::new(icon_path).exists() {
            let dest = Path::new("uvd").join(icon_path);
            if let Some(parent) = dest.parent() {
                create_dir_all(parent)?;
            }
            fs::copy(icon_path, dest)?;
        }
    }

    File::create("uvd/uvd.json")?.write_all(serde_json::to_string(&syl)?.as_bytes())?;

    let output_dir = syl.output.as_deref().unwrap_or(".");
    if output_dir != "." {
        create_dir_all(output_dir)?;
    }

    let archive_name = format!("{}_{}.syl", syl.name, syl.version);
    let archive_path = Path::new(output_dir).join(&archive_name);

    // Si on a déjà une archive, on la supprime pour ne pas l'inclure si elle est dans le même dossier
    if archive_path.exists() {
        let _ = fs::remove_file(&archive_path);
    }

    // 1. Création de l'archive tar
    let status = ProcessCommand::new("tar")
        .args(["-cf", archive_path.to_str().unwrap(), "uvd"])
        .status()?;

    if !status.success() {
        return Err(Error::new(
            ErrorKind::Other,
            format!(
                "Failed to create archive {} (tar exit code: {:?})",
                archive_path.display(),
                status.code()
            ),
        ));
    }

    // 2. Signature de l'archive
    // On calcule le hash Blake3 du fichier tar généré
    let mut file = File::open(&archive_path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let hash = blake3::hash(&buffer).to_hex().to_string();

    let timestamp = Utc::now().timestamp();
    let nonce = Uuid::new_v4().to_string();
    let block_input = format!("{hash}:{timestamp}:{nonce}");
    let block_hash = blake3::hash(block_input.as_bytes()).to_hex().to_string();

    // On signe le block hash (inspiré blockchain)
    let root_path = std::env::current_dir()?;
    match sign_message(&root_path, &block_hash) {
        Ok(signature) => {
            // On crée un fichier de signature
            let sig_file = format!("{}.sig", archive_path.display());
            let payload = UvdSignature {
                version: 1,
                hash,
                timestamp,
                nonce,
                block_hash,
                signature,
            };
            File::create(sig_file)?
                .write_all(serde_json::to_string_pretty(&payload)?.as_bytes())?;
            println!(
                "Archive {} created and signed successfully.",
                archive_path.display()
            );
        }
        Err(e) => {
            println!("Warning: Could not sign the archive ({e}).");
            println!("Please ensure an identity key is generated using 'lys keygen'.");
        }
    }
    Ok(())
}

fn cli() -> Command {
    Command::new("syl")
        .about("Universal Verified Disc manager")
        .subcommand(Command::new("create").about("Generate a new signed UVD archive"))
        .subcommand(
            Command::new("verify")
                .about("Verify a signed UVD archive")
                .arg(Arg::new("archive").required(true).action(ArgAction::Set)),
        )
        .subcommand(
            Command::new("extract")
                .about("Extract a signed UVD archive")
                .arg(
                    Arg::new("archive")
                        .required(true)
                        .action(ArgAction::Set)
                        .help("Archive to extract"),
                ),
        )
}

pub fn main() -> Result<(), Error> {
    let matches = cli().get_matches();

    match matches.subcommand() {
        Some(("create", _)) => {
            create_uvd()?;
        }
        Some(("extract", a)) => {
            let archive_path = a.get_one::<String>("archive").unwrap();
            let p = Path::new(&archive_path);
            extract_uvd(&p.to_path_buf())?;
        }
        Some(("verify", a)) => {
            let archive_path = a.get_one::<String>("archive").unwrap();
            let p = Path::new(&archive_path);
            verify_uvd(&p.to_path_buf())?;
        }
        _ => {
            cli().print_help().expect("Failed to print help");
        }
    }
    Ok(())
}

fn verify_uvd(archive: &PathBuf) -> Result<(), Error> {
    if let Some(ext) = archive.extension() {
        if ext != "syl" {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Archive must have .syl extension",
            ));
        }
    } else {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "Archive must have .syl extension",
        ));
    }

    if !archive.exists() {
        return Err(Error::new(ErrorKind::NotFound, "Archive not found"));
    }

    let sig_path = PathBuf::from(format!("{}.sig", archive.display()));
    if !sig_path.exists() {
        return Err(Error::new(ErrorKind::NotFound, "Signature file not found"));
    }

    let mut file = File::open(archive)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    let hash = blake3::hash(&buffer).to_hex().to_string();

    let signature_raw = read_to_string(&sig_path)?;
    let signature_raw = signature_raw.trim();
    if signature_raw.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Signature file is empty",
        ));
    }

    let root_path = std::env::current_dir()?;
    if let Ok(payload) = serde_json::from_str::<UvdSignature>(signature_raw) {
        if payload.signature.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Signature payload is missing signature",
            ));
        }
        if payload.hash != hash {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Archive hash does not match signature payload",
            ));
        }
        let block_input = format!("{}:{}:{}", payload.hash, payload.timestamp, payload.nonce);
        let expected_block_hash = blake3::hash(block_input.as_bytes()).to_hex().to_string();
        if payload.block_hash != expected_block_hash {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "Block hash does not match signature payload",
            ));
        }

        match verify_signature(&root_path, &payload.block_hash, &payload.signature) {
            Ok(true) => {
                ok("Archive signature verified.");
                Ok(())
            }
            Ok(false) => Err(Error::new(
                ErrorKind::Other,
                "Archive signature verification failed",
            )),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    } else {
        match verify_signature(&root_path, &hash, signature_raw) {
            Ok(true) => {
                ok("Archive signature verified (legacy format).");
                Ok(())
            }
            Ok(false) => Err(Error::new(
                ErrorKind::Other,
                "Archive signature verification failed",
            )),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    }
}
fn extract_uvd(archive: &PathBuf) -> Result<(), Error> {
    if let Some(ext) = archive.extension() {
        if ext != "syl" {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Archive must have .syl extension",
            ));
        }
        decompress_archive(&archive)?;
        ok("Archive extracted successfully.");
        return Ok(());
    }
    Err(Error::new(
        ErrorKind::InvalidInput,
        "Archive must have .syl extension",
    ))
}
