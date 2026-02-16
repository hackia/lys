# Silexium Spec (Concept v0.1)

Scope: Silexium is a binary-only API that resolves UVD installs to
verified artifacts (executable + source). Integrity is enforced via
blake3 hashes, ed25519 signatures, strong timestamps, and a public
append-only transparency log with mirrors. No code in this spec.

## 1) Core Concepts

Entities:
- Package: name, description, license, metadata.
- Version: package + semver + channel (stable/beta/etc).
- Artifact: binary or source with OS/arch, size, hash, URL.
- Manifest: signed description of one release.
- Attestation: author, tests, server signatures (all required).
- Log Entry: append-only, Merkle log inclusion for public audit.

Trust model:
- Author signature required.
- Tests signature required.
- Server signature required.
- TSA + OpenTimestamps proofs required on each attestation.

## 2) Source Index File (SRC)

Purpose: reproducible verification of source tree contents.

File name: SRC
Encoding: UTF-8
Line endings: LF (\n)

Format (one line per file):
path<TAB>size<TAB>blake3

Rules:
- path is relative, normalized, uses '/' separators.
- path MUST NOT contain '.', '..', '//' or trailing '/'.
- size is integer bytes.
- blake3 is lower-case hex of file bytes.
- lines are sorted lexicographically by path.
- only regular files are listed (no directories).

Symlinks and hardlinks:
- Forbidden. Any symlink or hardlink in the source archive invalidates
  the package. Build/publish must fail if detected. UVD must reject.

## 3) Artifacts

Each release provides:
- binary artifact (per os/arch)
- source artifact (archive, e.g. .uvd or .tar.zst)

Hashing:
- blake3 over exact bytes of each artifact.
- SRC provides file-level hashing for the source tree.

## 4) Manifest (Release Description)

Manifest is the signed description of a release.
The signature is calculated over the exact bytes of the manifest file
as served (no re-serialization in the verifier).

Minimal manifest fields:
- package (string)
- version (string)
- channel (string)
- license (string)
- created_at (RFC3339)
- artifacts[]:
  - type: "binary" | "source"
  - os
  - arch
  - size
  - blake3
  - url
- src_index:
  - path: "SRC"
  - size
  - blake3

## 5) Attestations (All Required)

Each attestation includes:
- key_id
- signature (ed25519)
- created_at (RFC3339)
- timestamp_proof:
  - tsa (RFC3161 proof)
  - ots (OpenTimestamps proof)

Attestation payloads (conceptual):

Author attestation signs:
- manifest_hash
- src_index_hash
- source_artifact_hash
- package + version + license

Tests attestation signs:
- author_attestation_hash
- manifest_hash
- test_suite_id
- test_result (pass/fail)
- test_report_hash (optional)

Server attestation signs:
- author_attestation_hash
- tests_attestation_hash
- manifest_hash
- all artifact hashes

## 6) Transparency Log (Public, Mirrored)

Model: append-only Merkle log (CT-style).

Log entry contains:
- manifest_hash
- all attestation hashes
- TSA + OTS proofs (hash references)
- entry_hash = blake3(canonical_entry_bytes)

Merkle hashing (CT-style):
- leaf_hash = blake3(0x00 || entry_hash_bytes)
- node_hash = blake3(0x01 || left || right)
- MTH uses the largest power-of-two split.

Log exposes:
- STH (Signed Tree Head): tree_size, root_hash, timestamp, signature
- Inclusion proof (for a given entry)
- Consistency proof (between two STHs)

Mirrors:
- Mirrors replicate log + STHs.
- UVD can fetch proofs from any mirror.
- UVD must verify STH signature and consistency.

## 7) API Surface (Conceptual)

UVD command mapping:
- uvd install X -> resolve
- uvd update X  -> resolve
- uvd uninstall X -> local only (no server call)

Minimal endpoints:
- POST /install
  Request: package, os, arch, version (opt) or channel (opt), known_sth (opt)
  Response:
    - manifest (raw bytes + hash)
    - attestations (author/tests/server)
    - log_proof (STH + inclusion + consistency if known_sth provided)
    - artifacts (binary + source)
- POST /update
  Request: package, os, arch, current_version, channel (opt), known_sth (opt)
  Response: same as /install (plus up_to_date flag)
- POST /resolve (optional internal alias)
- GET /health (optional)

## 8) Verification Order (UVD)

1) Verify author, tests, server signatures.
2) Verify TSA + OTS proofs for each attestation.
3) Verify log inclusion + consistency against trusted STHs.
4) Verify blake3 hashes for artifacts.
5) Verify SRC index after extraction.
6) Reject on any mismatch.

## 9) SQLite (XDG) Storage

Location:
- $XDG_DATA_HOME/silexium/silexium.db
- fallback: ~/.local/share/silexium/silexium.db

Concept tables:
- packages, versions, artifacts
- manifests
- attestations
- keys (trusted, revoked)
- log_entries
- log_sth (cache of STHs)

## 10) Rejection Policy (Strict)

Reject if:
- any signature is missing or invalid
- TSA or OTS proof is missing or invalid
- log proofs are invalid or inconsistent
- any artifact hash mismatch
- SRC mismatch
- any symlink or hardlink detected
