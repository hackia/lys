# Fixtures

This folder contains a self-consistent fixture set that can be ingested
directly by Silexium for tests.

Contents:
- `manifest.json` (JCS-canonical JSON)
- `payloads/*.json` (JCS-canonical JSON)
- `release.toml` (references the payloads + proofs)
- `tsa/*.tsr` and `ots/*.ots` (non-empty placeholders)

All hashes in `release.toml` match the JCS bytes of the fixture JSON files.

To ingest, add keys with these IDs and roles:
- author: `1111...` (64 hex chars of `1`)
- tests: `2222...`
- server: `3333...`
