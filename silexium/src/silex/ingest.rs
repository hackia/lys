use crate::canon;
use crate::silex::db;
use crate::silex::proofs;
use crate::silex::types::{ArtifactType, AuthorPayload, Manifest, ServerPayload, TestsPayload};
use anyhow::{Context, Result, anyhow};
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
    pub payload_path: PathBuf,
    pub tsa_proof_path: PathBuf,
    pub ots_proof_path: PathBuf,
}

#[derive(Debug)]
struct LoadedAttestation {
    pub attestation: db::AttestationData,
    pub payload: Payload,
}

#[derive(Debug)]
enum Payload {
    Author(AuthorPayload),
    Tests(TestsPayload),
    Server(ServerPayload),
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
    let manifest = load_manifest(&manifest_path)?;
    validate_manifest(&manifest)?;
    let manifest_bytes = canon::jcs_bytes(&manifest)?;
    let manifest_hash = canon::blake3_hex(&manifest_bytes);
    if let Some(expected) = &release.manifest.blake3 {
        if expected != &manifest_hash {
            return Err(anyhow!("manifest blake3 mismatch"));
        }
    }

    let source_artifact_hash = manifest_source_hash(&manifest)?;
    let binary_hashes = manifest_binary_hashes(&manifest);

    let artifacts = normalize_artifacts(&release.artifacts)?;
    let manifest_artifacts = manifest_to_artifacts(&manifest)?;
    ensure_artifacts_match(&manifest_artifacts, &artifacts)?;

    let src_index_size = match release.manifest.src_index_size {
        Some(size) => {
            if size != manifest.src_index.size {
                return Err(anyhow!("src_index_size mismatch with manifest"));
            }
            size
        }
        None => manifest.src_index.size,
    };
    let src_index_blake3 = match &release.manifest.src_index_blake3 {
        Some(hash) => {
            if hash != &manifest.src_index.blake3 {
                return Err(anyhow!("src_index_blake3 mismatch with manifest"));
            }
            hash.clone()
        }
        None => manifest.src_index.blake3.clone(),
    };

    let loaded_attestations = load_attestations(base, &release.attestations)?;

