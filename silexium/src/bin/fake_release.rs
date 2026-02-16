use anyhow::{anyhow, Context, Result};
use blake3::Hash;
use chrono::{DateTime, Duration, Utc};
use clap::Parser;
use ed25519_dalek::{Signature, SigningKey, Signer};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(name = "fake_release", about = "Generate a fake Silexium release fixture")]
struct Args {
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value = "uvd")]
    package: String,
    #[arg(long, default_value = "0.0.0")]
    version: String,
    #[arg(long, default_value = "stable")]
    channel: String,
    #[arg(long, default_value = "MIT")]
    license: String,
    #[arg(long, default_value = "linux")]
    os: String,
    #[arg(long, default_value = "x86_64")]
    arch: String,
    #[arg(long)]
    created_at: Option<String>,
    #[arg(long)]
    seed: Option<String>,
}

#[derive(Serialize)]
struct Manifest {
    schema_version: u32,
    package: String,
    version: String,
    channel: String,
    created_at: String,
    license: String,
    hash_algo: String,
    artifacts: Vec<Artifact>,
    src_index: SrcIndex,
}

#[derive(Serialize)]
struct SrcIndex {
    path: String,
    size: i64,
    blake3: String,
}

#[derive(Serialize)]
struct Artifact {
    #[serde(rename = "type")]
    artifact_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arch: Option<String>,
    size: i64,
    blake3: String,
    url: String,
}

#[derive(Serialize)]
struct AuthorPayload {
    schema_version: u32,
    package: String,
    version: String,
    channel: String,
    manifest_hash: String,
    src_index_hash: String,
    source_artifact_hash: String,
    license: String,
}

#[derive(Serialize)]
struct TestsPayload {
    schema_version: u32,
    author_attestation_hash: String,
    manifest_hash: String,
    test_suite_id: String,
    test_result: TestResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    test_report_hash: Option<String>,
}

