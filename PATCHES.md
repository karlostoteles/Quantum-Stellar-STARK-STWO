# Changes to the vendored Stwo crates

Source: `starkware-libs/proving` at `150a06b3c3a959b3c82d1dab9d3ede8aa5006c33` (2026-10-04),
crates `stwo` (2.4.0) and `stwo-constraint-framework` (2.4.0). The deployed contract uses the
vendored source unchanged; only the manifests differ (1 and 2). Change 3 adds an opt-in feature
that the default build does not enable.

1. `vendor/stwo/Cargo.toml`: `dashmap` is now optional and enabled by the `prover` feature.
   Only the prover uses it, and it pulls `parking_lot_core`, which needs `std` and does not build
   for `wasm32v1-none`, the target Soroban requires. Upstream candidate: make it prover-only.
2. Both manifests: `[dev-dependencies]` and `[[bench]]` targets removed (their tests and
   benchmarks are not built here).
3. `vendor/stwo/src/core/vcs/keccak256_hash.rs` and `vendor/stwo/Cargo.toml`: new feature
   `host-keccak`. With it, `Keccak256Hasher` buffers its input and calls an external
   `stwo_host_keccak256(data, len, out)`, which the contract defines with Soroban's `keccak256`
   host function. The output is the same Keccak-256. Off by default; used only by the
   `KECCAK=1` build.

Outside the vendored code:

4. `contracts/verifier`: registers a `getrandom` backend that always fails. `stwo` depends on
   `starknet-ff` (for its Poseidon252 channel), which depends on `getrandom`, which has no
   backend on `wasm32v1-none`. The verifier never asks for randomness; the stub only lets the
   contract link. Upstream candidate: make the Poseidon252 channel an optional feature.
