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

Notes:
- Silexium is a single binary service (no client SDK here).
- Storage uses SQLite in the XDG data directory.
- API endpoints are `/install` and `/update` (JSON POST).
- The server requires an ed25519 signing key for STH signatures.
