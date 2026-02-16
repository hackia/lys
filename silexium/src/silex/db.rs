use anyhow::{anyhow, Context, Result};
use sqlite::{Connection, State};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ManifestData {
    pub id: i64,
    pub format: String,
    pub bytes: Vec<u8>,
    pub blake3: String,
    pub src_index_size: Option<i64>,
    pub src_index_blake3: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ArtifactData {
    pub kind: String,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub size: i64,
    pub blake3: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct AttestationData {
    pub kind: String,
    pub key_id: String,
    pub payload_hash: String,
    pub signature: String,
    pub created_at: String,
    pub tsa_proof: Vec<u8>,
    pub ots_proof: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct LogEntryData {
    pub seq: i64,
    pub entry_hash: String,
}

#[derive(Debug, Clone)]
pub struct StoredSth {
    pub tree_size: u64,
    pub root_hash: String,
    pub timestamp: String,
    pub signature: String,
    pub key_id: String,
}

#[derive(Debug, Clone)]
pub struct ReleaseData {
    pub version: String,
    pub channel: String,
    pub up_to_date: bool,
    pub manifest: ManifestData,
    pub artifacts: Vec<ArtifactData>,
    pub attestations: Vec<AttestationData>,
    pub log_entry: LogEntryData,
}

pub fn open_db(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create data directory")?;
    }
    let conn = sqlite::open(path).context("failed to open sqlite database")?;
    Ok(conn)
}

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute(
        r#"
        CREATE TABLE IF NOT EXISTS packages (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            description TEXT,
            license TEXT,
            homepage TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS versions (
            id INTEGER PRIMARY KEY,
            package_id INTEGER NOT NULL,
            version TEXT NOT NULL,
            channel TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(package_id, version, channel)
        );
        CREATE TABLE IF NOT EXISTS artifacts (
            id INTEGER PRIMARY KEY,
            version_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            os TEXT,
            arch TEXT,
            size INTEGER NOT NULL,
            blake3 TEXT NOT NULL,
            url TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS manifests (
            id INTEGER PRIMARY KEY,
            version_id INTEGER NOT NULL,
            format TEXT NOT NULL,
            bytes BLOB NOT NULL,
            blake3 TEXT NOT NULL,
            src_index_size INTEGER,
            src_index_blake3 TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS attestations (
            id INTEGER PRIMARY KEY,
            version_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            key_id TEXT NOT NULL,
            payload_hash TEXT NOT NULL,
            signature TEXT NOT NULL,
            created_at TEXT NOT NULL,
            tsa_proof BLOB NOT NULL,
            ots_proof BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS keys (
            id INTEGER PRIMARY KEY,
            key_id TEXT NOT NULL UNIQUE,
            role TEXT NOT NULL,
            public_key BLOB NOT NULL,
            revoked INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS log_entries (
            id INTEGER PRIMARY KEY,
            seq INTEGER NOT NULL,
            manifest_id INTEGER NOT NULL,
            author_attestation_id INTEGER NOT NULL,
            tests_attestation_id INTEGER NOT NULL,
            server_attestation_id INTEGER NOT NULL,
            entry_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(seq)
        );
        CREATE TABLE IF NOT EXISTS log_sth (
            id INTEGER PRIMARY KEY,
            tree_size INTEGER NOT NULL,
            root_hash TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            signature TEXT NOT NULL,
            key_id TEXT NOT NULL
        );
        "#,
    )
    .context("failed to create schema")?;
    Ok(())
}

pub fn resolve_release(
    conn: &Connection,
    package: &str,
    os: &str,
    arch: &str,
    version: Option<&str>,
    channel: Option<&str>,
    current_version: Option<&str>,
) -> Result<ReleaseData> {
    let package_id = fetch_package_id(conn, package)?;
    let channel = channel.unwrap_or("stable").to_string();

    let version_row = if let Some(version) = version {
        fetch_version(conn, package_id, version, &channel)?
    } else {
        fetch_latest_version(conn, package_id, &channel)?
    };

    let up_to_date = current_version
        .map(|v| v == version_row.version)
        .unwrap_or(false);

    let manifest = fetch_manifest(conn, version_row.id)?;
    let artifacts = fetch_artifacts(conn, version_row.id, os, arch)?;
    let attestations = fetch_attestations(conn, version_row.id)?;
    let log_entry = fetch_log_entry(conn, manifest.id)?;

    Ok(ReleaseData {
        version: version_row.version,
        channel: version_row.channel,
        up_to_date,
        manifest,
        artifacts,
        attestations,
        log_entry,
    })
}

pub fn load_log_entries(conn: &Connection) -> Result<Vec<LogEntryData>> {
    let mut stmt = conn
        .prepare("SELECT seq, entry_hash FROM log_entries ORDER BY seq ASC")?;
    let mut entries = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        let seq: i64 = stmt.read(0)?;
        let entry_hash: String = stmt.read(1)?;
        entries.push(LogEntryData { seq, entry_hash });
    }
    Ok(entries)
}

pub fn load_sth(conn: &Connection, tree_size: u64) -> Result<Option<StoredSth>> {
    let mut stmt = conn.prepare(
        "SELECT tree_size, root_hash, timestamp, signature, key_id FROM log_sth WHERE tree_size = ?",
    )?;
    stmt.bind((1, tree_size as i64))?;
    if let Ok(State::Row) = stmt.next() {
        let tree_size: i64 = stmt.read(0)?;
        let root_hash: String = stmt.read(1)?;
        let timestamp: String = stmt.read(2)?;
        let signature: String = stmt.read(3)?;
        let key_id: String = stmt.read(4)?;
        return Ok(Some(StoredSth {
            tree_size: tree_size as u64,
            root_hash,
            timestamp,
            signature,
            key_id,
        }));
    }
    Ok(None)
}

pub fn store_sth(conn: &Connection, sth: &StoredSth) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO log_sth (tree_size, root_hash, timestamp, signature, key_id) VALUES (?, ?, ?, ?, ?)",
    )?;
    stmt.bind((1, sth.tree_size as i64))?;
    stmt.bind((2, sth.root_hash.as_str()))?;
    stmt.bind((3, sth.timestamp.as_str()))?;
    stmt.bind((4, sth.signature.as_str()))?;
    stmt.bind((5, sth.key_id.as_str()))?;
    stmt.next()?;
    Ok(())
}

#[derive(Debug, Clone)]
struct VersionRow {
    pub id: i64,
    pub version: String,
    pub channel: String,
}

fn fetch_package_id(conn: &Connection, package: &str) -> Result<i64> {
    let mut stmt = conn.prepare("SELECT id FROM packages WHERE name = ?")?;
    stmt.bind((1, package))?;
    if let Ok(State::Row) = stmt.next() {
        let id: i64 = stmt.read(0)?;
        return Ok(id);
    }
    Err(anyhow!("package not found"))
}

fn fetch_version(conn: &Connection, package_id: i64, version: &str, channel: &str) -> Result<VersionRow> {
    let mut stmt = conn.prepare(
        "SELECT id, version, channel FROM versions WHERE package_id = ? AND version = ? AND channel = ?",
    )?;
    stmt.bind((1, package_id))?;
    stmt.bind((2, version))?;
    stmt.bind((3, channel))?;
    if let Ok(State::Row) = stmt.next() {
        let id: i64 = stmt.read(0)?;
        let version: String = stmt.read(1)?;
        let channel: String = stmt.read(2)?;
        return Ok(VersionRow { id, version, channel });
    }
    Err(anyhow!("version not found"))
}

fn fetch_latest_version(conn: &Connection, package_id: i64, channel: &str) -> Result<VersionRow> {
    let mut stmt = conn.prepare(
        "SELECT id, version, channel FROM versions WHERE package_id = ? AND channel = ? ORDER BY created_at DESC LIMIT 1",
    )?;
    stmt.bind((1, package_id))?;
    stmt.bind((2, channel))?;
    if let Ok(State::Row) = stmt.next() {
        let id: i64 = stmt.read(0)?;
        let version: String = stmt.read(1)?;
        let channel: String = stmt.read(2)?;
        return Ok(VersionRow { id, version, channel });
    }
    Err(anyhow!("no versions found"))
}

fn fetch_manifest(conn: &Connection, version_id: i64) -> Result<ManifestData> {
    let mut stmt = conn.prepare(
        "SELECT id, format, bytes, blake3, src_index_size, src_index_blake3 FROM manifests WHERE version_id = ? ORDER BY id DESC LIMIT 1",
    )?;
    stmt.bind((1, version_id))?;
    if let Ok(State::Row) = stmt.next() {
        let id: i64 = stmt.read(0)?;
        let format: String = stmt.read(1)?;
        let bytes: Vec<u8> = stmt.read(2)?;
        let blake3: String = stmt.read(3)?;
        let src_index_size: Option<i64> = stmt.read(4).ok();
        let src_index_blake3: Option<String> = stmt.read(5).ok();
        return Ok(ManifestData {
            id,
            format,
            bytes,
            blake3,
            src_index_size,
            src_index_blake3,
        });
    }
    Err(anyhow!("manifest not found"))
}

fn fetch_artifacts(conn: &Connection, version_id: i64, os: &str, arch: &str) -> Result<Vec<ArtifactData>> {
    let mut stmt = conn.prepare(
        "SELECT kind, os, arch, size, blake3, url FROM artifacts WHERE version_id = ? AND (kind = 'source' OR (kind = 'binary' AND os = ? AND arch = ?))",
    )?;
    stmt.bind((1, version_id))?;
    stmt.bind((2, os))?;
    stmt.bind((3, arch))?;
    let mut artifacts = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        let kind: String = stmt.read(0)?;
        let os_val: Option<String> = stmt.read(1).ok();
        let arch_val: Option<String> = stmt.read(2).ok();
        let size: i64 = stmt.read(3)?;
        let blake3: String = stmt.read(4)?;
        let url: String = stmt.read(5)?;
        artifacts.push(ArtifactData {
            kind,
            os: os_val,
            arch: arch_val,
            size,
            blake3,
            url,
        });
    }
    if artifacts.is_empty() {
        return Err(anyhow!("no artifacts for requested os/arch"));
    }
    Ok(artifacts)
}

fn fetch_attestations(conn: &Connection, version_id: i64) -> Result<Vec<AttestationData>> {
    let mut stmt = conn.prepare(
        "SELECT kind, key_id, payload_hash, signature, created_at, tsa_proof, ots_proof FROM attestations WHERE version_id = ?",
    )?;
    stmt.bind((1, version_id))?;
    let mut attestations = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        let kind: String = stmt.read(0)?;
        let key_id: String = stmt.read(1)?;
        let payload_hash: String = stmt.read(2)?;
        let signature: String = stmt.read(3)?;
        let created_at: String = stmt.read(4)?;
        let tsa_proof: Vec<u8> = stmt.read(5)?;
        let ots_proof: Vec<u8> = stmt.read(6)?;
        attestations.push(AttestationData {
            kind,
            key_id,
            payload_hash,
            signature,
            created_at,
            tsa_proof,
            ots_proof,
        });
    }
    if attestations.len() < 3 {
        return Err(anyhow!("missing attestations"));
    }
    Ok(attestations)
}

fn fetch_log_entry(conn: &Connection, manifest_id: i64) -> Result<LogEntryData> {
    let mut stmt = conn.prepare(
        "SELECT seq, entry_hash FROM log_entries WHERE manifest_id = ?",
    )?;
    stmt.bind((1, manifest_id))?;
    if let Ok(State::Row) = stmt.next() {
        let seq: i64 = stmt.read(0)?;
        let entry_hash: String = stmt.read(1)?;
        return Ok(LogEntryData { seq, entry_hash });
    }
    Err(anyhow!("log entry not found"))
}
