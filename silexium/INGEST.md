# Silexium Ingest (Admin CLI)

This file defines the input format for `silexium ingest`.

## Usage

```
silexium ingest --file release.toml
```

The paths inside the file are resolved relative to the `release.toml` folder.

## release.toml (example)

```
[release]
package = "uvd"
version = "1.2.3"
channel = "stable"
created_at = "2026-02-16T12:00:00Z"

[package]
description = "Universal Verified Disk"
license = "MIT"
homepage = "https://example.invalid/uvd"

[manifest]
format = "json"
path = "manifest.json"
blake3 = "..."
src_index_size = 12345
src_index_blake3 = "..."

[[artifacts]]
kind = "binary"
os = "linux"
arch = "x86_64"
size = 123456
blake3 = "..."
url = "https://mirror.example/uvd/1.2.3/linux-x86_64/uvd"

[[artifacts]]
kind = "source"
size = 34567
blake3 = "..."
url = "https://mirror.example/uvd/1.2.3/uvd.uvd"

[[attestations]]
kind = "author"
key_id = "..."
payload_hash = "..."
signature = "..."
created_at = "2026-02-16T12:10:00Z"
payload_path = "payloads/author.json"
tsa_proof_path = "tsa/author.tsr"
ots_proof_path = "ots/author.ots"

[[attestations]]
kind = "tests"
key_id = "..."
payload_hash = "..."
signature = "..."
created_at = "2026-02-16T12:20:00Z"
payload_path = "payloads/tests.json"
tsa_proof_path = "tsa/tests.tsr"
ots_proof_path = "ots/tests.ots"

[[attestations]]
kind = "server"
key_id = "..."
payload_hash = "..."
signature = "..."
created_at = "2026-02-16T12:30:00Z"
payload_path = "payloads/server.json"
tsa_proof_path = "tsa/server.tsr"
ots_proof_path = "ots/server.ots"
```

Notes:
- `manifest.blake3` is verified against the manifest JCS bytes.
- `attestations` must include exactly: `author`, `tests`, `server`.
- Keys must exist in the Silexium DB before ingest (`silexium key add`).
- Keys require `expires_at` and must be unexpired at ingest time.
- `payload_path` is required and is hashed with JSON JCS to verify `payload_hash`.
- Timestamp proofs are verified via external commands (see below).
- Example manifest/payloads are in `examples/`.
- A full ingest-ready fixture set is in `fixtures/`.

Timestamp proof verification:
- `SILEXIUM_TSA_VERIFY` and `SILEXIUM_OTS_VERIFY` must point to executables.
- Each verifier is called as: `cmd <payload_hash> <proof_path>`.
- `payload_hash` is the lower-case blake3 hex of the JCS payload (UTF-8 bytes).
- Verifiers must exit 0 on success; nonzero exits fail ingest.
- `SILEXIUM_SKIP_PROOF_VERIFY=1` bypasses verification (testing only).