#[derive(Serialize)]
struct ServerPayload {
    schema_version: u32,
    author_attestation_hash: String,
    tests_attestation_hash: String,
    manifest_hash: String,
    binary_artifact_hashes: Vec<String>,
    source_artifact_hash: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum TestResult {
    Pass,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let created_at = args
        .created_at
        .clone()
        .unwrap_or_else(|| Utc::now().to_rfc3339());
    let created_dt = DateTime::parse_from_rfc3339(&created_at)
        .map_err(|_| anyhow!("created_at must be RFC3339"))?;
    let tests_created_at = (created_dt + Duration::seconds(10)).to_rfc3339();
    let server_created_at = (created_dt + Duration::seconds(20)).to_rfc3339();
    let seed_base = args.seed.clone().unwrap_or_else(|| created_at.clone());

    prepare_out_dir(&args.out)?;

    let payload_dir = args.out.join("payloads");
    let tsa_dir = args.out.join("tsa");
    let ots_dir = args.out.join("ots");
    let keys_dir = args.out.join("keys");
    let artifacts_dir = args.out.join("artifacts");
    let source_dir = args.out.join("source");
    fs::create_dir_all(&payload_dir)?;
    fs::create_dir_all(&tsa_dir)?;
    fs::create_dir_all(&ots_dir)?;
    fs::create_dir_all(&keys_dir)?;
    fs::create_dir_all(&artifacts_dir)?;
    fs::create_dir_all(&source_dir)?;

    let (bin_path, bin_size, bin_hash) =
        write_artifact(&artifacts_dir, &args.package, &args.os, &args.arch, b"fake-binary\n")?;
    let (src_path, src_size, src_hash) =
        write_source_archive(&artifacts_dir, &args.package, b"fake-source\n")?;
    let bin_name = file_name_string(&bin_path)?;
    let src_name = file_name_string(&src_path)?;

    let (src_index_size, src_index_hash) = write_src_index(&args.out, &source_dir)?;

    let manifest = Manifest {
        schema_version: 1,
        package: args.package.clone(),
        version: args.version.clone(),
        channel: args.channel.clone(),
        created_at: created_at.clone(),
        license: args.license.clone(),
        hash_algo: "blake3".to_string(),
        artifacts: vec![
            Artifact {
                artifact_type: "binary".to_string(),
                os: Some(args.os.clone()),
                arch: Some(args.arch.clone()),
                size: bin_size,
                blake3: bin_hash.clone(),
                url: format!(
                    "https://example.invalid/{}/{}/{}",
                    args.package, args.version, bin_name
                ),
            },
            Artifact {
                artifact_type: "source".to_string(),
                os: None,
                arch: None,
                size: src_size,
                blake3: src_hash.clone(),
                url: format!(
                    "https://example.invalid/{}/{}/{}",
                    args.package, args.version, src_name
                ),
            },
        ],
        src_index: SrcIndex {
            path: "SRC".to_string(),
            size: src_index_size,
            blake3: src_index_hash.clone(),
        },
    };

    let manifest_bytes = serde_jcs::to_vec(&manifest)?;
    let manifest_hash = blake3_hex(&manifest_bytes);
    fs::write(args.out.join("manifest.json"), &manifest_bytes)?;

    let author_key = signing_key_for(&seed_base, "author");
    let tests_key = signing_key_for(&seed_base, "tests");
    let server_key = signing_key_for(&seed_base, "server");
    let author_key_id = write_public_key(&keys_dir, "author", &author_key)?;
    let tests_key_id = write_public_key(&keys_dir, "tests", &tests_key)?;
    let server_key_id = write_public_key(&keys_dir, "server", &server_key)?;

    let author_payload = AuthorPayload {
        schema_version: 1,
        package: args.package.clone(),
        version: args.version.clone(),
        channel: args.channel.clone(),
        manifest_hash: manifest_hash.clone(),
        src_index_hash: src_index_hash.clone(),
        source_artifact_hash: src_hash.clone(),
        license: args.license.clone(),
    };
    let author_payload_bytes = serde_jcs::to_vec(&author_payload)?;
    let author_payload_hash = blake3_hex(&author_payload_bytes);
    fs::write(payload_dir.join("author.json"), &author_payload_bytes)?;
    let author_sig = sign_payload(&author_key, &author_payload_hash);
    let (author_tsa, author_ots) = write_mock_proofs(&tsa_dir, &ots_dir, "author")?;
    let author_att_hash = attestation_hash(
        "author",
        &author_key_id,
        &author_payload_hash,
        &author_sig,
        &created_at,
        &author_tsa,
        &author_ots,
    );

    let tests_payload = TestsPayload {
        schema_version: 1,
        author_attestation_hash: author_att_hash.clone(),
        manifest_hash: manifest_hash.clone(),
        test_suite_id: "fake-suite".to_string(),
        test_result: TestResult::Pass,
        test_report_hash: None,
    };
    let tests_payload_bytes = serde_jcs::to_vec(&tests_payload)?;
    let tests_payload_hash = blake3_hex(&tests_payload_bytes);
    fs::write(payload_dir.join("tests.json"), &tests_payload_bytes)?;
    let tests_sig = sign_payload(&tests_key, &tests_payload_hash);
    let (tests_tsa, tests_ots) = write_mock_proofs(&tsa_dir, &ots_dir, "tests")?;
    let tests_att_hash = attestation_hash(
        "tests",
        &tests_key_id,
        &tests_payload_hash,
        &tests_sig,
        &tests_created_at,
        &tests_tsa,
        &tests_ots,
    );

    let server_payload = ServerPayload {
        schema_version: 1,
        author_attestation_hash: author_att_hash.clone(),
        tests_attestation_hash: tests_att_hash.clone(),
        manifest_hash: manifest_hash.clone(),
        binary_artifact_hashes: vec![bin_hash.clone()],
        source_artifact_hash: src_hash.clone(),
    };
    let server_payload_bytes = serde_jcs::to_vec(&server_payload)?;
    let server_payload_hash = blake3_hex(&server_payload_bytes);
    fs::write(payload_dir.join("server.json"), &server_payload_bytes)?;
    let server_sig = sign_payload(&server_key, &server_payload_hash);
    let _ = write_mock_proofs(&tsa_dir, &ots_dir, "server")?;

    let release_toml = render_release_toml(
        &args,
        &created_at,
        &manifest_hash,
        src_index_size,
        &src_index_hash,
        bin_size,
        &bin_hash,
        &bin_name,
        src_size,
        &src_hash,
        &src_name,
        &author_key_id,
        &author_payload_hash,
        &author_sig,
        &created_at,
        &tests_key_id,
        &tests_payload_hash,
        &tests_sig,
        &tests_created_at,
        &server_key_id,
        &server_payload_hash,
        &server_sig,
        &server_created_at,
    );
    fs::write(args.out.join("release.toml"), release_toml)?;

    write_ingest_script(&args.out, &created_at, &author_key_id, &tests_key_id, &server_key_id)?;

    println!("fake release generated at {}", args.out.display());
    println!("manifest_hash={manifest_hash}");
    println!("author_key_id={author_key_id}");
    println!("tests_key_id={tests_key_id}");
    println!("server_key_id={server_key_id}");
    println!("next: run {}/ingest.sh", args.out.display());
    Ok(())
}

fn prepare_out_dir(out: &Path) -> Result<()> {
    if out.exists() {
        let mut entries = out.read_dir().context("read output directory")?;
        if entries.next().is_some() {
            return Err(anyhow!("output directory is not empty"));
        }
    } else {
        fs::create_dir_all(out)?;
    }
    Ok(())
}

fn write_artifact(
    artifacts_dir: &Path,
    package: &str,
    os: &str,
    arch: &str,
    contents: &[u8],
) -> Result<(PathBuf, i64, String)> {
    let file_name = format!("{package}-{os}-{arch}");
    let path = artifacts_dir.join(file_name);
    fs::write(&path, contents)?;
    let bytes = fs::read(&path)?;
    let size = bytes.len() as i64;
    let hash = blake3_hex(&bytes);
    Ok((path, size, hash))
}

fn write_source_archive(
    artifacts_dir: &Path,
    package: &str,
    contents: &[u8],
) -> Result<(PathBuf, i64, String)> {
    let file_name = format!("{package}.uvd");
    let path = artifacts_dir.join(file_name);
    fs::write(&path, contents)?;
    let bytes = fs::read(&path)?;
    let size = bytes.len() as i64;
    let hash = blake3_hex(&bytes);
    Ok((path, size, hash))
}

fn file_name_string(path: &Path) -> Result<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .ok_or_else(|| anyhow!("missing file name for {}", path.display()))
}