    for att in &loaded_attestations {
        let att = &att.attestation;
        let key = db::fetch_key(conn, &att.key_id)
            .with_context(|| format!("attestation key not found: {}", att.key_id))?;
        if key.revoked || key.revoked_at.is_some() {
            return Err(anyhow!("attestation key revoked: {}", att.key_id));
        }
        let expires_at = key
            .expires_at
            .as_deref()
            .ok_or_else(|| anyhow!("attestation key missing expires_at: {}", att.key_id))?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| anyhow!("attestation key expires_at invalid: {}", att.key_id))?;
        if chrono::Utc::now() > expires_at.with_timezone(&chrono::Utc) {
            return Err(anyhow!("attestation key expired: {}", att.key_id));
        }
        if key.role != att.kind {
            return Err(anyhow!("attestation role mismatch for {}", att.key_id));
        }
    }

    let attestations: Vec<db::AttestationData> = loaded_attestations
        .iter()
        .map(|item| item.attestation.clone())
        .collect();

    let (author_hash, tests_hash, server_hash) = compute_attestation_hashes(&attestations)?;
    validate_attestation_payloads(
        &loaded_attestations,
        &manifest,
        &manifest_hash,
        &source_artifact_hash,
        &binary_hashes,
        &author_hash,
        &tests_hash,
        &server_hash,
    )?;
    let entry_hash = canon::entry_hash(&manifest_hash, &author_hash, &tests_hash, &server_hash);

    conn.execute("BEGIN")?;
    let result: Result<()> = (|| {
        let package_id = db::upsert_package(
            conn,
            package_name,
            release
                .package
                .as_ref()
                .and_then(|p| p.description.as_deref()),
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
            Some(src_index_size),
            Some(src_index_blake3.as_str()),
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

fn load_attestations(
    base: &Path,
    entries: &[AttestationSection],
) -> Result<Vec<LoadedAttestation>> {
    let mut out = Vec::new();
    for att in entries {
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

        let payload_path = resolve_path(base, &att.payload_path);
        let payload_bytes =
            fs::read(&payload_path).with_context(|| format!("read {}", payload_path.display()))?;

        let payload = match kind {
            "author" => {
                let parsed: AuthorPayload = serde_json::from_slice(&payload_bytes)
                    .with_context(|| format!("parse {}", payload_path.display()))?;
                Payload::Author(parsed)
            }
            "tests" => {
                let parsed: TestsPayload = serde_json::from_slice(&payload_bytes)
                    .with_context(|| format!("parse {}", payload_path.display()))?;
                Payload::Tests(parsed)
            }
            "server" => {
                let parsed: ServerPayload = serde_json::from_slice(&payload_bytes)
                    .with_context(|| format!("parse {}", payload_path.display()))?;
                Payload::Server(parsed)
            }
            _ => return Err(anyhow!("invalid attestation kind")),
        };

        let payload_hash = match &payload {
            Payload::Author(value) => canon::jcs_blake3_hex(value)?,
            Payload::Tests(value) => canon::jcs_blake3_hex(value)?,
            Payload::Server(value) => canon::jcs_blake3_hex(value)?,
        };
        if payload_hash != att.payload_hash.trim() {
            return Err(anyhow!("payload_hash mismatch for {}", kind));
        }

        let tsa_path = resolve_path(base, &att.tsa_proof_path);
        let ots_path = resolve_path(base, &att.ots_proof_path);
        let tsa_proof =
            fs::read(&tsa_path).with_context(|| format!("read {}", tsa_path.display()))?;
        let ots_proof =
            fs::read(&ots_path).with_context(|| format!("read {}", ots_path.display()))?;
        if tsa_proof.is_empty() || ots_proof.is_empty() {
            return Err(anyhow!("timestamp proofs must not be empty"));
        }
        proofs::verify_proofs(&payload_hash, &tsa_proof, &ots_proof)
            .with_context(|| format!("verify {kind} timestamp proofs"))?;

        out.push(LoadedAttestation {
            attestation: db::AttestationData {
                kind: kind.to_string(),
                key_id: att.key_id.trim().to_string(),
                payload_hash,
                signature: att.signature.trim().to_string(),
                created_at: att.created_at.trim().to_string(),
                tsa_proof,
                ots_proof,
            },
            payload,
        });
    }
    Ok(out)
}

fn compute_attestation_hashes(
    attestations: &[db::AttestationData],
) -> Result<(String, String, String)> {
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

fn load_manifest(path: &Path) -> Result<Manifest> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {}", path.display()))?;
    Ok(manifest)
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.schema_version != 1 {
        return Err(anyhow!("manifest schema_version must be 1"));
    }
    if manifest.hash_algo != "blake3" {
        return Err(anyhow!("manifest hash_algo must be blake3"));
    }
    let mut source_count = 0;
    let mut binary_count = 0;
    for artifact in &manifest.artifacts {
        match artifact.artifact_type {
            ArtifactType::Source => source_count += 1,
            ArtifactType::Binary => binary_count += 1,
        }
    }
    if source_count != 1 {
        return Err(anyhow!("manifest must include exactly one source artifact"));
    }
    if binary_count == 0 {
        return Err(anyhow!(
            "manifest must include at least one binary artifact"
        ));
    }
    Ok(())
}

fn manifest_source_hash(manifest: &Manifest) -> Result<String> {
    for artifact in &manifest.artifacts {
        if artifact.artifact_type == ArtifactType::Source {
            return Ok(artifact.blake3.clone());
        }
    }
    Err(anyhow!("manifest source artifact missing"))
}

fn manifest_binary_hashes(manifest: &Manifest) -> Vec<String> {
    let mut hashes: Vec<String> = manifest
        .artifacts
        .iter()
        .filter(|a| a.artifact_type == ArtifactType::Binary)
        .map(|a| a.blake3.clone())
        .collect();
    hashes.sort();
    hashes
}

fn manifest_to_artifacts(manifest: &Manifest) -> Result<Vec<db::ArtifactData>> {
    let mut out = Vec::new();
    for artifact in &manifest.artifacts {
        match artifact.artifact_type {
            ArtifactType::Binary => {
                if artifact.os.as_deref().unwrap_or("").is_empty()
                    || artifact.arch.as_deref().unwrap_or("").is_empty()
                {
                    return Err(anyhow!("manifest binary artifact missing os/arch"));
                }
            }
            ArtifactType::Source => {
                if artifact.os.is_some() || artifact.arch.is_some() {
                    return Err(anyhow!("manifest source artifact must not include os/arch"));
                }
            }
        }
        out.push(db::ArtifactData {
            kind: match artifact.artifact_type {
                ArtifactType::Binary => "binary".to_string(),
                ArtifactType::Source => "source".to_string(),
            },
            os: artifact.os.clone(),
            arch: artifact.arch.clone(),
            size: artifact.size,
            blake3: artifact.blake3.clone(),
            url: artifact.url.clone(),
        });
    }
    Ok(out)
}

fn ensure_artifacts_match(
    manifest_artifacts: &[db::ArtifactData],
    release_artifacts: &[db::ArtifactData],
) -> Result<()> {
    let manifest_set = artifact_index(manifest_artifacts)?;
    let release_set = artifact_index(release_artifacts)?;
    if manifest_set != release_set {
        return Err(anyhow!("manifest artifacts do not match release artifacts"));
    }
    Ok(())
}

fn artifact_index(artifacts: &[db::ArtifactData]) -> Result<std::collections::BTreeSet<String>> {
    let mut set = std::collections::BTreeSet::new();
    for artifact in artifacts {
        let key = format!(
            "{}|{}|{}|{}|{}|{}",
            artifact.kind,
            artifact.os.as_deref().unwrap_or("-"),
            artifact.arch.as_deref().unwrap_or("-"),
            artifact.size,
            artifact.blake3,
            artifact.url
        );
        if !set.insert(key) {
            return Err(anyhow!("duplicate artifact entry"));
        }
    }
    Ok(set)
}

fn validate_attestation_payloads(
    attestations: &[LoadedAttestation],
    manifest: &Manifest,
    manifest_hash: &str,
    source_artifact_hash: &str,
    binary_hashes: &[String],
    author_attestation_hash: &str,
    tests_attestation_hash: &str,
    server_attestation_hash: &str,
) -> Result<()> {
    for item in attestations {
        match &item.payload {
            Payload::Author(payload) => {
                if payload.schema_version != 1 {
                    return Err(anyhow!("author payload schema_version must be 1"));
                }
                if payload.manifest_hash != manifest_hash {
                    return Err(anyhow!("author payload manifest_hash mismatch"));
                }
                if payload.package != manifest.package
                    || payload.version != manifest.version
                    || payload.channel != manifest.channel
                {
                    return Err(anyhow!("author payload package/version/channel mismatch"));
                }
                if payload.license != manifest.license {
                    return Err(anyhow!("author payload license mismatch"));
                }
                if payload.src_index_hash != manifest.src_index.blake3 {
                    return Err(anyhow!("author payload src_index_hash mismatch"));
                }
                if payload.source_artifact_hash != source_artifact_hash {
                    return Err(anyhow!("author payload source_artifact_hash mismatch"));
                }
            }
            Payload::Tests(payload) => {
                if payload.schema_version != 1 {
                    return Err(anyhow!("tests payload schema_version must be 1"));
                }
                if payload.manifest_hash != manifest_hash {
                    return Err(anyhow!("tests payload manifest_hash mismatch"));
                }
                if payload.author_attestation_hash != author_attestation_hash {
                    return Err(anyhow!("tests payload author_attestation_hash mismatch"));
                }
            }
            Payload::Server(payload) => {
                if payload.schema_version != 1 {
                    return Err(anyhow!("server payload schema_version must be 1"));
                }
                if payload.manifest_hash != manifest_hash {
                    return Err(anyhow!("server payload manifest_hash mismatch"));
                }
                if payload.author_attestation_hash != author_attestation_hash {
                    return Err(anyhow!("server payload author_attestation_hash mismatch"));
                }
                if payload.tests_attestation_hash != tests_attestation_hash {
                    return Err(anyhow!("server payload tests_attestation_hash mismatch"));
                }
                if payload.source_artifact_hash != source_artifact_hash {
                    return Err(anyhow!("server payload source_artifact_hash mismatch"));
                }
                let mut payload_binary_hashes = payload.binary_artifact_hashes.clone();
                payload_binary_hashes.sort();
                if payload_binary_hashes != binary_hashes {
                    return Err(anyhow!("server payload binary_artifact_hashes mismatch"));
                }
                if server_attestation_hash.is_empty() {
                    return Err(anyhow!("server attestation hash missing"));
                }
            }
        }
    }
    Ok(())
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
