# Changes to the vendored Stwo crates

Source: `starkware-libs/proving` at `150a06b3c3a959b3c82d1dab9d3ede8aa5006c33` (2026-10-04),
crates `stwo` (2.4.0) and `stwo-constraint-framework` (2.4.0). The deployed contract uses the
vendored source unchanged; only the manifests differ (1 and 2). Changes 3 and 5 add opt-in features
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

5. `vendor/stwo/Cargo.toml`, `src/lib.rs`, `src/core/mod.rs`, new `src/core/op_count.rs`,
   `src/core/fields/{m31,cm31,qm31}.rs`, `src/core/vcs/blake2_hash.rs`, `src/core/verifier.rs`,
   `src/core/pcs/verifier.rs`, `src/core/fri.rs`: new feature `op-count` (profiling, 22 October).
   With it, the field operations (M31 add/sub/neg/mul/inverse, CM31 mul/inverse, QM31 mul/inverse)
   and Blake2s hashes bump atomic counters, and the verifier records a checkpoint after each of its
   phases. Off by default: the `op_count!` macro and the checkpoints compile to nothing, and the
   deployed contract build is byte-for-byte unchanged (SHA-256 `22143bfe…`, 126,650 bytes). Used by
   `stwo-soroban-prover count-ops` (see `profiling/`).

6. Feature `fast` (22 October): the same verification, cheaper on Soroban. Off by default; every
   change is behind `#[cfg(feature = "fast")]` (or `cfg_attr`), the default build is unchanged, and
   the results are identical (the same proofs are accepted, the same tampered ones rejected, the
   Blake2s equivalence is tested). Measured on the compact / standard proofs: 43.1 M → 17.3 M and
   216.6 M → 99.8 M instructions locally (testnet declares 18.6 M and 106.5 M).
   - `src/core/circle.rs`: `CirclePointIndex::to_point` adds one entry per non-zero 4-bit digit of
     the index from a compile-time table `GEN · (d << 4w)` (1 KB), instead of a double-and-add
     over 31 bits. Every twiddle and domain point goes through it.
   - `src/core/fields/{m31,cm31,qm31,mod}.rs`, `circle.rs`: `#[inline(always)]` on the small
     field operations and the circle addition (Soroban charges 67 fuel per Wasm call).
   - `src/core/vcs_lifted/blake2_merkle.rs`: `update_leaf` hashes the leaf's values in one update
     (same bytes); `hash_children` hashes the 64-byte node in one compression through the `blake2`
     crate's core API (`src/core/vcs/blake2_hash.rs`, `fast_blake2s::hash_two`).
   - `src/core/vcs_lifted/verifier.rs`: `MerkleVerifierLifted::verify` without the hash map, the
     sorts and the per-position vectors of the original (same hashing and witness order), plus
     `verify_ref` on borrowed inputs.
   - `src/core/pcs/verifier.rs`: no clone of the sampled values (`mix_felts`) nor of the queried
     values (Merkle verification) in `verify_values`.
   - `src/core/pcs/quotients.rs`: `fri_answers` builds the sample batches directly (same order and
     random powers as `build_samples_with_randomness_and_periodicity` + `ColumnSampleBatch::new_vec`)
     and factors each batch's numerator as `c·Σαᵢvᵢ − y·Σαᵢaᵢ − Σαᵢbᵢ`, so a query costs one
     QM31×M31 multiply per column; no per-query allocations.
   - `src/core/fri.rs`: `fold_coset` and `fold_circle_into_line` take their twiddles by stepping
     along the coset (one circle addition each, `double_x` between levels) and invert them in one
     batch; the layer verifiers call `verify_ref` instead of cloning the witness.
   - Also: `src/core/op_count.rs` gains nothing; feature `probe` (7) only marks a few verifier
     functions `#[inline(never)]` so the profiler in `profiling/` can see them. Not for deployment.

Outside the vendored code:

4. `contracts/verifier`: registers a `getrandom` backend that always fails. `stwo` depends on
   `starknet-ff` (for its Poseidon252 channel), which depends on `getrandom`, which has no
   backend on `wasm32v1-none`. The verifier never asks for randomness; the stub only lets the
   contract link. Upstream candidate: make the Poseidon252 channel an optional feature.

5. `crates/air` (feature `fast`): `FibSqFastComponent`, the same component as the framework one
   written by hand for the verifier: no constraint-framework info pass at run time, mask points
   without a point addition per column, constraints evaluated directly in the framework's order.
   `verify_envelope` uses it when `fast` is on; the prover keeps the framework component.
6. `scripts/build-contract.sh` with `FAST=1` builds with `-Ctarget-feature=+bulk-memory`
   (`memory.copy`/`memory.fill` instead of byte loops; Soroban's wasmi has bulk memory enabled
   and testnet accepted the contract). Profile `contract-fast`, deployed on testnet as
   `CDSF6RMU7BDO57RKOUHI2LJAQLWFBMSKRW2YJYSMO34WSOJBBCWC2EOD` (117,225 bytes).