fn write_src_index(out: &Path, source_dir: &Path) -> Result<(i64, String)> {
    let hello_path = source_dir.join("hello.txt");
    fs::write(&hello_path, b"hello\n")?;
    let bytes = fs::read(&hello_path)?;
    let size = bytes.len();
    let hash = blake3_hex(&bytes);
    let src_contents = format!("hello.txt\t{size}\t{hash}\n");
    let src_path = out.join("SRC");
    fs::write(&src_path, src_contents.as_bytes())?;
    let src_bytes = fs::read(&src_path)?;
    let src_size = src_bytes.len() as i64;
    let src_hash = blake3_hex(&src_bytes);
    Ok((src_size, src_hash))
}

fn signing_key_for(seed: &str, role: &str) -> SigningKey {
    let mut bytes = [0u8; 32];
    let hash = blake3::hash(format!("silexium-fake-release:{seed}:{role}").as_bytes());
    bytes.copy_from_slice(hash.as_bytes());
    SigningKey::from_bytes(&bytes)
}

fn write_public_key(dir: &Path, role: &str, key: &SigningKey) -> Result<String> {
    let verifying = key.verifying_key();
    let pub_bytes = verifying.to_bytes();
    let key_id = hex::encode(pub_bytes);
    let path = dir.join(format!("{role}.pub"));
    fs::write(path, pub_bytes)?;
    Ok(key_id)
}

fn sign_payload(key: &SigningKey, payload_hash: &str) -> String {
    let signature: Signature = key.sign(payload_hash.as_bytes());
    hex::encode(signature.to_bytes())
}

fn write_mock_proofs(
    tsa_dir: &Path,
    ots_dir: &Path,
    kind: &str,
) -> Result<(String, String)> {
    let tsa_bytes = format!("tsa-{kind}\n").into_bytes();
    let ots_bytes = format!("ots-{kind}\n").into_bytes();
    fs::write(tsa_dir.join(format!("{kind}.tsr")), &tsa_bytes)?;
    fs::write(ots_dir.join(format!("{kind}.ots")), &ots_bytes)?;
    Ok((hex::encode(tsa_bytes), hex::encode(ots_bytes)))
}

fn attestation_hash(
    kind: &str,
    key_id: &str,
    payload_hash: &str,
    signature: &str,
    created_at: &str,
    tsa_hex: &str,
    ots_hex: &str,
) -> String {
    let data = format!(
        "SILEXIUM-ATTESTATION\n{kind}\n{key_id}\n{payload_hash}\n{signature}\n{created_at}\n{tsa_hex}\n{ots_hex}\n"
    );
    blake3_hex(data.as_bytes())
}

fn blake3_hex(bytes: &[u8]) -> String {
    let hash: Hash = blake3::hash(bytes);
    hash.to_hex().to_string()
}

