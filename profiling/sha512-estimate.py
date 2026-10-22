#!/usr/bin/env python3
"""Paper estimate for a SHA-512 preimage AIR (the post-cutoff seed statement) verified on Soroban,
scaled from the measured FibonacciSq figures in profiling/results/. No circuit is built here.

Statement: a private 32-byte k such that SHA-512(k)[0..32] equals a public value (the verifier
clamps it and checks [s]B = A outside the proof, as contracts/recovery does). One SHA-512
compression: 80 rounds on 64-bit words.

AIR sketch used for the counts (one row per round, 2^7 rows):
  - the words that feed bitwise functions are bit-decomposed: new `a` (Σ0, Maj), new `e` (Σ1, Ch)
    and the schedule word W_t (σ0, σ1), 64 bit columns each;
  - the 64-bit additions use 16-bit limbs (4 per word) with small carry columns, the limb/bit
    consistency doubles as the range check;
  - f, g = e of the previous two rows, b, c = a of the previous two rows and W_{t-2}, W_{t-15} are
    read through mask offsets, so the bit columns are sampled at three points each.
Degree <= 3. A lookup-based design (XOR tables through logup) would have fewer columns and more
interaction columns; it is not estimated here.
"""
import json
import pathlib

RES = pathlib.Path(__file__).resolve().parent / "results"
CPU_PER_FUEL = 4

# --- AIR shape
BIT_COLS = 3 * 64            # a, e, W bits
LIMB_COLS = 3 * 4            # a, e, W as 16-bit limbs
CARRY_COLS = 12
COLS = BIT_COLS + LIMB_COLS + CARRY_COLS
LOG_ROWS = 7                 # 80 rounds + final addition, padded to 128
MASK_POINTS_BITS = 3         # offsets 0, -1, -2 (W: 0, -2, -15)
SAMPLES = BIT_COLS * MASK_POINTS_BITS + (LIMB_COLS + CARRY_COLS) * 1 + 8   # + composition columns
CONSTRAINTS = COLS           # one constraint per column, roughly
QM31_MULS_PER_CONSTRAINT = 20

# --- measured references: FibonacciSq with the compact parameters (9 queries, blowup 2^8, fold 8,
# log size 12, 4 FRI layers) and with StarkWare's default (70 queries, blowup 2, fold 1, log size 11,
# 11 FRI layers). Query-dependent phases are calibrated on both points.
REF_COLS, REF_SAMPLES = 100, 108
REF = {}
for name, queries, log_size, layers in [("compact", 9, 12, 4), ("standard", 70, 11, 11)]:
    ph = json.load(open(RES / f"{name}-phases.json"))
    cum = ph["cumulative"]
    phase = {n: (cum[i]["fuel"] - (cum[i - 1]["fuel"] if i else 0)) * CPU_PER_FUEL for i, n in enumerate(ph["phases"])}
    nat = json.load(open(RES / f"{name}-native.json"))
    REF[name] = dict(queries=queries, log_size=log_size, layers=layers, phase=phase, bytes=nat["proof_bytes"])
mb = json.load(open(RES / "microbench.json"))
QM31_MUL_CPU = mb["QM31 mul"]["cpu_per_op"]
HASH_CPU = mb["Blake2s of 64 B (one compression)"]["cpu_per_op"]


def raw_size(cols, samples, queries, log_size, fold_step, layers):
    """Simple sum of the proof parts (shared Merkle nodes and leaf packing ignored)."""
    oods = samples * 16
    queried = cols * queries * 4
    merkle = 2 * queries * log_size * 32
    fri = queries * sum((2 ** fold_step - 1) * 16 + 32 * max(log_size - fold_step * (i + 1) - 2, 0) for i in range(layers))
    return oods + queried + merkle + fri


# Ratio measured/raw for the two references; interpolated in log(queries) in between.
import math
_r = {n: REF[n]["bytes"] / raw_size(REF_COLS, REF_SAMPLES, REF[n]["queries"], REF[n]["log_size"], 3 if n == "compact" else 1, REF[n]["layers"])
      for n in REF}


def size_factor(queries):
    t = (math.log(queries) - math.log(9)) / (math.log(70) - math.log(9))
    t = min(max(t, 0.0), 1.0)
    return _r["compact"] * (1 - t) + _r["standard"] * t


