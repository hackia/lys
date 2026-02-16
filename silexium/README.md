# Silexium

Silexium is the binary-only API service for UVD. It resolves install/update
requests into verified artifacts (executable + source archive) with a strict
integrity chain.

Core ideas:
- UVD calls Silexium to resolve the correct release for an OS/arch.
- Responses include a signed manifest, artifact URLs, and verification proofs.
- Integrity uses blake3 hashes + ed25519 signatures + strong timestamps.
- A public, append-only transparency log (with mirrors) makes releases auditable.

Specification:
- The full concept spec is in `SPEC.md`.
- The normative contract is in `CONTRACT.md`.

Notes:
- Silexium is a single binary service (no client SDK here).
- Storage uses SQLite in the XDG data directory.
- API endpoints are `/install` and `/update` (JSON POST).
- The server requires an ed25519 signing key for STH signatures.
- Admin CLI supports `key add` and `ingest` (see `INGEST.md`).
- Public keys are expected as raw 32 bytes or 64 hex characters.
- Key roles: `author`, `tests`, `server`.
- Timestamp proofs are verified via external commands (see below).
- Example manifests/payloads live in `examples/`.
- Test fixtures live in `fixtures/`.

Admin examples:
```
silexium key add --role author --key author.pub --expires-at 2027-01-01T00:00:00Z
silexium key revoke --key-id <hex> --revoked-at 2026-02-16T12:00:00Z
silexium key rotate --role author --old-key-id <hex> --new-key author_v2.pub --expires-at 2028-01-01T00:00:00Z
silexium key list
silexium key list --json
silexium key revoke-expired
silexium ingest --file release.toml
```

Example (jq):
```
silexium key list --json | jq '.[] | {key_id, role, expires_at, revoked}'
```

Key list JSON schema:
```
[
  {
    "key_id": "hex",
    "role": "author|tests|server",
    "created_at": "RFC3339",
    "expires_at": "RFC3339|null",
    "revoked_at": "RFC3339|null",
    "revoked": true,
    "public_key_hex": "hex"
  }
]
```

Timestamp proof verification:
- `SILEXIUM_TSA_VERIFY` and `SILEXIUM_OTS_VERIFY` must point to executables.
- Each verifier is called as: `cmd <payload_hash> <proof_path>`.
- `payload_hash` is the lower-case blake3 hex of the JCS payload (UTF-8 bytes).
- Verifiers must exit 0 on success; nonzero exits fail ingest/serve.
- `SILEXIUM_SKIP_PROOF_VERIFY=1` bypasses verification (testing only).
