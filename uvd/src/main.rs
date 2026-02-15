use crate::crypto::{sign_message, verify_signature};
use crate::utils::ok;
use chrono::Utc;
use clap::{Arg, ArgAction, Command};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, create_dir_all, read_to_string};
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::NamedTempFile;
use zstd::stream::{decode_all, encode_all};
pub mod tree;

pub mod utils;
pub mod vcs {
    pub enum FileStatus {
        New(std::path::PathBuf),
        Modified(std::path::PathBuf, i64),
        Deleted(std::path::PathBuf, i64),
        Unchanged,
    }
}
pub mod crypto;

#[derive(Serialize, Deserialize)]
pub struct Uvd {
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
struct UvdMetadata {
    timestamp: u64,
    prev_block_hash: Option<String>,
    content_hash: String,
    signature: String,
}

#[derive(Serialize)]
struct UvdMetadataUnsigned {
    timestamp: u64,
    prev_block_hash: Option<String>,
    content_hash: String,
}

fn metadata_hash(unsigned: &UvdMetadataUnsigned) -> Result<String, Error> {
    let bytes = serde_json::to_vec(unsigned)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
}

fn validate_archive_path(archive: &Path) -> Result<(), Error> {
    match archive.extension().and_then(|ext| ext.to_str()) {
        Some("uvd") => {}
        _ => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Archive must have .uvd extension",
            ));
        }
    }

    if !archive.exists() {
        return Err(Error::new(ErrorKind::NotFound, "Archive not found"));
    }

    Ok(())
}

fn fail_and_delete<T>(archive: &Path, message: &str) -> Result<T, Error> {
    let _ = fs::remove_file(archive);
    Err(Error::new(ErrorKind::Other, message))
}

fn read_uvd_footer(file: &mut File) -> Result<(UvdMetadata, u64), Error> {
    let file_len = file.metadata()?.len();
    if file_len < 4 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Archive footer is missing",
        ));
    }

    file.seek(SeekFrom::End(-4))?;
    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)?;
    let json_len = u32::from_le_bytes(len_buf) as u64;
    if json_len == 0 || json_len > file_len - 4 {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Invalid metadata length",
        ));
    }

    let json_start = file_len - 4 - json_len;
    if json_start > usize::MAX as u64 {
        return Err(Error::new(ErrorKind::InvalidData, "Archive is too large"));
    }

    file.seek(SeekFrom::Start(json_start))?;
    let mut json_bytes = vec![0u8; json_len as usize];
    file.read_exact(&mut json_bytes)?;
    let metadata: UvdMetadata = serde_json::from_slice(&json_bytes)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

    Ok((metadata, json_start))
}

fn read_and_verify_uvd(archive: &Path) -> Result<Vec<u8>, Error> {
    validate_archive_path(archive)?;

    let mut file = File::open(archive)?;
    let (metadata, data_len) = read_uvd_footer(&mut file)?;

    if metadata.signature.trim().is_empty() {
        return fail_and_delete(archive, "Signature payload is missing signature");
    }

    let now = Utc::now().timestamp();
    let now = if now < 0 { 0 } else { now as u64 };
    if metadata.timestamp > now {
        return fail_and_delete(archive, "Timestamp is in the future");
    }

    let unsigned = UvdMetadataUnsigned {
        timestamp: metadata.timestamp,
        prev_block_hash: metadata.prev_block_hash.clone(),
        content_hash: metadata.content_hash.clone(),
    };
    let unsigned_hash = metadata_hash(&unsigned)?;

    let root_path = std::env::current_dir()?;
    match verify_signature(&root_path, &unsigned_hash, metadata.signature.trim()) {
        Ok(true) => {}
        Ok(false) => {
            return fail_and_delete(archive, "Archive signature verification failed");
        }
        Err(e) => {
            return Err(Error::new(ErrorKind::Other, e));
        }
    }

    file.seek(SeekFrom::Start(0))?;
    let mut archive_data = vec![0u8; data_len as usize];
    file.read_exact(&mut archive_data)?;
    let content_hash = blake3::hash(&archive_data).to_hex().to_string();
    if content_hash != metadata.content_hash {
        return fail_and_delete(archive, "Archive hash does not match metadata");
    }

    Ok(archive_data)
}

