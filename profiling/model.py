#!/usr/bin/env python3
"""What each lever would save, computed from profiling/results/*.json (stdout, Markdown).

    python3 profiling/model.py >> profiling/results/REPORT.md

Every scenario states its assumption in the code next to the number. Measured quantities come
from the profiles; host prices come from soroban-env-host 29.0.1's cost parameters (src/budget.rs).
"""
import json
import pathlib
import re

RES = pathlib.Path(__file__).resolve().parent / "results"
PROOFS = ["compact", "standard", "compact-192"]
CPU_PER_FUEL = 4
CALL_FUEL = 67
# Host cost parameters, CPU instructions (soroban-env-host 29.0.1).
DISPATCH = 295                       # DispatchHostFunction, per host function call
SHA256_CONST, SHA256_PER_BYTE = 3636, 7013 / 128   # ComputeSha256Hash; used as the price of a Blake2s host function
MEMCPY_CONST, MEMCPY_PER_BYTE = 42, 1 / 8          # MemCpy
WASM_CALL_CPU = CALL_FUEL * CPU_PER_FUEL           # what the contract pays for the `call` instruction itself
# Measured (microbench.json): a bn254_fr_mul through the host, all included.
# Native work per operation when a batched host function does it (an estimate in CPU instructions:
# a QM31 multiply is 16 M31 multiplies plus adds; Soroban prices BLS12-381 Fr mul at 332).
NATIVE_QM31_MUL = 60
NATIVE_M31_INV = 150


def load(name):
    return json.load(open(RES / name))


def fuel_of(functions, pattern, key="self_fuel"):
    return sum(f[key] for f in functions if re.search(pattern, f["name"]))


def calls_of(functions, pattern):
    return sum(f["calls"] for f in functions if re.search(pattern, f["name"]))


def phase_fuel(ph, name):
    i = ph["phases"].index(name)
    cum = ph["cumulative"]
    return cum[i]["fuel"] - (cum[i - 1]["fuel"] if i > 0 else 0)


def m(cpu):
    return f"{cpu / 1e6:.1f} M"


