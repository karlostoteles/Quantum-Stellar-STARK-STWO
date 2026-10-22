#!/usr/bin/env bash
# Builds the Soroban verifier contract. Needs the nightly toolchain from rust-toolchain.toml with
# the rust-src component (rustup component add rust-src).
#
#   ./scripts/build-contract.sh            default: Blake2s compiled into the contract (deployed)
#   KECCAK=1 ./scripts/build-contract.sh   Keccak-256 via the host, Stwo at opt-level 3 (~20 % cheaper)
#   FAST=1 ./scripts/build-contract.sh     cheaper verifier, same proofs and results (PATCHES.md, change 6);
#                                          NAMED=1 keeps the function names for profiling/
#   DEMO=1 ./scripts/build-contract.sh     the small contract that keeps proofs on chain (contracts/demo)
#   RECOVERY=1 ./scripts/build-contract.sh the seed-recovery prototype (contracts/recovery);
#                                          CONTRACT_FEATURES=wasm-ec adds its measurement-only entry point
#
# soroban-sdk 29 expects to be built by `stellar contract build` (stellar-cli >= 25.2), which also
# removes unused entries from the contract spec. This script builds with cargo directly, because
# the contract needs -Zbuild-std; it sets the variable stellar-cli sets.
set -euo pipefail
cd "$(dirname "$0")/.."
PROFILE=contract
FEATURES=()
PKG=stwo-soroban-verifier
if [ "${KECCAK:-0}" = "1" ]; then PROFILE=contract-keccak; FEATURES=(--features keccak); fi
if [ "${FAST:-0}" = "1" ]; then
  PROFILE=contract-fast; FEATURES+=(--features fast)
  # memory.copy / memory.fill instead of byte loops: Soroban's wasmi enables bulk memory. This
  # variable replaces the flags of .cargo/config.toml, so they are repeated here.
  export CARGO_TARGET_WASM32V1_NONE_RUSTFLAGS="-Zlocation-detail=none -Zfmt-debug=none -Zunstable-options -Cpanic=immediate-abort -Ctarget-feature=+bulk-memory"
fi
if [ "${NAMED:-0}" = "1" ]; then PROFILE=contract-named; fi
if [ "${DEMO:-0}" = "1" ]; then PKG=stwo-soroban-demo; fi
if [ "${RECOVERY:-0}" = "1" ]; then PKG=stwo-soroban-recovery; fi
if [ -n "${CONTRACT_FEATURES:-}" ]; then FEATURES+=(--features "$CONTRACT_FEATURES"); fi
SOROBAN_SDK_BUILD_SYSTEM_SUPPORTS_SPEC_SHAKING_V2=1 cargo build \
  -p "$PKG" --target wasm32v1-none --profile "$PROFILE" -Zbuild-std=core,alloc ${FEATURES[@]+"${FEATURES[@]}"}
WASM=target/wasm32v1-none/$PROFILE/${PKG//-/_}.wasm
SIZE=$(stat -c %s "$WASM")
echo "$WASM: $SIZE bytes (Soroban limit 131072)"
if [ "${NAMED:-0}" != "1" ]; then
  [ "$SIZE" -le 131072 ] || { echo "contract too large for Soroban" >&2; exit 1; }
fi
