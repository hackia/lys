# Fake Release Fixtures

This folder is the default output for the fake release generator.

Generate a new fake release:
```
./silexium/scripts/gen_fake_release.sh
```

Then run:
```
./silexium/fixtures-fake/ingest.sh
```

Notes:
- The generated proofs are mocked, so `ingest.sh` sets
  `SILEXIUM_SKIP_PROOF_VERIFY=1`.
- Public keys are written to `keys/*.pub`.
- A local SQLite DB is created at `fixtures-fake/silexium.db`.
