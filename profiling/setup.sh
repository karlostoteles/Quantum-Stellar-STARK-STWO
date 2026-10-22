#!/usr/bin/env bash
# Prepares the profiler: copies soroban-wasmi 0.31.1-soroban.29.0.0 from the cargo registry (it is
# a dependency of soroban-env-host, fetched by `cargo fetch` of the main workspace), applies the
# fuel-profiling patch and builds the profiler binary.
set -euo pipefail
cd "$(dirname "$0")"
if [ ! -d soroban-wasmi ]; then
  (cd .. && cargo fetch)
  src=$(ls -d ~/.cargo/registry/src/*/soroban-wasmi-0.31.1-soroban.29.0.0 | head -1)
  cp -r "$src" soroban-wasmi
  rm -f soroban-wasmi/Cargo.toml.orig soroban-wasmi/.cargo_vcs_info.json soroban-wasmi/.cargo-ok
fi
python3 patch-wasmi.py soroban-wasmi
cargo build --release
echo "profiler: $(pwd)/target/release/stwo-soroban-profiler"
