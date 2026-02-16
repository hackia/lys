#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
crate_dir="$(cd "${script_dir}/.." && pwd)"
out_dir="${crate_dir}/fixtures-fake"
if [[ "$#" -gt 0 ]]; then
  out_dir="$1"
  shift
fi

cd "${crate_dir}/.."
cargo run -p silexium --bin fake_release -- --out "${out_dir}" "$@"
