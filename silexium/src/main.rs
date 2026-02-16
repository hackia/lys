mod canon;
pub mod silex;
use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use clap::Parser;
use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;

use crate::silex::{config, db, ingest, log, proofs, types};
use silex::config::{Cli, Command, resolve_db_path, resolve_server_key};
use silex::types::{
    AttestationOut, ErrorResponse, InstallRequest, LogProofOut, ManifestOut, ResolveResponse,
    SthOut, UpdateRequest,
};

struct AppState {
    conn: Mutex<sqlite::Connection>,
    server_key: [u8; 32],
    server_key_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => serve(args).await,
        Command::Key(args) => key_command(args),
        Command::Ingest(args) => ingest_command(args),
    }
}

async fn serve(args: config::ServeArgs) -> Result<()> {
    let db_path = resolve_db_path(args.db)?;
    let server_key_path = resolve_server_key(args.server_key)?;
    let server_key = load_key_bytes(&server_key_path)?;
    let server_key_id = derive_key_id(&server_key)?;

    let conn = db::open_db(&db_path)?;
    db::init_db(&conn)?;

    let state = Arc::new(AppState {
        conn: Mutex::new(conn),
        server_key,
        server_key_id,
    });

    let app = Router::new()
        .route("/health", get(health))
        .route("/install", post(install))
        .route("/update", post(update))
        .with_state(state)
        .layer(CorsLayer::very_permissive());

    let bind_addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("failed to bind {bind_addr}"))?;

    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}

async fn health() -> Json<&'static str> {
    Json("OK")
}

async fn install(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InstallRequest>,
) -> Result<Json<ResolveResponse>, ApiError> {
    let response = resolve(
        state,
        req.package,
        req.os,
        req.arch,
        req.version.as_deref(),
        req.channel.as_deref(),
        None,
        req.known_sth,
    )?;
    Ok(Json(response))
}

async fn update(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateRequest>,
) -> Result<Json<ResolveResponse>, ApiError> {
    validate_str(&req.current_version, "current_version")?;
    let response = resolve(
        state,
        req.package,
        req.os,
        req.arch,
        None,
        req.channel.as_deref(),
        Some(req.current_version),
        req.known_sth,
    )?;
    Ok(Json(response))
}

fn resolve(
    state: Arc<AppState>,
    package: String,
    os: String,
    arch: String,
    version: Option<&str>,
    channel: Option<&str>,
    current_version: Option<String>,
    known_sth: Option<types::KnownSth>,
) -> Result<ResolveResponse, ApiError> {
    validate_str(&package, "package")?;
    validate_str(&os, "os")?;
    validate_str(&arch, "arch")?;

    let conn_guard = state
        .conn
        .lock()
        .map_err(|_| ApiError::internal("db lock poisoned"))?;
    let release = db::resolve_release(
        &conn_guard,
        &package,
        &os,
        &arch,
        version,
        channel,
        current_version.as_deref(),
    )
    .map_err(ApiError::from_anyhow)?;

    let log_proof = build_log_proof(
        &conn_guard,
        &state.server_key,
        &state.server_key_id,
        &release.log_entry,
        known_sth,
    )
    .map_err(ApiError::from_anyhow)?;

    validate_manifest_bytes(
        &release.manifest.bytes,
        &release.manifest.blake3,
        release.manifest.src_index_size,
        release.manifest.src_index_blake3.as_deref(),
    )
    .map_err(ApiError::from_anyhow)?;

    validate_attestations(&conn_guard, &release)?;

    let manifest = ManifestOut {
        format: release.manifest.format,
        bytes_hex: hex::encode(release.manifest.bytes),
        blake3: release.manifest.blake3,
        src_index_size: release.manifest.src_index_size,
        src_index_blake3: release.manifest.src_index_blake3,
    };

    let mut attestations: Vec<AttestationOut> = release
        .attestations
        .into_iter()
        .map(|att| AttestationOut {
            kind: att.kind,
            key_id: att.key_id,
            payload_hash: att.payload_hash,
            signature: att.signature,
            created_at: att.created_at,
            tsa_proof_hex: hex::encode(att.tsa_proof),
            ots_proof_hex: hex::encode(att.ots_proof),
        })
        .collect();
    attestations.sort_by(|a, b| a.kind.cmp(&b.kind));

    let artifacts = release
        .artifacts
        .into_iter()
        .map(|artifact| types::ArtifactOut {
            kind: artifact.kind,
            os: artifact.os,
            arch: artifact.arch,
            size: artifact.size,
            blake3: artifact.blake3,
            url: artifact.url,
        })
        .collect();

    Ok(ResolveResponse {
        package,
        version: release.version,
        channel: release.channel,
        os,
        arch,
        up_to_date: release.up_to_date,
        manifest,
        artifacts,
        attestations,
        log: log_proof,
    })
}

