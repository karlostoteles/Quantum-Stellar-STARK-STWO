# stwo-soroban

A Soroban contract that verifies [Stwo](https://github.com/starkware-libs/proving) proofs on Stellar.
Stwo is StarkWare's Circle STARK prover, the one Starknet uses. The check is hash-based
(Blake2s over the Mersenne-31 field): no elliptic curves and no pairings, which is what makes
STARKs a candidate for post-quantum proof verification.

Stellar's [Quantum Preparedness Plan](https://stellar.org/blog/foundation-news/introducing-the-quantum-preparedness-plan)
leaves zero-knowledge proofs as open work and names STARKs as a candidate. This project measures
that candidate on Soroban today, with StarkWare's open-source prover and Stellar's own tooling.

Status: working on Stellar testnet. It is a demo and has not been audited.

A second prototype, `contracts/recovery`, recovers an Ed25519 account from its seed after Stellar
stops accepting Ed25519 signatures, with hashes only (see
[Seed recovery](#seed-recovery-after-the-cutoff-prototype)).

## On testnet (protocol 29)

Contract `CAUHDUJYMYIDUHMR4AH4TSHXYXLTCBDZSLKX7AW3R274VKAIJNHBX3QG`

| Proof | FRI parameters | Proof size | CPU instructions | Share of the 400 M tx limit | Resource fee | Transaction |
|---|---|---|---|---|---|---|
| compact | pow 24, blowup 2^8, 9 queries, fold 2^3 | 12,813 B | 45,418,055 | 11.4 % | 0.0107 XLM | [b5b83965…](https://stellar.expert/explorer/testnet/tx/b5b83965591a9976836447d418420b50974233152f5bfba13c01a29146712a92) |
| standard (StarkWare's default for Stwo-Cairo) | pow 26, blowup 2, 70 queries | 71,701 B | 227,905,305 | 57.0 % | 0.0517 XLM | [b7fede74…](https://stellar.expert/explorer/testnet/tx/b7fede74f7b47178cc97f0124b95aded6bac7e235c79c0488a777f19f45a9a71) |
| compact, one queried value changed | — | 12,813 B | — | — | — | [rejected](https://stellar.expert/explorer/testnet/tx/42d173ffb5e6c2f71941200d5bb6d565bd317e5969e25ee1417ac6599122ae7b) |
| compact, twice the formula's bits (a cost test, see Caveats; 19 October) | pow 24, blowup 2^8, 21 queries, fold 2^3 | 26,164 B | 77,707,666 | 19.4 % | 0.0093 XLM at the halved testnet rates | [d024ae8d…](https://stellar.expert/explorer/testnet/tx/d024ae8d4ef366353a1d204249175633cbf4b8a8a341421bee7925bf6b8d0aee) |

Both accepted proofs claim 96 bits of security by Stwo's formula, `pow_bits + log_blowup × queries`
(see Caveats). Contract size: 126,650 bytes of the 131,072 allowed. Instruction counts throughout
this README are as declared by simulation, which stellar-rpc pads a few percent above the metered
use; the fees shown are the resource fee declared by simulation, and the fee actually charged runs a
few percent lower (the [summary page](https://claude.ai/artifact/P8axyK8yQZT4QKuMdomCo6) shows
charged fees). The fees are from 13 October, when testnet charged mainnet's rates; testnet has since
halved them (3 stroops per 10,000 instructions instead of 7, and 203 per KB of transaction instead
of 406).

A second build, with Keccak from the host instead of Blake2s and more inlining (see below), is
deployed as `CCHOM5ONNJVD3G4ECVG44MJHMHOWXYZXBFGW5PEDI2R5SV7I557PCIJ3` (129,372 bytes):

| Proof | Proof size | CPU instructions | Share of the 400 M tx limit | Against the first build | Transaction |
|---|---|---|---|---|---|
| compact (Keccak) | 12,875 B | 35,980,178 | 9.0 % | −20.8 % | [c34ba5b2…](https://stellar.expert/explorer/testnet/tx/c34ba5b24f95686ea93dc18e371a49802bbca210c0d6ca8d64f3ef89307d8b6c) |
| standard (Keccak) | 71,245 B | 188,249,851 | 47.1 % | −17.4 % | [e6e62ee2…](https://stellar.expert/explorer/testnet/tx/e6e62ee2190826c4810a4e83987ec32496360eae2d0711b3423baced29e09933) |
| compact (Keccak), one queried value changed | 12,875 B | — | — | — | [rejected](https://stellar.expert/explorer/testnet/tx/fadf3a889b9a0401f23c78ee42d92a5c8ee2272da7b33be913f29026d207abff) |

### Run it yourself

`contracts/demo` (`CDD4QZ6ULNZ57EWVQEKWR7RIQERSAADGRBW3LGLQ2M2MBJCRY76H4QRB`) keeps proofs on
chain and `run(slot)` hands one to its verifier, so a transaction that re-runs a verification is a
few hundred bytes. `node scripts/demo.mjs testnet links` prints, for each slot, a Stellar Lab link
that opens the simulateTransaction form with that transaction filled in: press Submit and a
testnet RPC node runs the verifier and returns its answer. Nothing is signed or sent. A link that
carried the proof itself would be about 17 KB, and Lab's server refuses it (HTTP 431).

| Slot | Proof | Verifier | Answer | Instructions |
|---|---|---|---|---|
| 1 | compact | first build | `security_bits: 96`, `last: 1476137047` | 45.9 M |
| 2 | compact, one value changed | first build | `Error(Contract, #4)` | — |
| 3 | standard | first build | `security_bits: 96`, `last: 1476137047` | 228.5 M |
| 4 | compact (Keccak) | second build | `security_bits: 96`, `last: 1476137047` | 36.5 M |

The hop through the demo contract adds about 0.5 M instructions.

Testnet archives a contract 7 days after its last extension (mainnet: 120 days), and calls fail
until it is restored. Both verifiers were extended on 16 October to about 20 November with
`node scripts/extend-ttl.mjs testnet --days=35 <contract>`; the demo contract and its proofs were
stored for the maximum, 180 days. On testnet, rent for the 126 KB verifier runs at about 11 XLM a
day (the 35-day extension cost 323 XLM of free testnet XLM).

## What is proven

The claim is that the FibonacciSq sequence `a(n+2) = a(n)^2 + a(n+1)^2` over the Mersenne-31 field,
starting at `(a0, a1)`, has `last` as its term 99. This is the same recurrence as StarkWare's
[STARK 101](https://starkware.co/stark-101/), on a different field. The demo proves
`(1, 3141592) -> 1476137047`.

The AIR (`crates/air`) gives each trace row the 100 terms as columns. It has degree-2 transition
constraints and boundary constraints that pin every row to the public `a0`, `a1` and `last`. The
claim is mixed into the Fiat-Shamir transcript before any commitment. A change to the claim or
to a single queried value is rejected (see the tests).

## How it works

```
stwo-soroban-prover prove ──► proof envelope (postcard: claim + FRI parameters + StarkProof)
                                            │
                       verify(proof: Bytes) ▼   Soroban contract (Stwo verifier, no_std, wasm32v1-none)
                                   Verified { a0, a1, last, security_bits, proof_sha256 } + event
```

- The contract does not trust the parameters inside the proof. It derives the commitment-scheme
  layout from the claim, rejects FRI parameters below 96 bits or outside fixed ranges, and
  checks that the proof was made for exactly that layout.
- The same `verify_envelope` runs natively in the prover as a self-check and inside the contract.

## Where the cost goes

Almost all of it (99 %) is Wasm execution (`WasmInsnExec`). Host-side costs such as memory,
SHA-256 of the proof and instantiation are small. Two things decide the bill:

1. **Wasm function calls.** Soroban's calibrated fuel model charges 67 fuel for each `call`,
   against 1 for most instructions, so inlining matters. For the compact proof, the same verifier
   costs about 106 M instructions built at `opt-level = "z"` and 43 M at `"s"`. It would cost about
   31 M at `3`, but the contract would then be 148 KB, which is over the limit. Building at `"s"` fits only because
   `-Zbuild-std` with `panic=immediate-abort`, `-Zlocation-detail=none` and `-Zfmt-debug=none`
   drop the panic and formatting code.
2. **Queries, columns and tree depth.** Measured locally with Soroban's own metering
   (`cargo test … measure`):

| Rows | Parameters | Proof | Instructions |
|---|---|---|---|
| 2^4 | blowup 2^8, 9 queries | 12.8 KB | 43.1 M |
| 2^6 | blowup 2^8, 9 queries | 15.9 KB | 48.4 M |
| 2^4 | blowup 2^4, 18 queries | 14.0 KB | 50.7 M |
| 2^10 | blowup 2^4, 18 queries | 27.8 KB | 84.4 M |
| 2^10 | blowup 2, 70 queries (default) | 71.7 KB | 216.6 M |

For the compact proof, the 43.1 M break down roughly as follows:

- decoding the proof: 3.6 M (16.8 M for the 72 KB default proof);
- each extra FRI query: about 2.5 M at this layout (going from 9 to 21 queries adds 30.6 M), most
  of it opening the 100 trace columns at the queried point and checking their Merkle paths;
- each extra trace column: about 0.17 M with 9 queries (100 columns down to 50 saves 8.7 M);
- the rest, about 16 M by linear extrapolation, does not depend on the number of queries: mostly
  small field operations and function calls (evaluating the constraints at a random point, the
  transcript, the last FRI layer).

Hashing is not the bottleneck. Routing every hash through the host's `keccak256` instead of
Blake2s compiled into the contract changes the cost by less than 1 % (43.2 M against 43.1 M), though
it makes the contract 6.6 KB smaller. That space buys inlining: Keccak through the host plus
`opt-level = 3` for the two Stwo crates costs 34.0 M for the compact proof and 178.5 M for the
default one in the local measurement, at 129,372 bytes (`KECCAK=1 ./scripts/build-contract.sh`).
On testnet simulation declares the same build at 36.0 M and 188.2 M — a few percent above the local
metered figure, which is the margin stellar-rpc adds to its estimate — still 17–21 % below the first
build. Fees fall less, about 5–6 %, because for these proofs most of the fee pays for transaction
size and history, not instructions.

What would make STARK verification cheaper on Soroban, as input for protocol discussions:

- **M31/QM31 field arithmetic in the host.** Most of the cost is small field operations and the
  calls around them.
- **A larger contract size limit**, so verifiers can be built with full inlining (about −30 %).
- **A cheaper Wasm `call`** in the fuel model (67 fuel today).
- A hash host function (Blake2s, or a Merkle-path check) would help little, judging by the
  Keccak measurement above.

## Caveats

- **Security level.** 96 bits is what Stwo's formula gives for the parameters used
  (`security_bits = pow_bits + log_blowup_factor * n_queries`): 24 + 8×9 for the compact layout,
  26 + 1×70 for StarkWare's default. The formula assumes up-to-capacity proximity gaps for
  Reed–Solomon codes, shown false in November 2025
  ([Crites and Stewart, 2025/2046](https://eprint.iacr.org/2025/2046);
  [Ben-Sasson et al., 2025/2055](https://eprint.iacr.org/2025/2055)). A corrected,
  list-decoding-capacity conjecture costs little at these rates and field size: about 1 bit, so
  ~95 (compact) and ~94 (standard). These counterexamples are for Reed–Solomon codes over prime
  fields, not yet shown for the circle codes over QM31 that Stwo uses. What is *proven* today
  reaches only the Johnson bound, each query worth about half its formula bits, so roughly 60 bits
  for both layouts; that floor is untouched by the November result. A quantum attacker can grind
  Fiat–Shamir challenges with Grover (Chiesa, Manohar and Spooner, TCC 2019), roughly halving each
  term, so about 30 proven bits (the Merkle hash's collision resistance falls less far, to ~85).
  Field-size terms cap every setting well below log2|QM31| ≈ 124 bits whatever the query count, and
  a quantum attacker halves that too, so the formula's "192" (compact with 21 queries, 77.7 M
  instructions, 19.4 % of a transaction, table above) is a cost test, not a security level; a
  larger margin needs a larger field.
- **No zero-knowledge.** Stwo proofs as used here are not zero-knowledge, so this proves public
  computations. It cannot protect a secret, such as a key behind a hash-based signature.
- **One statement.** The contract verifies one AIR. Verifying arbitrary Cairo programs needs
  StarkWare's recursion: the Cairo verifier alone compiles to 1.8 MB of Wasm, and a Cairo proof
  is 0.6–1 MB.
- **Vendored Stwo.** StarkWare's crates are vendored with two manifest changes (see `PATCHES.md`).
  No Rust source was changed.
- **Not audited.** Nothing here should secure funds.

## Seed recovery after the cutoff (prototype)

`contracts/recovery` tests a way to recover an Ed25519 account once Stellar stops accepting Ed25519
signatures, with hashes only and no zero-knowledge proof. A Stellar secret key (S…) is the RFC 8032
seed k, and the account key is A = [s]B with s = clamp(SHA-512(k)[0..32]). A quantum attacker who
breaks A learns s but not k, so after the cutoff k still identifies the owner.

1. `commit(C)`, at any time, before the cutoff too, with
   C = SHA-256("stellar-seed-recovery-v0" ‖ network id ‖ A ‖ k ‖ SHA-256(new key)).
2. `reveal(A, k, new key)` once the cutoff has passed and C is at least `delay` ledgers old. The
   contract recomputes C, derives s from k and checks [s]B = A. The first reveal that passes
   records the new key for A; later ones are refused.

Whoever reads k — from the owner's own `reveal`, from the `simulateTransaction` a wallet or fee
relayer runs, or from any later leak — can commit to it with their own key, but must then wait
`delay` ledgers. The owner's commitment is already aged, so a prompt reveal wins. Unlike Fawkescoin
and Stewart et al., where the earliest commitment wins, here the first valid reveal is final at
once: no dispute window, but it assumes liveness — from the first time k leaves the wallet the owner
has `delay` ledgers to land one transaction, so wallets should reveal as soon as the cutoff passes
and through more than one relayer. `delay` must be sized for the cutoff surge and for validator
censorship; "about a day" is a placeholder. A commitment made at least `delay` ledgers before the
cutoff can be opened as soon as the cutoff passes. This is the commit–delay–reveal pattern of
Fawkescoin (Bonneau and Miller, 2014) and Stewart et al. (ePrint 2018/213).

The check [s]B = A uses the host's `ed25519_verify` instead of curve arithmetic in Wasm: the
contract builds a signature with nonce point R = B, S = 1 + h·s mod L with
h = SHA-512(B ‖ A ‖ C ‖ ctr) mod L. The host accepts it when [h]([s]B − A) = 0. `verify_strict`
rejects a small-order A but not A plus an 8-torsion point, so the contract grinds the one-byte ctr
until h is odd; an odd h kills no nonzero 2-, 4- or 8-torsion component, forcing [s]B = A exactly.
It is the Ed25519 counterpart of Buterin's `ecrecover` trick on Ethereum. A host verifier that
multiplied by the cofactor would break this check.

On testnet (contract
[`CBBGCPAY…`](https://stellar.expert/explorer/testnet/contract/CBBGCPAY2YG7N6KCTGYTNNF46GD4H7LRYW5YWTMQ2J3WG25TKUI4GZ36),
delay 60 ledgers, 21 October 2026), with a thief who read the seed from the owner's pending reveal:

| Step | CPU instructions (sim.) | Transaction size | Fee charged | Transaction |
|---|---|---|---|---|
| owner commits | 508,545 | 496 B | 0.0114 XLM | [5a125b7b…](https://stellar.expert/explorer/testnet/tx/5a125b7bf29a7df620b51af4ba4d9f1082ea7c53f1cbe063f87c4f3a58247ce4) |
| thief commits with their own key | 508,545 | 496 B | 0.0114 XLM | [bb106661…](https://stellar.expert/explorer/testnet/tx/bb1066613bae769c5cb0246a6ed0d331f0760bcd9e41f86915f2c2afafa91fdf) |
| thief reveals at once | refused: `TooEarly` (#2) | — | — | simulation |
| owner reveals, 1,312-byte new key | 1,430,577 | 1,972 B | 0.0225 XLM | [296cb61b…](https://stellar.expert/explorer/testnet/tx/296cb61b56c69b7961ecbaad1c542b183da3ca829f9b3b51af5e7506c65a70a7) |
| thief reveals after the delay | refused: `AlreadyRecovered` (#3) | — | — | simulation |

The key check alone, declared by simulation on testnet: 1,097,592 instructions through
`ed25519_verify`, 23,283,138 with Curve25519 compiled into the contract, about 21× more
(`deployments/testnet-recovery-measure.json`; the Wasm figure uses curve25519-dalek without its
precomputed tables). The tests in `contracts/recovery/tests` also cover the cutoff, a commitment
made too close to it, copied commitments, wrong seeds and keys, network-id binding, a mixed-order A,
and the case the scheme cannot help — an owner kept out for the whole delay loses. An earlier
design, in which the earliest commitment wins during a dispute window after the reveal, was replaced
by this first-valid-reveal rule, which leaves no window in which a recorded recovery can still
change.

The new key here is 1,312 random bytes, the size of an ML-DSA-44 public key; the contract stores
only its SHA-256. The commitment, the `Recovered` record and the contract instance are persistent
entries, so "commit at any time" means a years-old commitment may have to be restored before its
reveal (archived persistent entries restore automatically since protocol 23, and cannot be
recreated by anyone else). A real recovery of classic G… accounts would be a protocol rule, because
a contract cannot change a classic account's signers. Fake seeds only.

## Reproduce

Requirements: rustup (the toolchain in `rust-toolchain.toml` is installed automatically), the
`rust-src` component (`rustup component add rust-src`), and Node 18+ for deployment.

```bash
# 1. Prover (native)
cargo build --release -p stwo-soroban-prover
./target/release/stwo-soroban-prover prove --a0 1 --a1 3141592 --security compact --out proofs/compact.bin
./target/release/stwo-soroban-prover prove --a0 1 --a1 3141592 --log-rows 10 --security standard --out proofs/standard.bin
./target/release/stwo-soroban-prover prove --a0 1 --a1 3141592 --fri 24,8,21,3,0 --out proofs/compact-192.bin   # twice the bits
./target/release/stwo-soroban-prover tamper --in proofs/compact.bin --out proofs/compact-bad-proof.bin --mode proof
./target/release/stwo-soroban-prover tamper --in proofs/compact.bin --out proofs/compact-false-claim.bin --mode claim
./target/release/stwo-soroban-prover tamper --in proofs/standard.bin --out proofs/standard-bad-proof.bin --mode proof

# 2. Contract (wasm32v1-none, must stay under 131,072 bytes)
./scripts/build-contract.sh
# Keccak variant (about 20 % cheaper): proofs from a prover built with --features keccak, and
#   KECCAK=1 ./scripts/build-contract.sh

# 3. Tests: the compiled contract runs inside the Soroban host with mainnet limits enforced
cargo test -p stwo-soroban-verifier --test onchain -- --nocapture --test-threads 1

# 4. Testnet (the account is created and funded by friendbot; results go to deployments/testnet.json)
(cd scripts && npm install)
node scripts/deploy.mjs testnet deploy
node scripts/deploy.mjs testnet verify proofs/compact.bin
node scripts/deploy.mjs testnet reject proofs/compact-bad-proof.bin

# 5. Second build on testnet (logs to deployments/testnet-keccak.json)
cargo build --release -p stwo-soroban-prover --features keccak   # then prove into proofs/keccak/
KECCAK=1 ./scripts/build-contract.sh
VARIANT=keccak node scripts/deploy.mjs testnet deploy
VARIANT=keccak node scripts/deploy.mjs testnet verify proofs/keccak/compact.bin

# 6. Proofs kept on chain and Stellar Lab links (logs to deployments/testnet-demo.json)
DEMO=1 ./scripts/build-contract.sh
node scripts/demo.mjs testnet deploy
node scripts/demo.mjs testnet put 1 CAUHDUJYMYIDUHMR4AH4TSHXYXLTCBDZSLKX7AW3R274VKAIJNHBX3QG proofs/compact.bin
node scripts/demo.mjs testnet links

# 7. Keep the testnet contracts live (they are archived 7 days after the last extension)
node scripts/extend-ttl.mjs testnet --days=35 CAUHDUJYMYIDUHMR4AH4TSHXYXLTCBDZSLKX7AW3R274VKAIJNHBX3QG

# 8. Seed recovery prototype: tests, then testnet with fake seeds (logs to deployments/testnet-recovery*.json)
./scripts/build-recovery.sh
cargo test -p stwo-soroban-recovery --release -- --nocapture
node scripts/recovery.mjs testnet deploy 60      # delay of 60 ledgers, cutoff = now
node scripts/recovery.mjs testnet race           # owner, thief, about 10 minutes
node scripts/recovery-measure.mjs testnet        # key check: host ed25519_verify vs Wasm
```

## Layout

```
crates/air            statement, AIR, envelope and verify_envelope (no_std, shared)
crates/prover         CLI: prove, tamper, inspect
contracts/verifier    Soroban contract + tests that run the Wasm under Soroban metering
contracts/demo        keeps proofs on chain so a short transaction can re-run a verification
contracts/recovery    seed recovery after the Ed25519 cutoff, commit–delay–reveal (prototype)
vendor/               StarkWare's stwo and stwo-constraint-framework 2.4.0 (Apache-2.0)
scripts/              build-contract.sh, build-recovery.sh, deploy.mjs, demo.mjs, extend-ttl.mjs,
                      recovery.mjs, recovery-measure.mjs
deployments/          transactions sent, per network
```

License: Apache-2.0. Stwo is © StarkWare (Apache-2.0).
