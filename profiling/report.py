#!/usr/bin/env python3
"""Turns profiling/results/*.json into the Markdown tables of the cost report (stdout).

    python3 profiling/report.py > profiling/results/REPORT.md
"""
import json
import pathlib
import re

RES = pathlib.Path(__file__).resolve().parent / "results"
PROOFS = ["compact", "standard", "compact-192"]
CPU_PER_FUEL = 4
# Soroban host cost parameters (soroban-env-host 29.0.1, src/budget.rs), in CPU instructions.
DISPATCH = 295
SHA256_CONST, SHA256_PER_BYTE = 3636, 7013 / 128
MEMCPY_CONST, MEMCPY_PER_BYTE = 42, 1 / 8
CALL_FUEL = 67

CATEGORIES = [  # matched in order
    ("proof decoding (postcard/serde; the rest of it is inlined into `verify`)", r"postcard|serde"),
    ("Merkle verifier, own code (hashing excluded)", r"MerkleVerifierLifted"),
    ("verifier glue: verify, verify_envelope, verify_values (quotients inlined here)", r"^verify$|verify_envelope|CommitmentSchemeVerifier"),
    ("Blake2s: compress and hasher wrapper", r"blake2::|blake2_hash::|Blake2sHasherGeneric"),
    ("twiddles and domain points (CirclePoint, Coset, domains)", r"circle::|CircleDomain|LineDomain|poly::"),
    ("field arithmetic functions (CM31, QM31, M31 inverse)", r"^<?stwo::core::fields::"),
    ("memcpy, memset, allocator", r"memcpy|memset|__rust_alloc|__rust_realloc|__rust_dealloc|RawVec|finish_grow|grow_amortized|BumpPointer|::reserve|::clone"),
    ("constraint framework (component info pass, constraint evaluation)", r"stwo_constraint_framework"),
    ("FRI, own code (folding glue, witness rebuild)", r"stwo::core::fri|fft::ibutterfly|queries::"),
    ("sorting, hash maps, iterators", r"sort|hashbrown|core::iter|itertools"),
    ("channel (Fiat-Shamir)", r"channel"),
]


def load(name):
    return json.load(open(RES / name))


def fmt_m(x):
    return f"{x / 1e6:.2f} M"


def pct(a, b):
    return f"{100 * a / b:.1f} %"