fn create_hooks() -> Result<(), Error> {
    let uvd = Path::new("uvd");
    let hooks = uvd.join("hooks");
    let tree = uvd.join("tree");
    let macos = hooks.join("macos");
    let linux = hooks.join("linux");
    let windows = hooks.join("windows");
    let bsd = hooks.join("bsd");
    if !uvd.exists() {
        create_dir_all(uvd.to_path_buf())?;
    }
    if !macos.exists() {
        create_dir_all(macos.to_path_buf())?;
    }

    if !windows.exists() {
        create_dir_all(windows.to_path_buf())?;
    }

    if !linux.exists() {
        create_dir_all(linux.to_path_buf())?;
    }
    if !bsd.exists() {
        create_dir_all(bsd.to_path_buf())?;
    }

    if !hooks.exists() {
        create_dir_all(hooks.to_path_buf())?;
    }

    if !tree.exists() {
        create_dir_all(tree.to_path_buf())?;
    }

    let windows_hooks_list = [
        windows.join("pre-upgrade.bat"),
        windows.join("upgrade.bat"),
        windows.join("post-upgrade.bat"),
        windows.join("pre-uninstall.bat"),
        windows.join("uninstall.bat"),
        windows.join("post-uninstall.bat"),
        windows.join("pre-install.bat"),
        windows.join("install.bat"),
        windows.join("post-install.bat"),
        windows.join("build.bat"),
    ];
    let linux_hooks_list = [
        linux.join("pre-upgrade.sh"),
        linux.join("upgrade.sh"),
        linux.join("post-upgrade.sh"),
        linux.join("pre-uninstall.sh"),
        linux.join("uninstall.sh"),
        linux.join("post-uninstall.sh"),
        linux.join("pre-install.sh"),
        linux.join("install.sh"),
        linux.join("post-install.sh"),
        linux.join("build.sh"),
    ];

    let macos_hooks_list = [
        macos.join("pre-upgrade.sh"),
        macos.join("upgrade.sh"),
        macos.join("post-upgrade.sh"),
        macos.join("pre-uninstall.sh"),
        macos.join("uninstall.sh"),
        macos.join("post-uninstall.sh"),
        macos.join("pre-install.sh"),
        macos.join("install.sh"),
        macos.join("post-install.sh"),
        macos.join("build.sh"),
    ];

    let bsd_hooks_list = [
        bsd.join("pre-upgrade.sh"),
        bsd.join("upgrade.sh"),
        bsd.join("post-upgrade.sh"),
        bsd.join("pre-uninstall.sh"),
        bsd.join("uninstall.sh"),
        bsd.join("post-uninstall.sh"),
        bsd.join("pre-install.sh"),
        bsd.join("install.sh"),
        bsd.join("post-install.sh"),
        bsd.join("build.sh"),
    ];

    for hook in &windows_hooks_list {
        if !hook.exists() {
            create_dir_all(hook.parent().unwrap())?;
            let mut f = File::create(hook)?;
            f.write_all(b"@echo off\necho Running upgrade hook for UVD...")?;
            f.sync_all()?;
        }
    }

    for hook in &bsd_hooks_list {
        if !hook.exists() {
            create_dir_all(hook.parent().unwrap())?;
            let mut f = File::create(hook)?;
            f.write_all(b"#!/bin/sh")?;
            f.sync_all()?;
        }
    }

    for hook in &linux_hooks_list {
        if !hook.exists() {
            create_dir_all(hook.parent().unwrap())?;
            let mut f = File::create(hook)?;
            f.write_all(b"#!/bin/sh")?;
            f.sync_all()?;
        }
    }

    for hook in &macos_hooks_list {
        if !hook.exists() {
            create_dir_all(hook.parent().unwrap())?;
            let mut f = File::create(hook)?;
            f.write_all(b"#!/bin/sh")?;
            f.sync_all()?;
        }
    }
    Ok(())
}

fn copy_repo_tree_into_uvd() -> Result<(), Error> {
    let repo_root = std::env::current_dir()?;
    let tree_dir = repo_root.join("uvd").join("tree");

    if tree_dir.exists() {
        fs::remove_dir_all(&tree_dir)?;
    }
    create_dir_all(&tree_dir)?;

    let files = tree::ls_files(&repo_root);
    for rel_path in files {
        let rel_path = PathBuf::from(rel_path);
        if rel_path.starts_with(Path::new(".git")) {
            continue;
        }
        if rel_path.starts_with(Path::new("uvd").join("tree")) {
            continue;
        }

        let src_path = repo_root.join(&rel_path);
        let dest_path = tree_dir.join(rel_path);
        if let Some(parent) = dest_path.parent() {
            create_dir_all(parent)?;
        }
        fs::copy(src_path, dest_path)?;
    }

    Ok(())
}

fn cleanup_uvd_tree() -> Result<(), Error> {
    let tree_dir = Path::new("uvd").join("tree");
    if tree_dir.exists() {
        fs::remove_dir_all(&tree_dir)?;
    }
    Ok(())
}

