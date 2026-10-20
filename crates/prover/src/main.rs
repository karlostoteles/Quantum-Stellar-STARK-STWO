//! Command-line prover for the Soroban STARK verifier demo.
//!
//!   stwo-soroban-prover prove  --a0 1 --a1 3141592 [--log-rows 4] [--security standard|compact] --out proof.bin
//!                              [--fri pow,log_blowup,queries,fold,log_last]   (instead of --security)
//!   stwo-soroban-prover tamper --in proof.bin --out bad.bin [--mode claim|proof]
//!   stwo-soroban-prover inspect --in proof.bin
use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;

use stwo::core::fields::m31::BaseField;
use stwo::core::fields::FieldExpOps;
use stwo::core::fri::FriConfig;
use stwo::core::poly::circle::CanonicCoset;
use stwo::prover::backend::simd::SimdBackend;
use stwo::prover::backend::{Col, Column};
use stwo::prover::poly::circle::{CircleEvaluation, PolyOps};
use stwo::prover::poly::BitReversedOrder;
use stwo::prover::{prove, CommitmentSchemeProver};
use stwo_soroban_air::{
    component, fibsq_last, mix_statement, pcs_config, verify_envelope, Envelope, MerkleChannel,
    ProofChannel, Statement, SEQ_LEN,
};

fn security_preset(name: &str) -> Result<FriConfig, String> {
    // FriConfig::new(pow_bits, log_last_layer_degree_bound, log_blowup_factor, n_queries, fold_step)
    match name {
        // StarkWare's production default for Stwo-Cairo: 26 + 1 * 70 = 96 bits.
        "standard" => Ok(FriConfig::new(26, 0, 1, 70, 1)),
        // Fewer queries for the same 96 bits: 24 + 8 * 9. About 5x cheaper to verify on Soroban.
        "compact" => Ok(FriConfig::new(24, 0, 8, 9, 3)),
        other => Err(format!("unknown --security preset '{other}' (use standard or compact)")),
    }
}

fn sequence(a0: u32, a1: u32) -> Vec<BaseField> {
    let mut seq = vec![BaseField::from(a0), BaseField::from(a1)];
    while seq.len() < SEQ_LEN {
        let n = seq.len();
        seq.push(seq[n - 2].square() + seq[n - 1].square());
    }
    seq
}

/// One column per term; every row holds the same sequence.
fn trace(log_n_rows: u32, seq: &[BaseField]) -> Vec<CircleEvaluation<SimdBackend, BaseField, BitReversedOrder>> {
    let n = 1usize << log_n_rows;
    let domain = CanonicCoset::new(log_n_rows).circle_domain();
    seq.iter()
        .map(|&v| {
            let mut col = Col::<SimdBackend, BaseField>::zeros(n);
            for i in 0..n {
                col.set(i, v);
            }
            CircleEvaluation::<SimdBackend, _, BitReversedOrder>::new(domain, col)
        })
        .collect()
}

fn prove_statement(statement: Statement, fri_config: FriConfig) -> Result<Envelope, String> {
    let config = pcs_config(fri_config, statement.log_n_rows);
    let twiddles = SimdBackend::precompute_twiddles(
        CanonicCoset::new(statement.log_n_rows + 1 + fri_config.log_blowup_factor)
            .circle_domain()
            .half_coset,
    );
    let channel = &mut ProofChannel::default();
    mix_statement(channel, &statement);
    let mut commitment_scheme = CommitmentSchemeProver::<SimdBackend, MerkleChannel>::new(config, &twiddles);

    // Preprocessed trace: none.
    let mut tree_builder = commitment_scheme.tree_builder();
    tree_builder.extend_evals(vec![]);
    tree_builder.commit(channel);

    // Execution trace.
    let mut tree_builder = commitment_scheme.tree_builder();
    tree_builder.extend_evals(trace(statement.log_n_rows, &sequence(statement.a0, statement.a1)));
    tree_builder.commit(channel);

    let component = component(&statement);
    let proof = prove::<SimdBackend, MerkleChannel>(&[&component], channel, commitment_scheme)
        .map_err(|e| format!("proving failed: {e:?}"))?;
    Ok(Envelope { statement, fri_config, proof })
}

fn args_map(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let key = k.strip_prefix("--").ok_or_else(|| format!("unexpected argument '{k}'"))?;
        let v = it.next().ok_or_else(|| format!("missing value for --{key}"))?;
        map.insert(key.to_string(), v.clone());
    }
    Ok(map)
}