#[allow(clippy::too_many_arguments)]
fn render_release_toml(
    args: &Args,
    created_at: &str,
    manifest_hash: &str,
    src_index_size: i64,
    src_index_hash: &str,
    bin_size: i64,
    bin_hash: &str,
    bin_name: &str,
    src_size: i64,
    src_hash: &str,
    src_name: &str,
    author_key_id: &str,
    author_payload_hash: &str,
    author_sig: &str,
    author_created_at: &str,
    tests_key_id: &str,
    tests_payload_hash: &str,
    tests_sig: &str,
    tests_created_at: &str,
    server_key_id: &str,
    server_payload_hash: &str,
    server_sig: &str,
    server_created_at: &str,
) -> String {
    format!(
        r#"[release]
package = "{package}"
version = "{version}"
channel = "{channel}"
created_at = "{created_at}"

[package]
description = "Fake package for Silexium ingest"
license = "{license}"
homepage = "https://example.invalid/{package}"

[manifest]
format = "json"
path = "manifest.json"
blake3 = "{manifest_hash}"
src_index_size = {src_index_size}
src_index_blake3 = "{src_index_hash}"

[[artifacts]]
kind = "binary"
os = "{os}"
arch = "{arch}"
size = {bin_size}
blake3 = "{bin_hash}"
url = "https://example.invalid/{package}/{version}/{bin_name}"

[[artifacts]]
kind = "source"
size = {src_size}
blake3 = "{src_hash}"
url = "https://example.invalid/{package}/{version}/{src_name}"

[[attestations]]
kind = "author"
key_id = "{author_key_id}"
payload_hash = "{author_payload_hash}"
signature = "{author_sig}"
created_at = "{author_created_at}"
payload_path = "payloads/author.json"
tsa_proof_path = "tsa/author.tsr"
ots_proof_path = "ots/author.ots"

[[attestations]]
kind = "tests"
key_id = "{tests_key_id}"
payload_hash = "{tests_payload_hash}"
signature = "{tests_sig}"
created_at = "{tests_created_at}"
payload_path = "payloads/tests.json"
tsa_proof_path = "tsa/tests.tsr"
ots_proof_path = "ots/tests.ots"

[[attestations]]
kind = "server"
key_id = "{server_key_id}"
payload_hash = "{server_payload_hash}"
signature = "{server_sig}"
created_at = "{server_created_at}"
payload_path = "payloads/server.json"
tsa_proof_path = "tsa/server.tsr"
ots_proof_path = "ots/server.ots"
"#,
        package = args.package,
        version = args.version,
        channel = args.channel,
        created_at = created_at,
        license = args.license,
        manifest_hash = manifest_hash,
        src_index_size = src_index_size,
        src_index_hash = src_index_hash,
        os = args.os,
        arch = args.arch,
        bin_size = bin_size,
        bin_hash = bin_hash,
        bin_name = bin_name,
        src_size = src_size,
        src_hash = src_hash,
        src_name = src_name,
        author_key_id = author_key_id,
        author_payload_hash = author_payload_hash,
        author_sig = author_sig,
        author_created_at = author_created_at,
        tests_key_id = tests_key_id,
        tests_payload_hash = tests_payload_hash,
        tests_sig = tests_sig,
        tests_created_at = tests_created_at,
        server_key_id = server_key_id,
        server_payload_hash = server_payload_hash,
        server_sig = server_sig,
        server_created_at = server_created_at,
    )
}

fn write_ingest_script(
    out: &Path,
    created_at: &str,
    author_key_id: &str,
    tests_key_id: &str,
    server_key_id: &str,
) -> Result<()> {
    let script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
script_dir="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
cd "${{script_dir}}"

db_path="${{script_dir}}/silexium.db"
expires_at="2099-01-01T00:00:00Z"

silexium key add --db "${{db_path}}" --role author --key keys/author.pub --key-id {author_key_id} --expires-at "${{expires_at}}"
silexium key add --db "${{db_path}}" --role tests --key keys/tests.pub --key-id {tests_key_id} --expires-at "${{expires_at}}"
silexium key add --db "${{db_path}}" --role server --key keys/server.pub --key-id {server_key_id} --expires-at "${{expires_at}}"

SILEXIUM_SKIP_PROOF_VERIFY=1 silexium ingest --db "${{db_path}}" --file release.toml

echo "ingest ok (created_at {created_at})"
"#
    );
    fs::write(out.join("ingest.sh"), script)?;
    Ok(())
}