fn validate_attestations(
    conn: &sqlite::Connection,
    release: &db::ReleaseData,
) -> Result<(), ApiError> {
    let mut has_author = false;
    let mut has_tests = false;
    let mut has_server = false;

    let manifest_hash = canon::blake3_hex(&release.manifest.bytes);
    if manifest_hash != release.manifest.blake3 {
        return Err(ApiError::internal("manifest blake3 mismatch"));
    }

    let mut author_hash = None;
    let mut tests_hash = None;
    let mut server_hash = None;

    for att in &release.attestations {
        if att.tsa_proof.is_empty() || att.ots_proof.is_empty() {
            return Err(ApiError::internal("missing timestamp proofs"));
        }
        proofs::verify_proofs(&att.payload_hash, &att.tsa_proof, &att.ots_proof)
            .with_context(|| format!("verify {} timestamp proofs", att.kind))
            .map_err(ApiError::from_anyhow)?;
        let key = db::fetch_key(conn, &att.key_id).map_err(ApiError::from_anyhow)?;
        if key.revoked || key.revoked_at.is_some() {
            return Err(ApiError::internal("attestation key revoked"));
        }
        let expires_at = key
            .expires_at
            .as_deref()
            .ok_or_else(|| ApiError::internal("attestation key missing expires_at"))?;
        let expires_at = chrono::DateTime::parse_from_rfc3339(expires_at)
            .map_err(|_| ApiError::internal("attestation key expires_at invalid"))?;
        if chrono::Utc::now() > expires_at.with_timezone(&chrono::Utc) {
            return Err(ApiError::internal("attestation key expired"));
        }
        if key.role != att.kind {
            return Err(ApiError::internal("attestation role mismatch"));
        }
        verify_attestation_signature(&key.public_key, &att.payload_hash, &att.signature)?;
        let tsa_hex = hex::encode(&att.tsa_proof);
        let ots_hex = hex::encode(&att.ots_proof);
        let att_hash = canon::attestation_hash(
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
                if has_author {
                    return Err(ApiError::internal("duplicate author attestation"));
                }
                has_author = true;
                author_hash = Some(att_hash);
            }
            "tests" => {
                if has_tests {
                    return Err(ApiError::internal("duplicate tests attestation"));
                }
                has_tests = true;
                tests_hash = Some(att_hash);
            }
            "server" => {
                if has_server {
                    return Err(ApiError::internal("duplicate server attestation"));
                }
                has_server = true;
                server_hash = Some(att_hash);
            }
            _ => {}
        }
    }
    if !has_author || !has_tests || !has_server {
        return Err(ApiError::internal("missing required attestations"));
    }

    let author_hash =
        author_hash.ok_or_else(|| ApiError::internal("author attestation missing"))?;
    let tests_hash = tests_hash.ok_or_else(|| ApiError::internal("tests attestation missing"))?;
    let server_hash =
        server_hash.ok_or_else(|| ApiError::internal("server attestation missing"))?;
    let expected_entry_hash = canon::entry_hash(
        &release.manifest.blake3,
        &author_hash,
        &tests_hash,
        &server_hash,
    );
    if expected_entry_hash != release.log_entry.entry_hash {
        return Err(ApiError::internal("log entry hash mismatch"));
    }
    Ok(())
}

