# Real Proof Fixtures

This folder holds a local, real-proof fixture set for the opt-in integration
test `verify_real_proofs_env`.

Nothing here is committed by default. You provide the proof files and the
verifier commands for your environment.

Required files:
- `proof.env` (environment variables for the test)
- TSA proof file (RFC3161, e.g. `tsa/author.tsr`)
- OTS proof file (OpenTimestamps, e.g. `ots/author.ots`)

How to use:
1) Generate TSA + OTS proofs for a chosen payload_hash.
2) Put the proofs in `tsa/` and `ots/`.
3) Create `proof.env` (see `proof.env.example`).
4) Run `../scripts/run_proof_test.sh` from this repo.

Notes:
- The proof target MUST be the UTF-8 bytes of `payload_hash` (lower-case hex).
- The verifier commands must accept: `cmd <payload_hash> <proof_path>`.
- `SILEXIUM_SKIP_PROOF_VERIFY=1` will skip the test.
