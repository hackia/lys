use crate::canon;
use crate::silex::db;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::Deserialize;
use sqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct ReleaseFile {
    pub release: ReleaseSection,
    pub package: Option<PackageSection>,
    pub manifest: ManifestSection,
    pub artifacts: Vec<ArtifactSection>,
    pub attestations: Vec<AttestationSection>,
}

#[derive(Debug, Deserialize)]
pub struct ReleaseSection {
    pub package: String,
    pub version: String,
    pub channel: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PackageSection {
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestSection {
    pub format: String,
    pub path: PathBuf,
    pub blake3: Option<String>,
    pub src_index_size: Option<i64>,
    pub src_index_blake3: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArtifactSection {
    pub kind: String,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub size: i64,
    pub blake3: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct AttestationSection {
    pub kind: String,
    pub key_id: String,
    pub payload_hash: String,
    pub signature: String,
    pub created_at: String,
    pub tsa_proof_path: PathBuf,
    pub ots_proof_path: PathBuf,
}

pub fn load_release(path: &Path) -> Result<ReleaseFile> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let release: ReleaseFile = toml::from_str(&contents).context("parse release toml")?;
    Ok(release)
}

pub fn ingest_release(conn: &Connection, path: &Path) -> Result<()> {
    let release = load_release(path)?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));

    let package_name = release.release.package.trim();
    if package_name.is_empty() {
        return Err(anyhow!("release.package is required"));
    }
    let version = release.release.version.trim();
    if version.is_empty() {
        return Err(anyhow!("release.version is required"));
    }
    let channel = release
        .release
        .channel
        .as_deref()
        .unwrap_or("stable")
        .trim();
    if channel.is_empty() {
        return Err(anyhow!("release.channel is required"));
    }
    let created_at = match &release.release.created_at {
        Some(value) if value.trim().is_empty() => {
            return Err(anyhow!("release.created_at is empty"));
        }
        Some(value) => value.clone(),
        None => Utc::now().to_rfc3339(),
    };

    let manifest_path = resolve_path(base, &release.manifest.path);
    if release.manifest.format.trim().is_empty() {
        return Err(anyhow!("manifest.format is required"));
    }
    let manifest_bytes =
        fs::read(&manifest_path).with_context(|| format!("read {}", manifest_path.display()))?;
    let manifest_hash = canon::blake3_hex(&manifest_bytes);
    if let Some(expected) = &release.manifest.blake3 {
        if expected != &manifest_hash {
            return Err(anyhow!("manifest blake3 mismatch"));
        }
    }

    let artifacts = normalize_artifacts(&release.artifacts)?;
    let attestations = load_attestations(base, &release.attestations)?;

    for att in &attestations {
        let key = db::fetch_key(conn, &att.key_id)
            .with_context(|| format!("attestation key not found: {}", att.key_id))?;
        if key.revoked {
            return Err(anyhow!("attestation key revoked: {}", att.key_id));
        }
        if key.role != att.kind {
            return Err(anyhow!("attestation role mismatch for {}", att.key_id));
        }
    }

    let (author_hash, tests_hash, server_hash) = compute_attestation_hashes(&attestations)?;
    let entry_hash = canon::entry_hash(&manifest_hash, &author_hash, &tests_hash, &server_hash);

    conn.execute("BEGIN")?;
    let result: Result<()> = (|| {
        let package_id = db::upsert_package(
            conn,
            package_name,
            release.package.as_ref().and_then(|p| p.description.as_deref()),
            release.package.as_ref().and_then(|p| p.license.as_deref()),
            release.package.as_ref().and_then(|p| p.homepage.as_deref()),
            &created_at,
        )?;
        let version_id = db::upsert_version(conn, package_id, version, channel, &created_at)?;
        let manifest_id = db::insert_manifest(
            conn,
            version_id,
            &release.manifest.format,
            &manifest_bytes,
            &manifest_hash,
            release.manifest.src_index_size,
            release.manifest.src_index_blake3.as_deref(),
            &created_at,
        )?;

        for artifact in &artifacts {
            db::insert_artifact(conn, version_id, artifact)?;
        }

        let mut author_id = None;
        let mut tests_id = None;
        let mut server_id = None;
        for att in &attestations {
            let id = db::insert_attestation(conn, version_id, att)?;
            match att.kind.as_str() {
                "author" => author_id = Some(id),
                "tests" => tests_id = Some(id),
                "server" => server_id = Some(id),
                _ => {}
            }
        }

        let author_id = author_id.ok_or_else(|| anyhow!("missing author attestation"))?;
        let tests_id = tests_id.ok_or_else(|| anyhow!("missing tests attestation"))?;
        let server_id = server_id.ok_or_else(|| anyhow!("missing server attestation"))?;

        let seq = db::next_log_seq(conn)?;
        db::insert_log_entry(
            conn,
            seq,
            manifest_id,
            author_id,
            tests_id,
            server_id,
            &entry_hash,
            &created_at,
        )?;
        Ok(())
    })();

    match result {
        Ok(()) => {
            conn.execute("COMMIT")?;
            Ok(())
        }
        Err(err) => {
            let _ = conn.execute("ROLLBACK");
            Err(err)
        }
    }
}

fn normalize_artifacts(artifacts: &[ArtifactSection]) -> Result<Vec<db::ArtifactData>> {
    let mut out = Vec::new();
    let mut has_binary = false;
    let mut has_source = false;
    for artifact in artifacts {
        let kind = artifact.kind.trim();
        if kind.is_empty() {
            return Err(anyhow!("artifact kind is required"));
        }
        match kind {
            "binary" => {
                has_binary = true;
                if artifact.os.as_deref().unwrap_or("").is_empty()
                    || artifact.arch.as_deref().unwrap_or("").is_empty()
                {
                    return Err(anyhow!("binary artifact requires os and arch"));
                }
            }
            "source" => {
                has_source = true;
                if artifact.os.is_some() || artifact.arch.is_some() {
                    return Err(anyhow!("source artifact must not include os/arch"));
                }
            }
            _ => return Err(anyhow!("artifact kind must be binary or source")),
        }
        out.push(db::ArtifactData {
            kind: kind.to_string(),
            os: artifact.os.clone(),
            arch: artifact.arch.clone(),
            size: artifact.size,
            blake3: artifact.blake3.clone(),
            url: artifact.url.clone(),
        });
    }
    if !has_binary {
        return Err(anyhow!("missing binary artifact"));
    }
    if !has_source {
        return Err(anyhow!("missing source artifact"));
    }
    Ok(out)
}

fn load_attestations(base: &Path, entries: &[AttestationSection]) -> Result<Vec<db::AttestationData>> {
    let mut out = Vec::new();
    for att in entries {
        let tsa_path = resolve_path(base, &att.tsa_proof_path);
        let ots_path = resolve_path(base, &att.ots_proof_path);
        let tsa_proof = fs::read(&tsa_path)
            .with_context(|| format!("read {}", tsa_path.display()))?;
        let ots_proof = fs::read(&ots_path)
            .with_context(|| format!("read {}", ots_path.display()))?;

        if tsa_proof.is_empty() || ots_proof.is_empty() {
            return Err(anyhow!("timestamp proofs must not be empty"));
        }

        let kind = att.kind.trim();
        if kind.is_empty()
            || att.key_id.trim().is_empty()
            || att.payload_hash.trim().is_empty()
            || att.signature.trim().is_empty()
        {
            return Err(anyhow!("attestation fields must not be empty"));
        }
        if !matches!(kind, "author" | "tests" | "server") {
            return Err(anyhow!("attestation kind must be author, tests, or server"));
        }
        out.push(db::AttestationData {
            kind: kind.to_string(),
            key_id: att.key_id.trim().to_string(),
            payload_hash: att.payload_hash.trim().to_string(),
            signature: att.signature.trim().to_string(),
            created_at: att.created_at.trim().to_string(),
            tsa_proof,
            ots_proof,
        });
    }
    Ok(out)
}

fn compute_attestation_hashes(attestations: &[db::AttestationData]) -> Result<(String, String, String)> {
    let mut author = None;
    let mut tests = None;
    let mut server = None;
    for att in attestations {
        let tsa_hex = hex::encode(&att.tsa_proof);
        let ots_hex = hex::encode(&att.ots_proof);
        let hash = canon::attestation_hash(
            &att.kind,
            &att.key_id,
            &att.payload_hash,
            &att.signature,
            &att.created_at,
            &tsa_hex,
            &ots_hex,
        );
        match att.kind.as_str() {
            "author" => {
                if author.is_some() {
                    return Err(anyhow!("duplicate author attestation"));
                }
                author = Some(hash);
            }
            "tests" => {
                if tests.is_some() {
                    return Err(anyhow!("duplicate tests attestation"));
                }
                tests = Some(hash);
            }
            "server" => {
                if server.is_some() {
                    return Err(anyhow!("duplicate server attestation"));
                }
                server = Some(hash);
            }
            _ => {}
        }
    }
    let author = author.ok_or_else(|| anyhow!("missing author attestation"))?;
    let tests = tests.ok_or_else(|| anyhow!("missing tests attestation"))?;
    let server = server.ok_or_else(|| anyhow!("missing server attestation"))?;
    Ok((author, tests, server))
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