def compute(prefix):
    mb = load("microbench.json")
    rows = {}
    for p in PROOFS:
        fj = load(f"{prefix}{p}-functions.json")
        fns = fj["functions"]
        T = fj["wasm_fuel_total"]
        total_cpu = fj["cpu_instructions"]
        ph = load(f"{prefix}{p}-phases.json")
        cl = load(f"{prefix}{p}-classes.json")
        oc = load(f"{prefix}{p}-opcount.json")
        nat = load(f"{prefix}{p}-native.json")
        C = cl["call"] - cl["fixed"]  # call instructions charged
        r = {"baseline_cpu": total_cpu, "wasm_cpu": CPU_PER_FUEL * T}

        # A. call weight 67 -> 1 (exact: every charged call instruction pays 66 fuel less)
        r["A_call_1"] = 66 * C * CPU_PER_FUEL

        # C1. twiddles: CirclePoint<M31>::mul inclusive; a table lookup would keep ~10 % of it
        tw = fuel_of(fns, r"CirclePoint<stwo::core::fields::m31::M31>>::mul", "inclusive_fuel")
        r["C1_twiddles"] = 0.9 * tw * CPU_PER_FUEL
        # C2. inline the field functions: their call charge disappears (loads/stores of arguments too, not counted)
        field_calls = calls_of(fns, r"^<?stwo::core::fields::")
        r["C2_inline_fields"] = field_calls * CALL_FUEL * CPU_PER_FUEL
        # C3. copying and allocation: assume half of memcpy/memset/alloc is avoidable (clones, collect_vec)
        mem = fuel_of(fns, r"memcpy|memset|__rust_alloc|__rust_realloc|RawVec|finish_grow|grow_amortized|BumpPointer|::reserve|::clone")
        r["C3_memory_half"] = 0.5 * mem * CPU_PER_FUEL
        # C4. component info pass (setup_component phase): could be computed at compile time; keep 10 %
        r["C4_setup"] = 0.9 * phase_fuel(ph, "setup_component") * CPU_PER_FUEL
        # C5. proof format: a fixed layout read in place; assume 75 % of decoding goes away
        r["C5_decode"] = 0.75 * phase_fuel(ph, "decode") * CPU_PER_FUEL
        r["C_engineering"] = r["C1_twiddles"] + r["C2_inline_fields"] + r["C3_memory_half"] + r["C4_setup"] + r["C5_decode"]

        # D. Merkle path verification in the host (one call per tree), hashing priced like SHA-256 today.
        merkle_incl = fuel_of(fns, r"MerkleVerifierLifted<.*>>::verify", "inclusive_fuel")
        hashes = bytes_ = 0
        for row in oc["phases"]:
            if row["phase"] in ("merkle_trace_trees", "fri_first_layer", "fri_inner_layers"):
                hashes += row["blake2s_hashes"]
                bytes_ += row["blake2s_bytes"]
        n_trees = calls_of(fns, r"MerkleVerifierLifted<.*>>::verify")
        host_price = hashes * SHA256_CONST + bytes_ * SHA256_PER_BYTE + n_trees * (DISPATCH + WASM_CALL_CPU + MEMCPY_CONST + bytes_ * MEMCPY_PER_BYTE / max(n_trees, 1))
        r["D_merkle_host"] = merkle_incl * CPU_PER_FUEL - host_price
        r["D_merkle_host_price"] = host_price
        r["D_merkle_incl"] = merkle_incl * CPU_PER_FUEL
        # D'. same with Blake2s priced at half of SHA-256 (it is the faster hash)
        r["D2_merkle_host_cheap_hash"] = merkle_incl * CPU_PER_FUEL - (host_price - 0.5 * (hashes * SHA256_CONST + bytes_ * SHA256_PER_BYTE))

        # E. only the hash in the host, one call per hash (measured price of a 64-byte sha256 through the host)
        blake_wasm = fuel_of(fns, r"^<?blake2::|^<?stwo::core::vcs::blake2_hash::") * CPU_PER_FUEL
        all_hashes, all_bytes = oc["totals"]["blake2s_hashes"], oc["totals"]["blake2s_bytes"]
        per_hash_host = mb["sha256 of 64 B through the host"]["cpu_per_op"]
        r["E_hash_per_call"] = blake_wasm - all_hashes * per_hash_host
        # E'. one host call hashing many inputs at once (price: SHA-256 const + per byte, one dispatch per phase)
        r["E2_hash_many"] = blake_wasm - (all_hashes * SHA256_CONST + all_bytes * SHA256_PER_BYTE + 20 * (DISPATCH + WASM_CALL_CPU))
        r["E_blake_wasm"] = blake_wasm

        # F. per-element field host functions. Measured today (bn254_fr_mul): more expensive than a QM31 mul in Wasm.
        # Ideal ABI upper bound: dispatch + the Wasm call + ~100 of marshalling per op, only for the ops that
        # cost more than that in Wasm (QM31 mul and the inverses).
        ideal = DISPATCH + WASM_CALL_CPU + 100
        q = oc["totals"]
        r["F_measured_loss_qm31"] = -(mb["bn254_fr_mul through the host"]["cpu_per_op"] - mb["QM31 mul"]["cpu_per_op"]) * q["qm31_mul"]
        r["F_ideal"] = (max(mb["QM31 mul"]["cpu_per_op"] - ideal, 0) * q["qm31_mul"]
                        + max(mb["QM31 inverse"]["cpu_per_op"] - ideal, 0) * q["qm31_inv"]
                        + max(mb["M31 inverse"]["cpu_per_op"] - ideal, 0) * q["m31_inv"])

        # G. quotients (fri_answers) as one batched host call per query.
        n_queries = {"compact": 9, "standard": 70, "compact-192": 21}[p]
        samples = 108  # (column, point) pairs per query in this AIR
        host_q = n_queries * (DISPATCH + WASM_CALL_CPU + MEMCPY_CONST + samples * 52 * MEMCPY_PER_BYTE + samples * NATIVE_QM31_MUL)
        r["G_quotients_host"] = phase_fuel(ph, "fri_answers") * CPU_PER_FUEL - host_q

        # H. the whole verifier as a host function: native time priced like ed25519 (377,551 CPU for a
        # verification that takes roughly 50-100 us natively => about 4,000-7,500 CPU per us).
        us = nat["native_verify_us_median"]
        r["H_native_low"], r["H_native_high"] = us * 4000, us * 7500
        rows[p] = r
    return rows


def main():
    rows = compute("")
    print_table(rows, "What each lever would save (CPU instructions; baseline = host total for the deployed contract)")
    json.dump(rows, open(RES / "model.json", "w"), indent=1)
    if (RES / "fast-compact-functions.json").exists():
        fast = compute("fast-")
        print_table(fast, "The protocol levers against the `fast` build (what is left for the host to save after the verifier is tuned)",
                    protocol_only=True)
        json.dump(fast, open(RES / "fast-model.json", "w"), indent=1)


