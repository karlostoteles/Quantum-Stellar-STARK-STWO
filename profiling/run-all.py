#!/usr/bin/env python3
"""Runs every measurement behind profiling/results/ and writes the JSON files report.py reads.

    python3 profiling/run-all.py                 # the deployed verifier (builds the two named contracts first)
    python3 profiling/run-all.py --variant fast  # the `fast` build (results are prefixed fast-)
    python3 profiling/run-all.py --no-build      # reuse the contracts already in profiling/results/

Needs: the nightly toolchain of rust-toolchain.toml with rust-src, the profiler built
(`cargo build --release` inside profiling/), the prover built plain
(`cargo build --release -p stwo-soroban-prover`) and with the counters
(`cargo build --release -p stwo-soroban-prover --features op-count --target-dir target/op-count`).
"""
import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
RES = ROOT / "profiling" / "results"
VARIANT = sys.argv[sys.argv.index("--variant") + 1] if "--variant" in sys.argv else ""
PREFIX = f"{VARIANT}-" if VARIANT else ""
PROFILER = ROOT / "profiling" / "target" / "release" / "stwo-soroban-profiler"
# Provers: plain for the native timing, with --features op-count (own target dir, so the counters do
# not slow the timing) for the operation counts; the fast variant adds --features fast to both:
#   cargo build --release -p stwo-soroban-prover
#   cargo build --release -p stwo-soroban-prover --features op-count --target-dir target/op-count
#   cargo build --release -p stwo-soroban-prover --features fast --target-dir target/fast
#   cargo build --release -p stwo-soroban-prover --features fast,op-count --target-dir target/fast-op-count
PROVER = ROOT / "target" / ("fast" if VARIANT == "fast" else ".") / "release" / "stwo-soroban-prover"
PROVER_OPCOUNT = ROOT / "target" / ("fast-op-count" if VARIANT == "fast" else "op-count") / "release" / "stwo-soroban-prover"
PLAIN = RES / f"verifier-{PREFIX}named.wasm"        # the contract's code, with function names
PROBE = RES / f"verifier-{PREFIX}probe-named.wasm"  # + the profiling entry points (verify_phase, bench)
PROOFS = ["compact", "standard", "compact-192"]
PHASES = ["decode", "setup_component", "commit_trace_trees", "oods_pow_and_point", "composition_eval",
          "mix_sampled_values", "fri_commit", "fri_pow_and_queries", "merkle_trace_trees", "fri_answers",
          "fri_decommit"]
BENCH_OPS = {0: "loop only", 1: "M31 add", 2: "M31 mul", 3: "M31 inverse", 4: "CM31 mul", 5: "QM31 add",
             6: "QM31 mul", 7: "QM31 mul by M31", 8: "QM31 inverse", 9: "Blake2s of 64 B (one compression)",
             10: "circle point by scalar (a FRI twiddle)", 11: "bn254_fr_mul through the host",
             12: "bn254_fr_add through the host", 13: "sha256 of 64 B through the host"}
SLOW_OPS = {3, 8, 10, 11, 12, 13}


def sh(cmd, env=None, cwd=ROOT):
    e = dict(os.environ)
    if env:
        e.update(env)
    out = subprocess.run(cmd, cwd=cwd, env=e, capture_output=True, text=True)
    if out.returncode != 0:
        sys.stderr.write(out.stdout + out.stderr)
        raise SystemExit(f"failed: {' '.join(map(str, cmd))}")
    return out.stdout + out.stderr


def build_contracts():
    """Named builds through scripts/build-contract.sh (NAMED=1 keeps the function names)."""
    env = {"NAMED": "1"}
    if VARIANT == "fast":
        env["FAST"] = "1"
    out = ROOT / "target" / "wasm32v1-none" / "contract-named" / "stwo_soroban_verifier.wasm"
    sh([str(ROOT / "scripts" / "build-contract.sh")], env)
    PLAIN.write_bytes(out.read_bytes())
    sh([str(ROOT / "scripts" / "build-contract.sh")], {**env, "CONTRACT_FEATURES": "probe"})
    PROBE.write_bytes(out.read_bytes())


def run(wasm, fn, proof=None, u32s=(), env=None):
    cmd = [str(PROFILER), "--wasm", str(wasm), "--fn", fn, "--top", "0"]
    if proof:
        cmd += ["--proof", str(ROOT / "proofs" / f"{proof}.bin")]
    for n in u32s:
        cmd += ["--u32", str(n)]
    out = sh(cmd, env)
    m = re.search(r"ok=(\w+) cpu=(\d+) wasm_fuel=(\d+) .* calls=(\d+)", out)
    assert m, out
    return {"ok": m.group(1) == "true", "cpu": int(m.group(2)), "fuel": int(m.group(3)), "calls": int(m.group(4))}


def main():
    RES.mkdir(exist_ok=True)
    if "--no-build" not in sys.argv:
        build_contracts()
    for proof in PROOFS:
        # Per-function profile of the deployed code.
        sh([str(PROFILER), "--wasm", str(PLAIN), "--fn", "verify", "--proof", str(ROOT / "proofs" / f"{proof}.bin"),
            "--top", "0", "--out", str(RES / f"{PREFIX}{proof}-functions.json")])
        # Phases (probe build): cumulative cost after each phase.
        cum = [run(PROBE, "verify_phase", proof, (k,)) for k in range(len(PHASES))]
        full = run(PROBE, "verify", proof)
        json.dump({"phases": PHASES, "cumulative": cum, "verify_probe_build": full},
                  open(RES / f"{PREFIX}{proof}-phases.json", "w"), indent=1)
        # Fuel by instruction class (deployed code): one weight at a time, plus the fixed term.
        classes = {}
        for name, spec in [("base", "1,0,0,0,0"), ("entity", "0,1,0,0,0"), ("load", "0,0,1,0,0"),
                           ("store", "0,0,0,1,0"), ("call", "0,0,0,0,1"), ("fixed", "0,0,0,0,0")]:
            classes[name] = run(PLAIN, "verify", proof, env={"WASMI_FUEL_COSTS": spec})["fuel"]
        classes["total"] = run(PLAIN, "verify", proof)["fuel"]
        json.dump(classes, open(RES / f"{PREFIX}{proof}-classes.json", "w"), indent=1)
        # Operation counts (native, same code path).
        out = sh([str(PROVER_OPCOUNT), "count-ops", "--in", str(ROOT / "proofs" / f"{proof}.bin")])
        (RES / f"{PREFIX}{proof}-opcount.json").write_text(out)
        # Native verification time.
        out = sh([str(PROVER), "inspect", "--in", str(ROOT / "proofs" / f"{proof}.bin")])
        (RES / f"{PREFIX}{proof}-native.json").write_text(out)
        print(proof, "done", file=sys.stderr)
    # Micro-benchmarks: cost per operation = (cost(2n) - cost(n)) / n.
    bench = {}
    for op, name in BENCH_OPS.items():
        n = 200 if op in SLOW_OPS else 2000
        a, b = run(PROBE, "bench", u32s=(op, n)), run(PROBE, "bench", u32s=(op, 2 * n))
        bench[name] = {"op": op, "n": n, "fuel_per_op": (b["fuel"] - a["fuel"]) / n,
                       "cpu_per_op": (b["cpu"] - a["cpu"]) / n, "wasm_calls_per_op": (b["calls"] - a["calls"]) / n}
    json.dump(bench, open(RES / f"{PREFIX}microbench.json", "w"), indent=1)
    print("all done", file=sys.stderr)


if __name__ == "__main__":
    main()