fn num(map: &HashMap<String, String>, key: &str, default: Option<u32>) -> Result<u32, String> {
    match map.get(key) {
        Some(v) => v.parse().map_err(|_| format!("--{key} must be a number")),
        None => default.ok_or_else(|| format!("--{key} is required")),
    }
}

fn read_envelope(path: &str) -> Result<(Vec<u8>, Envelope), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    let env = postcard::from_bytes(&bytes).map_err(|e| format!("{path} is not a proof envelope: {e}"))?;
    Ok((bytes, env))
}

fn summary(env: &Envelope, bytes: usize) -> serde_json::Value {
    let s = env.statement;
    let f = env.fri_config;
    serde_json::json!({
        "statement": { "log_n_rows": s.log_n_rows, "a0": s.a0, "a1": s.a1, "last": s.last, "terms": SEQ_LEN },
        "fri": { "pow_bits": f.pow_bits, "log_blowup_factor": f.log_blowup_factor, "n_queries": f.n_queries,
                 "fold_step": f.fold_step, "log_last_layer_degree_bound": f.log_last_layer_degree_bound,
                 "security_bits": f.security_bits() },
        "proof_bytes": bytes,
    })
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = argv.split_first().ok_or("usage: stwo-soroban-prover <prove|tamper|inspect> [options]")?;
    let map = args_map(rest)?;
    match cmd.as_str() {
        "prove" => {
            let a0 = num(&map, "a0", None)?;
            let a1 = num(&map, "a1", None)?;
            let log_n_rows = num(&map, "log-rows", Some(4))?;
            let fri = match map.get("fri") {
                // --fri pow_bits,log_blowup,n_queries,fold_step,log_last_layer
                Some(spec) => {
                    let v: Vec<u32> = spec.split(',').map(|x| x.trim().parse().map_err(|_| format!("bad --fri '{spec}'"))).collect::<Result<_, _>>()?;
                    let [pow, blowup, queries, fold, last] = v[..] else { return Err("--fri needs 5 numbers: pow,log_blowup,queries,fold,log_last".into()) };
                    FriConfig::new(pow, last, blowup, queries as usize, fold)
                }
                None => security_preset(map.get("security").map(String::as_str).unwrap_or("standard"))?,
            };
            let out = map.get("out").ok_or("--out is required")?;
            let statement = Statement { log_n_rows, a0, a1, last: fibsq_last(a0, a1) };
            let t = Instant::now();
            let env = prove_statement(statement, fri)?;
            let prove_ms = t.elapsed().as_millis();
            let bytes = postcard::to_allocvec(&env).map_err(|e| e.to_string())?;
            let t = Instant::now();
            verify_envelope(env.clone()).map_err(|e| format!("self-check failed: {e:?}"))?;
            let verify_us = t.elapsed().as_micros();
            std::fs::write(out, &bytes).map_err(|e| format!("cannot write {out}: {e}"))?;
            let mut s = summary(&env, bytes.len());
            s["prove_ms"] = (prove_ms as u64).into();
            s["native_verify_us"] = (verify_us as u64).into();
            s["file"] = out.clone().into();
            println!("{}", serde_json::to_string_pretty(&s).unwrap());
        }
        "tamper" => {
            let input = map.get("in").ok_or("--in is required")?;
            let out = map.get("out").ok_or("--out is required")?;
            let (_, mut env) = read_envelope(input)?;
            match map.get("mode").map(String::as_str).unwrap_or("proof") {
                // Same proof, false claim about the result.
                "claim" => env.statement.last = (env.statement.last + 1) % stwo::core::fields::m31::P,
                // Same claim, one queried trace value changed.
                "proof" => {
                    let col = env.proof.0.queried_values[1]
                        .iter_mut()
                        .find(|c| !c.is_empty())
                        .ok_or("proof has no queried trace values")?;
                    col[0] = col[0] + BaseField::from(1);
                }
                other => return Err(format!("unknown --mode '{other}' (use claim or proof)")),
            }
            let bytes = postcard::to_allocvec(&env).map_err(|e| e.to_string())?;
            let native = verify_envelope(env.clone());
            std::fs::write(out, &bytes).map_err(|e| format!("cannot write {out}: {e}"))?;
            println!("{}", serde_json::json!({ "file": out, "native_verify": format!("{native:?}") }));
        }
        "inspect" => {
            let input = map.get("in").ok_or("--in is required")?;
            let (bytes, env) = read_envelope(input)?;
            let mut s = summary(&env, bytes.len());
            s["native_verify"] = format!("{:?}", verify_envelope(env)).into();
            println!("{}", serde_json::to_string_pretty(&s).unwrap());
        }
        other => return Err(format!("unknown command '{other}'")),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