def est(queries, log_blowup, fold_step):
    log_size = LOG_ROWS + log_blowup
    layers = -(-log_size // fold_step)
    size = raw_size(COLS, SAMPLES, queries, log_size, fold_step, layers) * size_factor(queries)

    c, s = REF["compact"], REF["standard"]
    cp, sp = c["phase"], s["phase"]
    # Query-dependent phases: cost = a + b*queries fitted on the two references, then scaled by the
    # AIR-dependent factor of each phase.
    def fit(name):
        b = (sp[name] - cp[name]) / (s["queries"] - c["queries"])
        a = cp[name] - b * c["queries"]
        return a, b
    a_q, b_q = fit("fri_answers")
    sample_ratio = SAMPLES / REF_SAMPLES
    # Merkle, trace trees: per-query cost, 45 % leaf hashing (∝ columns), 55 % paths (∝ depth);
    # per-query cost taken from the reference with the closer query count.
    ref = c if queries < 30 else s
    merkle_per_query = ref["phase"]["merkle_trace_trees"] / ref["queries"]
    merkle = queries * merkle_per_query * (0.45 * COLS / REF_COLS + 0.55 * log_size / ref["log_size"])
    # FRI decommit: per (query × layer) cost from the reference with the same fold step.
    ref_fri = c if fold_step > 1 else s
    fri = queries * layers * ref_fri["phase"]["fri_decommit"] / (ref_fri["queries"] * ref_fri["layers"])
    cost = {
        "decode": cp["decode"] * size / c["bytes"],
        "setup (component info pass)": cp["setup_component"] * 4,                      # ~4x the constraints work
        "mask points": cp["oods_pow_and_point"] * sample_ratio,
        "composition evaluation": CONSTRAINTS * QM31_MULS_PER_CONSTRAINT * QM31_MUL_CPU,
        "mix sampled values": cp["mix_sampled_values"] * sample_ratio,
        "Merkle, trace trees": merkle,
        "quotients (fri_answers)": (a_q + b_q * queries) * sample_ratio,
        "FRI decommit": fri,
        "other (commit, PoW, queries)": (cp["commit_trace_trees"] + cp["fri_commit"] + cp["fri_pow_and_queries"]) * queries / c["queries"],
    }
    return size, cost


def main():
    print(f"AIR sketch: {COLS} columns ({BIT_COLS} bit columns at {MASK_POINTS_BITS} mask points, {LIMB_COLS} limbs, "
          f"{CARRY_COLS} carries), 2^{LOG_ROWS} rows, {SAMPLES} sampled values at the OODS point, degree <= 3.\n")
    print("| FRI parameters | security (Stwo formula) | proof size (est.) | fits 132,096 B? | verification today (est.) | share of 400 M |")
    print("|---|---|---|---|---|---|")
    for label, queries, log_blowup, fold, pow_bits in [("compact: pow 24, blowup 2^8, 9 queries, fold 8", 9, 8, 3, 24),
                                                      ("pow 24, blowup 2^8, 21 queries, fold 8", 21, 8, 3, 24),
                                                      ("StarkWare default: pow 26, blowup 2, 70 queries", 70, 1, 1, 26)]:
        size, cost = est(queries, log_blowup, fold)
        total = sum(cost.values())
        print(f"| {label} | {pow_bits + log_blowup * queries} bits | ~{size / 1000:.0f} KB | {'yes' if size < 132096 else 'no'} | "
              f"~{total / 1e6:.0f} M | {100 * total / 400e6:.0f} % |")
    print()
    size, cost = est(9, 8, 3)
    print("Compact parameters, by phase (CPU instructions, estimate):\n")
    print("| phase | estimate | method |")
    print("|---|---|---|")
    methods = {
        "decode": "measured cost per byte × estimated size",
        "setup (component info pass)": "4× the FibonacciSq pass (more and longer constraints)",
        "mask points": "∝ sampled values",
        "composition evaluation": f"{CONSTRAINTS} constraints × {QM31_MULS_PER_CONSTRAINT} QM31 mul × {QM31_MUL_CPU:,.0f}",
        "mix sampled values": "∝ sampled values",
        "Merkle, trace trees": "per query from the reference; 45 % ∝ columns, 55 % ∝ depth",
        "quotients (fri_answers)": "a + b·queries fitted on both references, × sampled values (dominant)",
        "FRI decommit": "per query×layer from the reference with the same fold step",
        "other (commit, PoW, queries)": "∝ queries",
    }
    for k, v in cost.items():
        print(f"| {k} | {v / 1e6:.1f} M | {methods[k]} |")
    print(f"| **total** | **{sum(cost.values()) / 1e6:.0f} M** | ~{100 * sum(cost.values()) / 400e6:.0f} % of a transaction |")
    print()
    print(f"Reference per-unit costs used: QM31 mul {QM31_MUL_CPU:,.0f} CPU, Blake2s block {HASH_CPU:,.0f} CPU; "
          f"phases from compact-phases.json and standard-phases.json; size factor measured/raw = "
          f"{_r['compact']:.2f} (9 queries) to {_r['standard']:.2f} (70 queries).")


if __name__ == "__main__":
    main()
