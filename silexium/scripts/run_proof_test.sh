#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate_dir="$(cd "${script_dir}/.." && pwd)"
workspace_dir="$(cd "${crate_dir}/.." && pwd)"

env_file="${crate_dir}/fixtures-real/proof.env"
if [[ ! -f "${env_file}" ]]; then
  echo "missing ${env_file}"
  echo "copy fixtures-real/proof.env.example to proof.env and fill it in"
  exit 1
fi

set -a
# shellcheck disable=SC1090
. "${env_file}"
set +a

missing=0
for var in \
  SILEXIUM_PROOF_PAYLOAD_HASH \
  SILEXIUM_TSA_PROOF_FILE \
  SILEXIUM_OTS_PROOF_FILE \
  SILEXIUM_TSA_VERIFY \
  SILEXIUM_OTS_VERIFY
do
  if [[ -z "${!var:-}" ]]; then
    echo "missing env var: ${var}"
    missing=1
  fi
done
if [[ "${missing}" -ne 0 ]]; then
  exit 1
fi

cd "${workspace_dir}"
cargo test -p silexium verify_real_proofs_env -- --nocapture
