#!/usr/bin/env bash
# Builds the Soroban verifier contract. Needs the nightly toolchain from rust-toolchain.toml with
# the rust-src component (rustup component add rust-src).
#
#   ./scripts/build-contract.sh            default: Blake2s compiled into the contract (deployed)
#   KECCAK=1 ./scripts/build-contract.sh   Keccak-256 via the host, Stwo at opt-level 3 (~20 % cheaper)
#   DEMO=1 ./scripts/build-contract.sh     the small contract that keeps proofs on chain (contracts/demo)
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
if [ "${DEMO:-0}" = "1" ]; then PKG=stwo-soroban-demo; fi
SOROBAN_SDK_BUILD_SYSTEM_SUPPORTS_SPEC_SHAKING_V2=1 cargo build \
  -p "$PKG" --target wasm32v1-none --profile "$PROFILE" -Zbuild-std=core,alloc ${FEATURES[@]+"${FEATURES[@]}"}
WASM=target/wasm32v1-none/$PROFILE/${PKG//-/_}.wasm
SIZE=$(stat -c %s "$WASM")
echo "$WASM: $SIZE bytes (Soroban limit 131072)"
[ "$SIZE" -le 131072 ] || { echo "contract too large for Soroban" >&2; exit 1; }
