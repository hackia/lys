#!/usr/bin/env bash
set -euo pipefail

if [[ "$#" -ne 2 ]]; then
  echo "usage: mock_verify <payload_hash> <proof_path>" >&2
  exit 1
fi

proof_path="$2"
if [[ ! -f "${proof_path}" ]]; then
  echo "missing proof: ${proof_path}" >&2
  exit 1
fi
if [[ ! -s "${proof_path}" ]]; then
  echo "empty proof: ${proof_path}" >&2
  exit 1
fi

exit 0