fn verify_attestation_signature(
    public_key: &[u8],
    payload_hash: &str,
    signature_hex: &str,
) -> Result<(), ApiError> {
    if public_key.len() != 32 {
        return Err(ApiError::internal("invalid public key length"));
    }
    let mut key_bytes = [0u8; 32];
    key_bytes.copy_from_slice(public_key);
    let verifying_key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| ApiError::internal("invalid public key bytes"))?;
    let signature_bytes = hex::decode(signature_hex)
        .map_err(|_| ApiError::bad_request("bad signature hex".into()))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| ApiError::bad_request("bad signature".into()))?;
    verifying_key
        .verify(payload_hash.as_bytes(), &signature)
        .map_err(|_| ApiError::internal("attestation signature mismatch"))?;
    Ok(())
}

fn validate_manifest_bytes(
    bytes: &[u8],
    expected_hash: &str,
    expected_src_size: Option<i64>,
    expected_src_hash: Option<&str>,
) -> Result<types::Manifest> {
    let manifest: types::Manifest =
        serde_json::from_slice(bytes).context("manifest json parse failed")?;
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
            types::ArtifactType::Source => {
                source_count += 1;
                if artifact.os.is_some() || artifact.arch.is_some() {
                    return Err(anyhow!("manifest source artifact must not include os/arch"));
                }
            }
            types::ArtifactType::Binary => {
                binary_count += 1;
                if artifact.os.as_deref().unwrap_or("").is_empty()
                    || artifact.arch.as_deref().unwrap_or("").is_empty()
                {
                    return Err(anyhow!("manifest binary artifact missing os/arch"));
                }
            }
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

    let jcs_bytes = canon::jcs_bytes(&manifest)?;
    if jcs_bytes != bytes {
        return Err(anyhow!("manifest is not JCS canonical"));
    }
    let computed_hash = canon::blake3_hex(&jcs_bytes);
    if computed_hash != expected_hash {
        return Err(anyhow!("manifest blake3 mismatch"));
    }
    if let Some(size) = expected_src_size {
        if manifest.src_index.size != size {
            return Err(anyhow!("manifest src_index.size mismatch"));
        }
    }
    if let Some(hash) = expected_src_hash {
        if manifest.src_index.blake3 != hash {
            return Err(anyhow!("manifest src_index.blake3 mismatch"));
        }
    }
    Ok(manifest)
}

fn key_command(args: config::KeyArgs) -> Result<()> {
    match args.command {
        config::KeyCommand::Add(add) => key_add(args.db, add),
        config::KeyCommand::Revoke(revoke) => key_revoke(args.db, revoke),
        config::KeyCommand::Rotate(rotate) => key_rotate(args.db, rotate),
        config::KeyCommand::List(list) => key_list(args.db, list),
        config::KeyCommand::RevokeExpired => key_revoke_expired(args.db),
    }
}

fn key_add(db_path: Option<std::path::PathBuf>, add: config::KeyAddArgs) -> Result<()> {
    let role = add.role.trim();
    if !matches!(role, "author" | "tests" | "server") {
        return Err(anyhow!("role must be author, tests, or server"));
    }
    let expires_at = add.expires_at.trim();
    if expires_at.is_empty() {
        return Err(anyhow!("expires_at is required"));
    }
    chrono::DateTime::parse_from_rfc3339(expires_at)
        .map_err(|_| anyhow!("expires_at must be RFC3339"))?;
    let db_path = resolve_db_path(db_path)?;
    let conn = db::open_db(&db_path)?;
    db::init_db(&conn)?;

    let key_bytes = load_key_bytes(&add.key)?;
    let key_id = if let Some(key_id) = add.key_id {
        key_id
    } else {
        derive_public_key_id(&key_bytes)?
    };

    db::insert_key(&conn, &key_id, role, &key_bytes, expires_at, None)?;
    Ok(())
}

fn key_revoke(db_path: Option<std::path::PathBuf>, revoke: config::KeyRevokeArgs) -> Result<()> {
    let key_id = revoke.key_id.trim();
    if key_id.is_empty() {
        return Err(anyhow!("key_id is required"));
    }
    let revoked_at = revoke
        .revoked_at
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    chrono::DateTime::parse_from_rfc3339(&revoked_at)
        .map_err(|_| anyhow!("revoked_at must be RFC3339"))?;

    let db_path = resolve_db_path(db_path)?;
    let conn = db::open_db(&db_path)?;
    db::init_db(&conn)?;
    db::revoke_key(&conn, key_id, &revoked_at)?;
    Ok(())
}

