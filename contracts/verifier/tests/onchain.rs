//! Runs the compiled contract (WASM) inside the Soroban host, with the network's metering, and
//! checks real proofs and tampered ones.
//!
//! Build the contract and the proofs first (see README), then: cargo test -p stwo-soroban-verifier -- --nocapture
use soroban_sdk::{Bytes, Env};

mod verifier {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/contract/stwo_soroban_verifier.wasm");
}

const TX_MAX_INSTRUCTIONS: i64 = 400_000_000;
const TX_MAX_SIZE_BYTES: usize = 132_096;

fn proof_bytes(name: &str) -> Vec<u8> {
    let path = format!("{}/../../proofs/{name}.bin", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing {path}: {e}. Run the prover first."))
}

struct Outcome {
    result: Result<verifier::Verified, u32>,
    instructions: i64,
    mem_bytes: i64,
    fee_stroops: i64,
    proof_len: usize,
}

fn run(name: &str) -> Outcome {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let id = env.register(verifier::WASM, ());
    let client = verifier::Client::new(&env, &id);
    let raw = proof_bytes(name);
    let proof = Bytes::from_slice(&env, &raw);
    let result = match client.try_verify(&proof) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => panic!("{name}: could not decode the return value: {e:?}"),
        Err(Ok(e)) => Err(e as u32),
        Err(Err(e)) => panic!("{name}: host error: {e:?}"),
    };
    let res = env.cost_estimate().resources();
    let fee = env.cost_estimate().fee();
    let out = Outcome {
        result,
        instructions: res.instructions,
        mem_bytes: res.mem_bytes,
        fee_stroops: fee.total,
        proof_len: raw.len(),
    };
    println!(
        "{name:<22} {:<14} proof {:>6} B  instructions {:>12}  ({:>5.1}% of tx limit)  memory {:>10} B  fee ~{} stroops",
        match &out.result { Ok(v) => format!("OK {} bits", v.security_bits), Err(c) => format!("rejected ({c})") },
        out.proof_len,
        out.instructions,
        100.0 * out.instructions as f64 / TX_MAX_INSTRUCTIONS as f64,
        out.mem_bytes,
        out.fee_stroops,
    );
    out
}

#[test]
fn standard_proof_verifies_within_limits() {
    let out = run("standard");
    let v = out.result.expect("valid proof rejected");
    assert_eq!((v.a0, v.a1, v.last, v.security_bits), (1, 3141592, 1476137047, 96));
    assert!(out.instructions < TX_MAX_INSTRUCTIONS);
    assert!(out.proof_len < TX_MAX_SIZE_BYTES);
}

#[test]
fn compact_proof_verifies_within_limits() {
    let out = run("compact");
    let v = out.result.expect("valid proof rejected");
    assert_eq!((v.a0, v.a1, v.last, v.security_bits), (1, 3141592, 1476137047, 96));
    assert!(out.instructions < TX_MAX_INSTRUCTIONS);
}

#[test]
fn false_claim_is_rejected() {
    // Same proof, `last` changed by one: the transcript no longer matches.
    assert_eq!(run("compact-false-claim").result.err(), Some(4));
}

#[test]
fn altered_proof_is_rejected() {
    // Same claim, one queried trace value changed: the Merkle check fails.
    assert_eq!(run("compact-bad-proof").result.err(), Some(4));
    assert_eq!(run("standard-bad-proof").result.err(), Some(4));
}

#[test]
fn garbage_is_rejected() {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let id = env.register(verifier::WASM, ());
    let client = verifier::Client::new(&env, &id);
    let res = client.try_verify(&Bytes::from_slice(&env, b"not a proof"));
    assert!(matches!(res, Err(Ok(e)) if e as u32 == 1));
}

/// Cost of one proof file with the full budget breakdown:
/// PROOF=proofs/x.bin cargo test -p stwo-soroban-verifier --test onchain measure -- --ignored --nocapture
#[test]
#[ignore]
fn measure() {
    let path = std::env::var("PROOF").expect("set PROOF=<path to proof.bin>");
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    if std::env::var("NO_LIMITS").is_ok() {
        env.cost_estimate().disable_resource_limits();
    }
    let id = env.register(verifier::WASM, ());
    let client = verifier::Client::new(&env, &id);
    let raw = std::fs::read(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let res = client.try_verify(&Bytes::from_slice(&env, &raw));
    let r = env.cost_estimate().resources();
    println!("{path}: ok={} proof={} B instructions={} mem={} fee={}", matches!(res, Ok(Ok(_))), raw.len(), r.instructions, r.mem_bytes, env.cost_estimate().fee().total);
    if std::env::var("BREAKDOWN").is_ok() {
        env.cost_estimate().budget().print();
    }
}

/// Profiling (needs the contract built with --features probe):
/// PROOF=proofs/compact.bin cargo test -p stwo-soroban-verifier --test onchain probe_decode -- --ignored --nocapture
#[test]
#[ignore]
fn probe_decode() {
    let path = std::env::var("PROOF").expect("set PROOF=<path to proof.bin>");
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let id = env.register(verifier::WASM, ());
    let raw = std::fs::read(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let args = soroban_sdk::vec![&env, soroban_sdk::IntoVal::into_val(&Bytes::from_slice(&env, &raw), &env)];
    let n: u32 = env.invoke_contract(&id, &soroban_sdk::Symbol::new(&env, "decode_only"), args);
    let r = env.cost_estimate().resources();
    println!("{path}: decode_only commitments={n} instructions={}", r.instructions);
}
