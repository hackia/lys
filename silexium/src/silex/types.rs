use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct KnownSth {
    pub tree_size: u64,
    pub root_hash: String,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub package: String,
    pub os: String,
    pub arch: String,
    pub version: Option<String>,
    pub channel: Option<String>,
    pub known_sth: Option<KnownSth>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    pub package: String,
    pub os: String,
    pub arch: String,
    pub current_version: String,
    pub channel: Option<String>,
    pub known_sth: Option<KnownSth>,
}

#[derive(Debug, Serialize)]
pub struct ArtifactOut {
    pub kind: String,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub size: i64,
    pub blake3: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct ManifestOut {
    pub format: String,
    pub bytes_hex: String,
    pub blake3: String,
    pub src_index_size: Option<i64>,
    pub src_index_blake3: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AttestationOut {
    pub kind: String,
    pub key_id: String,
    pub payload_hash: String,
    pub signature: String,
    pub created_at: String,
    pub tsa_proof_hex: String,
    pub ots_proof_hex: String,
}

#[derive(Debug, Serialize)]
pub struct SthOut {
    pub tree_size: u64,
    pub root_hash: String,
    pub timestamp: String,
    pub signature: String,
    pub key_id: String,
}

#[derive(Debug, Serialize)]
pub struct LogProofOut {
    pub tree_size: u64,
    pub leaf_index: u64,
    pub entry_hash: String,
    pub leaf_hash: String,
    pub inclusion: Vec<String>,
    pub consistency: Option<Vec<String>>,
    pub sth: SthOut,
}

#[derive(Debug, Serialize)]
pub struct ResolveResponse {
    pub package: String,
    pub version: String,
    pub channel: String,
    pub os: String,
    pub arch: String,
    pub up_to_date: bool,
    pub manifest: ManifestOut,
    pub artifacts: Vec<ArtifactOut>,
    pub attestations: Vec<AttestationOut>,
    pub log: LogProofOut,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub package: String,
    pub version: String,
    pub channel: String,
    pub created_at: String,
    pub license: String,
    pub hash_algo: String,
    pub artifacts: Vec<Artifact>,
    pub src_index: SrcIndex,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SrcIndex {
    pub path: String,
    pub size: i64,
    pub blake3: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Artifact {
    #[serde(rename = "type")]
    pub artifact_type: ArtifactType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<String>,
    pub size: i64,
    pub blake3: String,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactType {
    Binary,
    Source,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthorPayload {
    pub schema_version: u32,
    pub package: String,
    pub version: String,
    pub channel: String,
    pub manifest_hash: String,
    pub src_index_hash: String,
    pub source_artifact_hash: String,
    pub license: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TestsPayload {
    pub schema_version: u32,
    pub author_attestation_hash: String,
    pub manifest_hash: String,
    pub test_suite_id: String,
    pub test_result: TestResult,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_report_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerPayload {
    pub schema_version: u32,
    pub author_attestation_hash: String,
    pub tests_attestation_hash: String,
    pub manifest_hash: String,
    pub binary_artifact_hashes: Vec<String>,
    pub source_artifact_hash: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TestResult {
    Pass,
    Fail,
}
