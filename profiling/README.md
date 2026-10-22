# Where the cost goes: profiling the verifier under Soroban's metering

This directory measures, operation by operation, what the Soroban host charges for verifying a
Stwo proof, and estimates what each proposed change (host functions, fuel model, contract size,
verifier engineering) would save. Results: [`results/REPORT.md`](results/REPORT.md) (measurements
and the savings model) and [`results/SHA512-ESTIMATE.md`](results/SHA512-ESTIMATE.md) (a paper
estimate for a SHA-512 preimage AIR). The JSON files next to them are the raw data.

Nothing here touches the deployed contract: the default build of `contracts/verifier` is
byte-for-byte the one on testnet (SHA-256 `22143bfe…`). Profiling uses builds with the function
names kept (`NAMED=1 ./scripts/build-contract.sh`) and, for phases and micro-benchmarks, the `probe`
feature; `FAST=1` measures the tuned verifier.

## Tools

- **`profiler/`** runs one contract call inside `soroban-env-host` 29.0.1 (the network's metering,
  via `soroban-sdk` testutils) and attributes every wasmi fuel charge to the Wasm function whose
  instruction is executing: self fuel, inclusive fuel and calls per function. It needs a patched
  `soroban-wasmi` (`patch-wasmi.py` + `profile.rs`, ~60 lines: hooks in the two places the executor
  charges fuel, in the call paths, and a `WASMI_FUEL_COSTS=base,entity,load,store,call` override to
  measure one instruction class at a time). `setup.sh` copies the crate from the cargo registry and
  patches it; the copy is not committed.
- **`verify_phase(proof, k)`** (contract, feature `probe`; `crates/air/src/probe.rs`) runs the same
  steps as `verify` and stops after phase `k`, so the difference between consecutive calls is the
  cost of a phase under real metering, independent of inlining.
- **`bench(op, n)`** (contract, feature `probe`) runs `n` field operations, hashes, twiddle
  computations or host calls on values the compiler cannot fold; cost per operation is
  `(cost(2n) − cost(n)) / n`.
- **`stwo-soroban-prover count-ops`** (feature `op-count`, vendored Stwo patch 5 in `PATCHES.md`)
  counts M31/CM31/QM31 operations and Blake2s hashes per verifier phase, natively, on the same code
  path the contract runs.
- `run-all.py` runs everything into `results/`; `report.py`, `model.py` and `sha512-estimate.py`
  turn the JSON into the Markdown reports.

## Reproduce

```bash
# once: patched soroban-wasmi and the profiler
./profiling/setup.sh

# the two provers (plain for native timing, op-count for the operation counts)
cargo build --release -p stwo-soroban-prover
cargo build --release -p stwo-soroban-prover --features op-count --target-dir target/op-count

# all measurements (builds the named contracts, ~5 min each variant) and the reports
python3 profiling/run-all.py
python3 profiling/run-all.py --variant fast   # needs the fast provers, see run-all.py
python3 profiling/report.py > profiling/results/REPORT.md
python3 profiling/model.py >> profiling/results/REPORT.md
python3 profiling/sha512-estimate.py > profiling/results/SHA512-ESTIMATE.md

# one-off: a per-function profile of any proof
profiling/target/release/stwo-soroban-profiler --wasm profiling/results/verifier-named.wasm \
  --proof proofs/compact.bin --top 40
```

## Headline figures (compact proof, 43.1 M instructions; standard, 216.6 M)

- The `fast` build (feature `fast`, `PATCHES.md` change 6; measured with `run-all.py --variant fast`,
  files `results/fast-*`) verifies the same proofs for 17.3 M / 99.8 M locally (−60 % / −54 %) and
  18.6 M / 106.5 M as declared on testnet, with no protocol change. What is left is mostly the
  Blake2s compressions (31 % / 42 %), proof decoding (19 % / 16 %) and the `call` charges (17 % / 15 %).

- By phase: FRI decommitment 26 % / 54 %, quotients (`fri_answers`) 21 % / 13 %, Merkle paths of
  the trace trees 18 % / 19 %, component setup 10 % / 2 %, decoding 8 % / 8 %.
- The `call` instruction's 67 fuel is 32 % / 28 % of the whole bill; a third of the charged calls
  never execute (wasmi charges a block when it is entered).
- A per-element field host function is a net loss: `bn254_fr_mul` costs 12,842 instructions per
  call as measured, a QM31 multiply in Wasm 4,424. A Blake2s host function called per hash saves
  nothing (as the Keccak build showed). A Merkle-path host function would save 22 % / 40 %.
- Without any protocol change, the verifier itself had 54–60 % to give (the `fast` build above):
  twiddles were recomputed with a scalar multiplication per butterfly (11 % / 18 %), field
  arithmetic was not inlined at opt-level "s" (10 % / 8 %), copying plus allocation was 21 % / 17 %
  (`memcpy` as a byte loop: the `bulk-memory` target feature alone was worth 19 % / 23 % of the
  tuned build), and the constraint framework's info pass ran at every verification (10 % / 2 %).
