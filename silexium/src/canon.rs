use blake3::Hasher;

pub fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

pub fn attestation_hash(
    kind: &str,
    key_id: &str,
    payload_hash: &str,
    signature: &str,
    created_at: &str,
    tsa_proof_hex: &str,
    ots_proof_hex: &str,
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"SILEXIUM-ATTESTATION\n");
    hasher.update(kind.as_bytes());
    hasher.update(b"\n");
    hasher.update(key_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(payload_hash.as_bytes());
    hasher.update(b"\n");
    hasher.update(signature.as_bytes());
    hasher.update(b"\n");
    hasher.update(created_at.as_bytes());
    hasher.update(b"\n");
    hasher.update(tsa_proof_hex.as_bytes());
    hasher.update(b"\n");
    hasher.update(ots_proof_hex.as_bytes());
    hasher.update(b"\n");
    hasher.finalize().to_hex().to_string()
}

pub fn entry_hash(manifest_hash: &str, author_hash: &str, tests_hash: &str, server_hash: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(b"SILEXIUM-LOG-ENTRY\n");
    hasher.update(b"manifest:");
    hasher.update(manifest_hash.as_bytes());
    hasher.update(b"\n");
    hasher.update(b"author:");
    hasher.update(author_hash.as_bytes());
    hasher.update(b"\n");
    hasher.update(b"tests:");
    hasher.update(tests_hash.as_bytes());
    hasher.update(b"\n");
    hasher.update(b"server:");
    hasher.update(server_hash.as_bytes());
    hasher.update(b"\n");
    hasher.finalize().to_hex().to_string()
}
