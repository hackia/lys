use anyhow::{anyhow, Context, Result};
use chrono::Utc;
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
pub struct KeyData {
    pub key_id: String,
    pub role: String,
    pub public_key: Vec<u8>,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub revoked: bool,
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
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            revoked_at TEXT
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
    migrate_keys_table(conn)?;
    Ok(())
}

fn migrate_keys_table(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(keys)")?;
    let mut has_expires = false;
    let mut has_revoked_at = false;
    while let Ok(State::Row) = stmt.next() {
        let name: String = stmt.read(1)?;
        match name.as_str() {
            "expires_at" => has_expires = true,
            "revoked_at" => has_revoked_at = true,
            _ => {}
        }
    }
    if !has_expires {
        conn.execute("ALTER TABLE keys ADD COLUMN expires_at TEXT")?;
    }
    if !has_revoked_at {
        conn.execute("ALTER TABLE keys ADD COLUMN revoked_at TEXT")?;
    }
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

pub fn fetch_key(conn: &Connection, key_id: &str) -> Result<KeyData> {
    let mut stmt = conn.prepare(
        "SELECT key_id, role, public_key, created_at, expires_at, revoked_at, revoked FROM keys WHERE key_id = ?",
    )?;
    stmt.bind((1, key_id))?;
    if let Ok(State::Row) = stmt.next() {
        let key_id: String = stmt.read(0)?;
        let role: String = stmt.read(1)?;
        let public_key: Vec<u8> = stmt.read(2)?;
        let created_at: String = stmt.read(3)?;
        let expires_at: Option<String> = stmt.read(4).ok();
        let revoked_at: Option<String> = stmt.read(5).ok();
        let revoked: i64 = stmt.read(6).unwrap_or(0);
        return Ok(KeyData {
            key_id,
            role,
            public_key,
            created_at,
            expires_at,
            revoked_at,
            revoked: revoked != 0,
        });
    }
    Err(anyhow!("key not found"))
}

pub fn insert_key(
    conn: &Connection,
    key_id: &str,
    role: &str,
    public_key: &[u8],
    expires_at: &str,
    revoked_at: Option<&str>,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO keys (key_id, role, public_key, revoked, created_at, expires_at, revoked_at) VALUES (?, ?, ?, 0, ?, ?, ?)",
    )?;
    stmt.bind((1, key_id))?;
    stmt.bind((2, role))?;
    stmt.bind((3, public_key))?;
    stmt.bind((4, Utc::now().to_rfc3339().as_str()))?;
    stmt.bind((5, expires_at))?;
    if let Some(revoked_at) = revoked_at {
        stmt.bind((6, revoked_at))?;
    } else {
        stmt.bind((6, sqlite::Value::Null))?;
    }
    stmt.next()?;
    Ok(())
}

pub fn revoke_key(conn: &Connection, key_id: &str, revoked_at: &str) -> Result<()> {
    let _ = fetch_key(conn, key_id)?;
    let mut stmt =
        conn.prepare("UPDATE keys SET revoked = 1, revoked_at = ? WHERE key_id = ?")?;
    stmt.bind((1, revoked_at))?;
    stmt.bind((2, key_id))?;
    stmt.next()?;
    Ok(())
}

pub fn list_keys(conn: &Connection) -> Result<Vec<KeyData>> {
    let mut stmt = conn.prepare(
        "SELECT key_id, role, public_key, created_at, expires_at, revoked_at, revoked FROM keys ORDER BY created_at ASC",
    )?;
    let mut keys = Vec::new();
    while let Ok(State::Row) = stmt.next() {
        let key_id: String = stmt.read(0)?;
        let role: String = stmt.read(1)?;
        let public_key: Vec<u8> = stmt.read(2)?;
        let created_at: String = stmt.read(3)?;
        let expires_at: Option<String> = stmt.read(4).ok();
        let revoked_at: Option<String> = stmt.read(5).ok();
        let revoked: i64 = stmt.read(6).unwrap_or(0);
        keys.push(KeyData {
            key_id,
            role,
            public_key,
            created_at,
            expires_at,
            revoked_at,
            revoked: revoked != 0,
        });
    }
    Ok(keys)
}

pub fn revoke_expired_keys(conn: &Connection, now: &str) -> Result<i64> {
    let mut stmt = conn.prepare(
        "UPDATE keys SET revoked = 1, revoked_at = ? WHERE revoked = 0 AND revoked_at IS NULL AND expires_at IS NOT NULL AND expires_at <= ?",
    )?;
    stmt.bind((1, now))?;
    stmt.bind((2, now))?;
    stmt.next()?;

    let mut changes = conn.prepare("SELECT changes()")?;
    if let Ok(State::Row) = changes.next() {
        let count: i64 = changes.read(0)?;
        return Ok(count);
    }
    Ok(0)
}

pub fn upsert_package(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    license: Option<&str>,
    homepage: Option<&str>,
    created_at: &str,
) -> Result<i64> {
    if let Ok(id) = fetch_package_id(conn, name) {
        return Ok(id);
    }
    let mut stmt = conn.prepare(
        "INSERT INTO packages (name, description, license, homepage, created_at) VALUES (?, ?, ?, ?, ?)",
    )?;
    stmt.bind((1, name))?;
    stmt.bind((2, description.unwrap_or("")))?;
    stmt.bind((3, license.unwrap_or("")))?;
    stmt.bind((4, homepage.unwrap_or("")))?;
    stmt.bind((5, created_at))?;
    stmt.next()?;
    last_insert_rowid(conn)
}

