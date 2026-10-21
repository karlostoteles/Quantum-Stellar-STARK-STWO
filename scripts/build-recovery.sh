#!/usr/bin/env bash
# Builds both variants of the seed-recovery prototype for the tests and for deployment:
#   target/wasm32v1-none/contract/recovery_wasm_ec.wasm  with check_key_wasm (measurement only)
#   target/wasm32v1-none/contract/recovery_default.wasm  without it (the one to deploy)
set -euo pipefail
cd "$(dirname "$0")/.."
OUT=target/wasm32v1-none/contract
RECOVERY=1 CONTRACT_FEATURES=wasm-ec ./scripts/build-contract.sh
cp "$OUT/stwo_soroban_recovery.wasm" "$OUT/recovery_wasm_ec.wasm"
RECOVERY=1 ./scripts/build-contract.sh
cp "$OUT/stwo_soroban_recovery.wasm" "$OUT/recovery_default.wasm"
sha256sum "$OUT/recovery_default.wasm" "$OUT/recovery_wasm_ec.wasm"