pub fn create_uvd() -> Result<(), Error> {
    create_hooks()?;
    let toml = read_to_string("uvd.toml")?;
    let uvd: Uvd = toml::from_str(&toml).expect("Failed to parse uvd.toml");

    // Copy icon if specified
    if let Some(icon_path) = &uvd.icon {
        if Path::new(icon_path).exists() {
            let dest = Path::new("uvd").join(icon_path);
            if let Some(parent) = dest.parent() {
                create_dir_all(parent)?;
            }
            fs::copy(icon_path, dest)?;
        }
    }

    File::create("uvd/uvd.json")?.write_all(serde_json::to_string(&uvd)?.as_bytes())?;
    copy_repo_tree_into_uvd()?;

    let output_dir = uvd.output.as_deref().unwrap_or(".");
    if output_dir != "." {
        create_dir_all(output_dir)?;
    }

    let archive_name = format!("{}_{}.uvd", uvd.name, uvd.version);
    let archive_path = Path::new(output_dir).join(&archive_name);

    // Si on a déjà une archive, on la supprime pour ne pas l'inclure si elle est dans le même dossier
    if archive_path.exists() {
        let _ = fs::remove_file(&archive_path);
    }

    let tar_path = NamedTempFile::new()?.into_temp_path();
    let tar_path_str = tar_path
        .to_str()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Temporary path invalid"))?;

    // 1. Création de l'archive tar
    let status = ProcessCommand::new("tar")
        .args(["-cf", tar_path_str, "uvd"])
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

    // 2. Compression zstd
    let mut tar_buffer = Vec::new();
    File::open(&tar_path)?.read_to_end(&mut tar_buffer)?;
    let archive_data = encode_all(&tar_buffer[..], 0)?;

    // 3. Hash du contenu compressé
    let content_hash = blake3::hash(&archive_data).to_hex().to_string();

    let timestamp = Utc::now().timestamp();
    let timestamp = if timestamp < 0 { 0 } else { timestamp as u64 };
    let unsigned = UvdMetadataUnsigned {
        timestamp,
        prev_block_hash: None,
        content_hash: content_hash.clone(),
    };
    let unsigned_hash = metadata_hash(&unsigned)?;

    // 4. Signature des métadonnées
    let root_path = std::env::current_dir()?;
    let signature =
        sign_message(&root_path, &unsigned_hash).map_err(|e| Error::new(ErrorKind::Other, e))?;

    let metadata = UvdMetadata {
        timestamp,
        prev_block_hash: None,
        content_hash,
        signature,
    };
    let metadata_bytes = serde_json::to_vec(&metadata)
        .map_err(|e| Error::new(ErrorKind::InvalidData, e.to_string()))?;

    if metadata_bytes.len() > u32::MAX as usize {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "Metadata size exceeds limit",
        ));
    }

    // 5. Écriture finale: [archive_data][metadata_json][u32 length]
    let mut out = File::create(&archive_path)?;
    out.write_all(&archive_data)?;
    out.write_all(&metadata_bytes)?;
    out.write_all(&(metadata_bytes.len() as u32).to_le_bytes())?;
    out.sync_all()?;
    cleanup_uvd_tree()?;

    ok(format!(
        "Archive {} created and signed successfully.",
        archive_path.display()
    )
    .as_str());
    Ok(())
}

fn cli() -> Command {
    Command::new("uvd")
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
    let _ = read_and_verify_uvd(archive)?;
    ok("Archive signature verified.");
    Ok(())
}

fn verify_and_unpack(archive: &Path) -> Result<PathBuf, Error> {
    let archive_data = read_and_verify_uvd(archive)?;
    let tar_data = decode_all(&archive_data[..])?;

    let extract_dir = match archive.file_stem() {
        Some(stem) => archive.with_file_name(stem),
        None => {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "Archive path must have a file name",
            ));
        }
    };
    create_dir_all(&extract_dir)?;

    let tar_path = NamedTempFile::new()?.into_temp_path();
    fs::write(&tar_path, &tar_data)?;
    let tar_path_str = tar_path
        .to_str()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "Temporary path invalid"))?;

    let extract_str = extract_dir.to_str().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            "Extraction path must be valid UTF-8",
        )
    })?;

    let status = ProcessCommand::new("tar")
        .args(["-xf", tar_path_str, "-C", extract_str])
        .status()?;
    if !status.success() {
        return Err(Error::new(
            ErrorKind::Other,
            format!(
                "Failed to extract archive {} (tar exit code: {:?})",
                archive.display(),
                status.code()
            ),
        ));
    }
    Ok(extract_dir)
}

fn extract_uvd(archive: &PathBuf) -> Result<(), Error> {
    let _ = verify_and_unpack(archive)?;
    ok("Archive extracted successfully.");
    Ok(())
}