pub fn upsert_version(
    conn: &Connection,
    package_id: i64,
    version: &str,
    channel: &str,
    created_at: &str,
) -> Result<i64> {
    if let Ok(row) = fetch_version(conn, package_id, version, channel) {
        return Ok(row.id);
    }
    let mut stmt = conn.prepare(
        "INSERT INTO versions (package_id, version, channel, created_at) VALUES (?, ?, ?, ?)",
    )?;
    stmt.bind((1, package_id))?;
    stmt.bind((2, version))?;
    stmt.bind((3, channel))?;
    stmt.bind((4, created_at))?;
    stmt.next()?;
    last_insert_rowid(conn)
}

pub fn insert_manifest(
    conn: &Connection,
    version_id: i64,
    format: &str,
    bytes: &[u8],
    blake3: &str,
    src_index_size: Option<i64>,
    src_index_blake3: Option<&str>,
    created_at: &str,
) -> Result<i64> {
    let mut stmt = conn.prepare(
        "INSERT INTO manifests (version_id, format, bytes, blake3, src_index_size, src_index_blake3, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.bind((1, version_id))?;
    stmt.bind((2, format))?;
    stmt.bind((3, bytes))?;
    stmt.bind((4, blake3))?;
    if let Some(size) = src_index_size {
        stmt.bind((5, size))?;
    } else {
        stmt.bind((5, sqlite::Value::Null))?;
    }
    if let Some(hash) = src_index_blake3 {
        stmt.bind((6, hash))?;
    } else {
        stmt.bind((6, sqlite::Value::Null))?;
    }
    stmt.bind((7, created_at))?;
    stmt.next()?;
    last_insert_rowid(conn)
}

pub fn insert_artifact(conn: &Connection, version_id: i64, artifact: &ArtifactData) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO artifacts (version_id, kind, os, arch, size, blake3, url) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.bind((1, version_id))?;
    stmt.bind((2, artifact.kind.as_str()))?;
    if let Some(os) = &artifact.os {
        stmt.bind((3, os.as_str()))?;
    } else {
        stmt.bind((3, sqlite::Value::Null))?;
    }
    if let Some(arch) = &artifact.arch {
        stmt.bind((4, arch.as_str()))?;
    } else {
        stmt.bind((4, sqlite::Value::Null))?;
    }
    stmt.bind((5, artifact.size))?;
    stmt.bind((6, artifact.blake3.as_str()))?;
    stmt.bind((7, artifact.url.as_str()))?;
    stmt.next()?;
    Ok(())
}

pub fn insert_attestation(
    conn: &Connection,
    version_id: i64,
    attestation: &AttestationData,
) -> Result<i64> {
    let mut stmt = conn.prepare(
        "INSERT INTO attestations (version_id, kind, key_id, payload_hash, signature, created_at, tsa_proof, ots_proof) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.bind((1, version_id))?;
    stmt.bind((2, attestation.kind.as_str()))?;
    stmt.bind((3, attestation.key_id.as_str()))?;
    stmt.bind((4, attestation.payload_hash.as_str()))?;
    stmt.bind((5, attestation.signature.as_str()))?;
    stmt.bind((6, attestation.created_at.as_str()))?;
    stmt.bind((7, attestation.tsa_proof.as_slice()))?;
    stmt.bind((8, attestation.ots_proof.as_slice()))?;
    stmt.next()?;
    last_insert_rowid(conn)
}

pub fn next_log_seq(conn: &Connection) -> Result<i64> {
    let mut stmt = conn.prepare("SELECT COALESCE(MAX(seq), 0) FROM log_entries")?;
    if let Ok(State::Row) = stmt.next() {
        let max_seq: i64 = stmt.read(0)?;
        return Ok(max_seq + 1);
    }
    Ok(1)
}

pub fn insert_log_entry(
    conn: &Connection,
    seq: i64,
    manifest_id: i64,
    author_attestation_id: i64,
    tests_attestation_id: i64,
    server_attestation_id: i64,
    entry_hash: &str,
    created_at: &str,
) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO log_entries (seq, manifest_id, author_attestation_id, tests_attestation_id, server_attestation_id, entry_hash, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )?;
    stmt.bind((1, seq))?;
    stmt.bind((2, manifest_id))?;
    stmt.bind((3, author_attestation_id))?;
    stmt.bind((4, tests_attestation_id))?;
    stmt.bind((5, server_attestation_id))?;
    stmt.bind((6, entry_hash))?;
    stmt.bind((7, created_at))?;
    stmt.next()?;
    Ok(())
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

fn last_insert_rowid(conn: &Connection) -> Result<i64> {
    let mut stmt = conn.prepare("SELECT last_insert_rowid()")?;
    if let Ok(State::Row) = stmt.next() {
        let id: i64 = stmt.read(0)?;
        return Ok(id);
    }
    Err(anyhow!("last_insert_rowid unavailable"))
}
