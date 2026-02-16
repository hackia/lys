pub mod silex;
use anyhow::{Context, Result, anyhow};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use clap::Parser;
use ed25519_dalek::SigningKey;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;

use crate::silex::{config, db, log, types};
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

    validate_attestations(&release.attestations)?;

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

fn validate_attestations(attestations: &[db::AttestationData]) -> Result<(), ApiError> {
    let mut has_author = false;
    let mut has_tests = false;
    let mut has_server = false;
    for att in attestations {
        if att.tsa_proof.is_empty() || att.ots_proof.is_empty() {
            return Err(ApiError::internal("missing timestamp proofs"));
        }
        match att.kind.as_str() {
            "author" => has_author = true,
            "tests" => has_tests = true,
            "server" => has_server = true,
            _ => {}
        }
    }
    if !has_author || !has_tests || !has_server {
        return Err(ApiError::internal("missing required attestations"));
    }
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
        if let Some(server_known) = db::load_sth(conn, known.tree_size)? {
            if server_known.root_hash != known.root_hash {
                return Err(anyhow!("known_sth root mismatch"));
            }
        }
    }

    let inclusion = log::inclusion_proof(&leaf_hashes, leaf_index)?;
    let leaf_hash = log::leaf_hash(&leaf_hashes[leaf_index]);

    let consistency = if let Some(known) = known_sth {
        if known.tree_size == tree_size {
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
    let trimmed = String::from_utf8(bytes).context("server key is not valid utf-8 or raw bytes")?;
    let trimmed = trimmed.trim();
    let decoded = hex::decode(trimmed).context("server key hex decode failed")?;
    if decoded.len() != 32 {
        return Err(anyhow!("server key must be 32 bytes or 64 hex chars"));
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