fn key_rotate(db_path: Option<std::path::PathBuf>, rotate: config::KeyRotateArgs) -> Result<()> {
    let role = rotate.role.trim();
    if !matches!(role, "author" | "tests" | "server") {
        return Err(anyhow!("role must be author, tests, or server"));
    }
    let expires_at = rotate.expires_at.trim();
    if expires_at.is_empty() {
        return Err(anyhow!("expires_at is required"));
    }
    chrono::DateTime::parse_from_rfc3339(expires_at)
        .map_err(|_| anyhow!("expires_at must be RFC3339"))?;

    let old_key_id = rotate.old_key_id.trim();
    if old_key_id.is_empty() {
        return Err(anyhow!("old_key_id is required"));
    }

    let db_path = resolve_db_path(db_path)?;
    let conn = db::open_db(&db_path)?;
    db::init_db(&conn)?;

    let key_bytes = load_key_bytes(&rotate.new_key)?;
    let new_key_id = if let Some(key_id) = rotate.new_key_id {
        key_id
    } else {
        derive_public_key_id(&key_bytes)?
    };

    conn.execute("BEGIN")?;
    let result: Result<()> = (|| {
        db::insert_key(&conn, &new_key_id, role, &key_bytes, expires_at, None)?;
        db::revoke_key(&conn, old_key_id, &chrono::Utc::now().to_rfc3339())?;
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

fn key_list(db_path: Option<std::path::PathBuf>, list: config::KeyListArgs) -> Result<()> {
    let db_path = resolve_db_path(db_path)?;
    let conn = db::open_db(&db_path)?;
    db::init_db(&conn)?;
    let keys = db::list_keys(&conn)?;
    if list.json {
        #[derive(serde::Serialize)]
        struct KeyOut<'a> {
            key_id: &'a str,
            role: &'a str,
            created_at: &'a str,
            expires_at: Option<&'a str>,
            revoked_at: Option<&'a str>,
            revoked: bool,
            public_key_hex: String,
        }

        let out: Vec<KeyOut<'_>> = keys
            .iter()
            .map(|key| KeyOut {
                key_id: &key.key_id,
                role: &key.role,
                created_at: &key.created_at,
                expires_at: key.expires_at.as_deref(),
                revoked_at: key.revoked_at.as_deref(),
                revoked: key.revoked,
                public_key_hex: hex::encode(&key.public_key),
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        for key in keys {
            let expires_at = key.expires_at.as_deref().unwrap_or("-");
            let revoked_at = key.revoked_at.as_deref().unwrap_or("-");
            println!(
                "{} {} created_at={} expires_at={} revoked_at={} revoked={}",
                key.key_id, key.role, key.created_at, expires_at, revoked_at, key.revoked
            );
        }
    }
    Ok(())
}

fn key_revoke_expired(db_path: Option<std::path::PathBuf>) -> Result<()> {
    let db_path = resolve_db_path(db_path)?;
    let conn = db::open_db(&db_path)?;
    db::init_db(&conn)?;
    let now = chrono::Utc::now().to_rfc3339();
    let count = db::revoke_expired_keys(&conn, &now)?;
    println!("revoked_expired={count}");
    Ok(())
}

fn ingest_command(args: config::IngestArgs) -> Result<()> {
    let db_path = resolve_db_path(args.db)?;
    let conn = db::open_db(&db_path)?;
    db::init_db(&conn)?;
    ingest::ingest_release(&conn, &args.file)?;
    Ok(())
}

fn build_log_proof(
    conn: &sqlite::Connection,
    server_key: &[u8; 32],
    server_key_id: &str,
    entry: &db::LogEntryData,
    known_sth: Option<types::KnownSth>,
) -> Result<LogProofOut> {
    let entries = db::load_log_entries(conn)?;
    let (leaf_index, entry_hash) = entries
        .iter()
        .enumerate()
        .find(|(_, item)| item.seq == entry.seq)
        .map(|(idx, item)| (idx, item.entry_hash.clone()))
        .ok_or_else(|| anyhow!("log entry not found in sequence"))?;

    let leaf_hashes: Vec<log::Hash> = entries
        .iter()
        .map(|item| log::decode_hash(&item.entry_hash))
        .collect::<Result<_>>()?;

    let tree_size = leaf_hashes.len() as u64;
    let root_hash = log::mth(&leaf_hashes);
    let root_hex = log::encode_hash(&root_hash);

    let stored = db::load_sth(conn, tree_size)?;
    let sth = match stored {
        Some(sth) => {
            if sth.root_hash != root_hex {
                return Err(anyhow!("sth root mismatch"));
            }
            sth
        }
        None => {
            let timestamp = chrono::Utc::now().to_rfc3339();
            let payload = log::sth_payload(tree_size, &root_hex, &timestamp);
            let signing_key = SigningKey::from_bytes(server_key);
            let signature = log::sign_sth_payload(&signing_key, &payload);
            let sth = db::StoredSth {
                tree_size,
                root_hash: root_hex.clone(),
                timestamp,
                signature,
                key_id: server_key_id.to_string(),
            };
            db::store_sth(conn, &sth)?;
            sth
        }
    };

    if let Some(known) = &known_sth {
        if known.tree_size > tree_size {
            return Err(anyhow!("known_sth tree_size ahead of server"));
        }
        if known.tree_size > 0 {
            if let Some(server_known) = db::load_sth(conn, known.tree_size)? {
                if server_known.root_hash != known.root_hash {
                    return Err(anyhow!("known_sth root mismatch"));
                }
            }
        }
    }

    let inclusion = log::inclusion_proof(&leaf_hashes, leaf_index)?;
    let leaf_hash = log::leaf_hash(&leaf_hashes[leaf_index]);

    let consistency = if let Some(known) = known_sth {
        if known.tree_size == 0 || known.tree_size == tree_size {
            None
        } else {
            Some(
                log::consistency_proof(&leaf_hashes, known.tree_size as usize, tree_size as usize)?
                    .into_iter()
                    .map(|hash| log::encode_hash(&hash))
                    .collect(),
            )
        }
    } else {
        None
    };

    Ok(LogProofOut {
        tree_size,
        leaf_index: leaf_index as u64,
        entry_hash,
        leaf_hash: log::encode_hash(&leaf_hash),
        inclusion: inclusion
            .into_iter()
            .map(|hash| log::encode_hash(&hash))
            .collect(),
        consistency,
        sth: SthOut {
            tree_size: sth.tree_size,
            root_hash: sth.root_hash,
            timestamp: sth.timestamp,
            signature: sth.signature,
            key_id: sth.key_id,
        },
    })
}

fn validate_str(value: &str, field: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() {
        return Err(ApiError::bad_request(format!("{field} is required")));
    }
    Ok(())
}

fn load_key_bytes(path: &Path) -> Result<[u8; 32]> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() == 32 {
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        return Ok(out);
    }
    let trimmed = String::from_utf8(bytes).context("key is not valid utf-8 or raw bytes")?;
    let trimmed = trimmed.trim();
    let decoded = hex::decode(trimmed).context("key hex decode failed")?;
    if decoded.len() != 32 {
        return Err(anyhow!("key must be 32 bytes or 64 hex chars"));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded);
    Ok(out)
}

fn derive_key_id(secret: &[u8; 32]) -> Result<String> {
    let signing_key = SigningKey::from_bytes(secret);
    let verifying_key = signing_key.verifying_key();
    Ok(hex::encode(verifying_key.as_bytes()))
}

fn derive_public_key_id(public: &[u8; 32]) -> Result<String> {
    Ok(hex::encode(public))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn internal(message: &str) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.to_string(),
        }
    }

    fn from_anyhow(err: anyhow::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let payload = Json(ErrorResponse {
            error: self.message,
        });
        (self.status, payload).into_response()
    }
}