def main():
    print("# Where the cost of verifying a Stwo proof on Soroban goes\n")
    print("Measured with soroban-env-host 29.0.1 (protocol 29 metering) and the deployed contract code "
          "(same build, function names kept). One wasmi fuel = 4 CPU instructions.\n")

    # ---- totals
    print("## Totals\n")
    print("| Proof | Proof size | CPU instructions (host total) | Wasm fuel | ×4 = Wasm CPU | Wasm calls executed | Native verify (x86, 2.8 GHz) |")
    print("|---|---|---|---|---|---|---|")
    for p in PROOFS:
        f = load(f"{p}-functions.json")
        nat = load(f"{p}-native.json")
        print(f"| {p} | {nat['proof_bytes']:,} B | {f['cpu_instructions']:,} | {f['wasm_fuel_total']:,} | "
              f"{CPU_PER_FUEL * f['wasm_fuel_total']:,} | {f['wasm_calls_total']:,} | {nat['native_verify_us_median']} µs |")
    print()

    # ---- the fast build, if measured
    if (RES / "fast-compact-functions.json").exists():
        print("## The `fast` build (PATCHES.md change 6) against the deployed code\n")
        print("| Proof | deployed: CPU | fast: CPU | change | deployed: Wasm calls | fast: calls | native µs (deployed → fast) |")
        print("|---|---|---|---|---|---|---|")
        for p in PROOFS:
            b, f = load(f"{p}-functions.json"), load(f"fast-{p}-functions.json")
            bn, fn = load(f"{p}-native.json"), load(f"fast-{p}-native.json")
            print(f"| {p} | {b['cpu_instructions']:,} | {f['cpu_instructions']:,} | {100 * (f['cpu_instructions'] / b['cpu_instructions'] - 1):+.1f} % | "
                  f"{b['wasm_calls_total']:,} | {f['wasm_calls_total']:,} | {bn['native_verify_us_median']} → {fn['native_verify_us_median']} |")
        print()
        print("By phase (fuel), deployed → fast:\n")
        print("| Phase | " + " | ".join(f"{p}" for p in PROOFS) + " |")
        print("|---|" + "---|" * len(PROOFS))
        bph = {p: load(f"{p}-phases.json") for p in PROOFS}
        fph = {p: load(f"fast-{p}-phases.json") for p in PROOFS}
        for k, name in enumerate(bph["compact"]["phases"]):
            row = f"| {k}. {name} |"
            for p in PROOFS:
                bc, fc = bph[p]["cumulative"], fph[p]["cumulative"]
                b = bc[k]["fuel"] - (bc[k - 1]["fuel"] if k else 0)
                f = fc[k]["fuel"] - (fc[k - 1]["fuel"] if k else 0)
                row += f" {b:,} → {f:,} ({100 * (f / b - 1):+.0f} %) |"
            print(row)
        print()
        for p in PROOFS[:2]:
            f = load(f"fast-{p}-functions.json")
            tot = f["wasm_fuel_total"]
            print(f"Top functions of the fast build, {p}:\n")
            print("| self fuel | share | inclusive | calls | function |")
            print("|---|---|---|---|---|")
            for fn in f["functions"][:12]:
                name = re.sub(r"<(.*?) as .*?>::", r"\1::", fn["name"])
                name = re.sub(r"stwo::core::", "", name).replace("compiler_builtins::mem::", "")
                if len(name) > 90:
                    name = name[:87] + "…"
                print(f"| {fn['self_fuel']:,} | {pct(fn['self_fuel'], tot)} | {fn['inclusive_fuel']:,} | {fn['calls']:,} | `{name}` |")
            print()
        print("Fuel by instruction class, fast build:\n")
        print("| Proof | call (×67) share | call instructions charged | calls executed | total if call cost 1 |")
        print("|---|---|---|---|---|")
        for p in PROOFS:
            c = load(f"fast-{p}-classes.json")
            x = c["fixed"]
            C = c["call"] - x
            t = c["total"]
            print(f"| {p} | {pct(67 * C, t)} | {C:,} | {load(f'fast-{p}-functions.json')['wasm_calls_total']:,} | {t - 66 * C:,} ({pct(-(66 * C), t)}) |")
        print()

    # ---- phases
    print("## By phase of the verifier (deployed code)\n")
    print("Cumulative cost after each phase (probe build, `verify_phase`), differenced. Fuel; share of the phased total.\n")
    ph = {p: load(f"{p}-phases.json") for p in PROOFS}
    names = ph["compact"]["phases"]
    header = "| Phase | " + " | ".join(f"{p} fuel | {p} share" for p in PROOFS) + " |"
    print(header)
    print("|---|" + "---|---|" * len(PROOFS))
    totals = {p: ph[p]["cumulative"][-1]["fuel"] for p in PROOFS}
    for k, name in enumerate(names):
        row = f"| {k}. {name} |"
        for p in PROOFS:
            cum = ph[p]["cumulative"]
            d = cum[k]["fuel"] - (cum[k - 1]["fuel"] if k > 0 else 0)
            row += f" {d:,} | {pct(d, totals[p])} |"
        print(row)
    row = "| total (phased) |"
    for p in PROOFS:
        row += f" {totals[p]:,} | — |"
    print(row)
    row = "| `verify` in the same build |"
    for p in PROOFS:
        row += f" {ph[p]['verify_probe_build']['fuel']:,} | |"
    print(row)
    print()

    # ---- categories
    print("## By kind of code (self fuel of the functions, deployed build)\n")
    print("Inlined callees count towards their caller, so these are lower bounds for leaf work and upper "
          "bounds for the big functions.\n")
    print("| Category | " + " | ".join(f"{p} fuel | {p} share" for p in PROOFS) + " |")
    print("|---|" + "---|---|" * len(PROOFS))
    cats = {p: {} for p in PROOFS}
    for p in PROOFS:
        f = load(f"{p}-functions.json")
        rest = 0
        for fn in f["functions"]:
            for cname, pat in CATEGORIES:
                if re.search(pat, fn["name"]):
                    cats[p][cname] = cats[p].get(cname, 0) + fn["self_fuel"]
                    break
            else:
                rest += fn["self_fuel"]
        cats[p]["everything else"] = rest
    for cname, _ in CATEGORIES + [("everything else", None)]:
        row = f"| {cname} |"
        for p in PROOFS:
            v = cats[p].get(cname, 0)
            row += f" {v:,} | {pct(v, load(f'{p}-functions.json')['wasm_fuel_total'])} |"
        print(row)
    print()

    # ---- top functions
    for p in PROOFS[:2]:
        f = load(f"{p}-functions.json")
        tot = f["wasm_fuel_total"]
        print(f"### Top functions, {p} (self fuel, inclusive fuel, calls)\n")
        print("| self fuel | share | inclusive | calls | function |")
        print("|---|---|---|---|---|")
        for fn in f["functions"][:20]:
            name = re.sub(r"<(.*?) as .*?>::", r"\1::", fn["name"])
            name = re.sub(r"stwo::core::", "", name)
            name = name.replace("stwo::core::fields::", "").replace("compiler_builtins::mem::", "")
            if len(name) > 90:
                name = name[:87] + "…"
            print(f"| {fn['self_fuel']:,} | {pct(fn['self_fuel'], tot)} | {fn['inclusive_fuel']:,} | {fn['calls']:,} | `{name}` |")
        print()

    # ---- instruction classes
    print("## By instruction class (wasmi fuel weights: base 1, entity 3, load 2, store 1, call 67)\n")
    print("| Proof | base | entity (×3) | load (×2) | store | call (×67) | locals/branch term | total | call instructions charged | calls executed | total if call cost 1 |")
    print("|---|---|---|---|---|---|---|---|---|---|---|")
    for p in PROOFS:
        c = load(f"{p}-classes.json")
        x = c["fixed"]
        B, E, L, S, C = (c[k] - x for k in ["base", "entity", "load", "store", "call"])
        t = c["total"]
        assert B + 3 * E + 2 * L + S + 67 * C + x == t, (p, B + 3 * E + 2 * L + S + 67 * C + x, t)
        calls = load(f"{p}-functions.json")["wasm_calls_total"]
        print(f"| {p} | {pct(B, t)} | {pct(3 * E, t)} | {pct(2 * L, t)} | {pct(S, t)} | {pct(67 * C, t)} | {pct(x, t)} | {t:,} | "
              f"{C:,} | {calls:,} | {t - 66 * C:,} ({pct(-(66 * C), t)}) |")
    print("\nwasmi charges a block's fuel when the block is entered, so instructions after a taken branch "
          "inside the block are paid for but not executed: that is the gap between call instructions charged "
          "and calls executed.\n")

    # ---- micro-benchmarks
    print("## Cost per operation (micro-benchmarks, `bench` entry point, same build flags)\n")
    mb = load("microbench.json")
    print("| Operation | Wasm fuel per op | CPU per op (host costs included) | Wasm calls per op |")
    print("|---|---|---|---|")
    for name, v in mb.items():
        print(f"| {name} | {v['fuel_per_op']:.0f} | {v['cpu_per_op']:,.0f} | {v['wasm_calls_per_op']:.1f} |")
    print("\n`loop only` is the loop overhead to subtract from the cheap operations.\n")

    # ---- operation counts
    print("## Operations executed (native run of the same verifier, `count-ops`)\n")
    for p in PROOFS:
        oc = load(f"{p}-opcount.json")
        t = oc["totals"]
        print(f"- {p}: M31 mul {t['m31_mul']:,}, add {t['m31_add']:,}, sub {t['m31_sub']:,}, inverse {t['m31_inv']:,}; "
              f"CM31 mul {t['cm31_mul']:,}; QM31 mul {t['qm31_mul']:,}, inverse {t['qm31_inv']:,}; "
              f"Blake2s hashes {t['blake2s_hashes']:,} ({t['blake2s_bytes']:,} bytes, {t['blake2s_compressions']:,} compressions).")
    print()
    oc = load("compact-opcount.json")
    print("Per phase, compact proof:\n")
    print("| phase | M31 mul | M31 add+sub | M31 inv | CM31 mul | QM31 mul | Blake2s hashes | bytes hashed | compressions |")
    print("|---|---|---|---|---|---|---|---|---|")
    for r in oc["phases"]:
        if r["phase"] == "rest":
            continue
        print(f"| {r['phase']} | {r['m31_mul']:,} | {r['m31_add'] + r['m31_sub']:,} | {r['m31_inv']} | {r['cm31_mul']:,} | "
              f"{r['qm31_mul']:,} | {r['blake2s_hashes']} | {r['blake2s_bytes']:,} | {r['blake2s_compressions']} |")
    print()


if __name__ == "__main__":
    main()