def print_table(rows, title, protocol_only=False):
    print(f"\n## {title}\n")
    print("| Lever | kind | " + " | ".join(PROOFS) + " |")
    print("|---|---|" + "---|" * len(PROOFS))

    def line(label, kind, key, as_range=None):
        cells = []
        for p in PROOFS:
            r = rows[p]
            if as_range:
                lo, hi = r[as_range[0]], r[as_range[1]]
                cells.append(f"−{m(r['baseline_cpu'] - hi)} to −{m(r['baseline_cpu'] - lo)} ({-100 * (1 - hi / r['baseline_cpu']):.0f} to {-100 * (1 - lo / r['baseline_cpu']):.0f} %)")
            else:
                v = r[key]
                cells.append(f"{'−' if v >= 0 else '+'}{m(abs(v))} ({-100 * v / r['baseline_cpu']:+.1f} %)")
        print(f"| {label} | {kind} | " + " | ".join(cells) + " |")

    print("| baseline | measured | " + " | ".join(f"{m(rows[p]['baseline_cpu'])}" for p in PROOFS) + " |")
    line("A. Wasm `call` charged 1 fuel instead of 67", "protocol (fuel model), exact", "A_call_1")
    if protocol_only:
        line("D. Merkle path verification host function, hash priced like SHA-256", "protocol (new host function)", "D_merkle_host")
        line("&nbsp;&nbsp;D'. same, Blake2s priced at half of SHA-256", "protocol", "D2_merkle_host_cheap_hash")
        line("&nbsp;&nbsp;E'. hash host function over many inputs per call", "protocol", "E2_hash_many")
        line("G. quotient accumulation as one host call per query", "protocol", "G_quotients_host")
        line("H. the whole Stwo verifier as a host function (native time priced like ed25519)", "protocol, order of magnitude", None, as_range=("H_native_low", "H_native_high"))
        print()
        return
    line("C. verifier engineering, total (C1–C5)", "no protocol change, estimate", "C_engineering")
    line("&nbsp;&nbsp;C1. twiddles from a table instead of a scalar multiplication per butterfly", "estimate (keeps 10 %)", "C1_twiddles")
    line("&nbsp;&nbsp;C2. field arithmetic inlined (its `call` charges)", "exact lower bound", "C2_inline_fields")
    line("&nbsp;&nbsp;C3. half of the copying and allocation", "estimate", "C3_memory_half")
    line("&nbsp;&nbsp;C4. component info pass done at compile time", "estimate (keeps 10 %)", "C4_setup")
    line("&nbsp;&nbsp;C5. fixed-layout proof format", "estimate (keeps 25 %)", "C5_decode")
    line("D. Merkle path verification host function, hash priced like SHA-256", "protocol (new host function)", "D_merkle_host")
    line("&nbsp;&nbsp;D'. same, Blake2s priced at half of SHA-256", "protocol", "D2_merkle_host_cheap_hash")
    line("E. hash host function, one call per hash (today's ABI, measured price)", "protocol", "E_hash_per_call")
    line("&nbsp;&nbsp;E'. hash host function over many inputs per call", "protocol", "E2_hash_many")
    line("F. per-element M31/QM31 host functions, today's ABI (bn254_fr_* price for each QM31 mul)", "protocol, measured price", "F_measured_loss_qm31")
    line("&nbsp;&nbsp;F'. per-element, ideal ABI (dispatch + call + 100), QM31 mul and inverses only", "protocol, upper bound", "F_ideal")
    line("G. quotient accumulation as one host call per query", "protocol", "G_quotients_host")
    line("H. the whole Stwo verifier as a host function (native time priced like ed25519)", "protocol, order of magnitude", None, as_range=("H_native_low", "H_native_high"))
    print()
    print("Inputs behind D and E, per proof: Merkle verification inclusive (fuel×4) "
          + "; ".join(f"{p} {m(rows[p]['D_merkle_incl'])} (host price {m(rows[p]['D_merkle_host_price'])})" for p in PROOFS)
          + ". Blake2s in Wasm: " + "; ".join(f"{p} {m(rows[p]['E_blake_wasm'])}" for p in PROOFS) + ".")
    print()


if __name__ == "__main__":
    main()
