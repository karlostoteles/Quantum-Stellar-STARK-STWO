//! Runs one contract call inside the Soroban host (soroban-env-host 29, the network's metering)
//! and reports the Wasm fuel charged to each function of the contract: self fuel (charged while
//! that function's own instructions run, inlined callees included), inclusive fuel (self plus
//! everything called from it) and the number of calls into it.
//!
//!   profiler --wasm <contract.wasm> [--fn verify] --proof <proof.bin> [--u32 <n>]... [--out profile.json]
//!
//! Arguments are passed in order: --proof adds the file as Bytes, each --u32 adds a u32.
//!
//! Fuel is Soroban's calibrated wasmi fuel (base 1, entity 3, load 2, store 1, call 67; one fuel
//! is charged as 4 CPU instructions). WASMI_FUEL_COSTS=base,entity,load,store,call overrides the
//! five weights, which isolates one instruction class at a time (the totals then stop matching
//! the network's bill, but the per-function counts are exact).
//!
//! The contract must keep its `name` section (build it with strip = "none").
use std::collections::HashMap;

use serde::Serialize;
use soroban_sdk::{Bytes, Env, IntoVal, Symbol, Val};

#[derive(Serialize)]
struct FuncRow {
    index: usize,
    name: String,
    self_fuel: u64,
    inclusive_fuel: u64,
    calls: u64,
}

#[derive(Serialize)]
struct Report {
    wasm: String,
    entry: String,
    argument: String,
    ok: bool,
    cpu_instructions: i64,
    mem_bytes: i64,
    wasm_fuel_total: u64,
    wasm_calls_total: u64,
    budget: String,
    functions: Vec<FuncRow>,
}

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned())
}

/// Function names from the Wasm `name` section, by Wasm function index (imports included).
fn function_names(wasm: &[u8]) -> (usize, HashMap<u32, String>) {
    let mut names = HashMap::new();
    let mut n_imports = 0usize;
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("wasm parse") {
            wasmparser::Payload::ImportSection(reader) => {
                for import in reader {
                    if let wasmparser::TypeRef::Func(_) = import.expect("import").ty {
                        n_imports += 1;
                    }
                }
            }
            wasmparser::Payload::CustomSection(reader) if reader.name() == "name" => {
                let name_reader = wasmparser::NameSectionReader::new(reader.data(), reader.data_offset());
                for name in name_reader {
                    if let wasmparser::Name::Function(map) = name.expect("name section") {
                        for naming in map {
                            let naming = naming.expect("naming");
                            names.insert(naming.index, format!("{:#}", rustc_demangle::demangle(naming.name)));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (n_imports, names)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let wasm_path = arg(&args, "--wasm").expect("--wasm <contract.wasm>");
    let wasm = std::fs::read(&wasm_path).expect("read wasm");
    let (n_imports, names) = function_names(&wasm);

    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.cost_estimate().disable_resource_limits();
    let id = env.register(&wasm[..], ());

    // Arguments, in order: --proof <file> adds the file as Bytes, each --u32 <n> adds a u32.
    let entry = arg(&args, "--fn").unwrap_or_else(|| "verify".to_string());
    let mut call_args: soroban_sdk::Vec<Val> = soroban_sdk::vec![&env];
    let mut argument = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--proof" => {
                let raw = std::fs::read(&args[i + 1]).expect("read proof");
                call_args.push_back(Bytes::from_slice(&env, &raw).into_val(&env));
                argument += &format!("{} ", args[i + 1]);
                i += 2;
            }
            "--u32" => {
                let n: u32 = args[i + 1].parse().expect("--u32 <n>");
                call_args.push_back(n.into_val(&env));
                argument += &format!("{n} ");
                i += 2;
            }
            _ => i += 1,
        }
    }
    let argument = argument.trim().to_string();

    soroban_wasmi::profile::enable();
    let res = env.try_invoke_contract::<Val, Val>(&id, &Symbol::new(&env, &entry), call_args);
    let profile = soroban_wasmi::profile::take();
    let ok = matches!(res, Ok(Ok(_)));
    if !ok {
        eprintln!("call did not succeed: {res:?}");
    }

    let resources = env.cost_estimate().resources();
    let budget = env.cost_estimate().budget();
    let budget_text = format!("{budget}");
    let wasm_fuel_total: u64 = profile.iter().map(|p| p.self_fuel).sum();
    let wasm_calls_total: u64 = profile.iter().map(|p| p.calls).sum();

    let mut functions: Vec<FuncRow> = profile
        .into_iter()
        .filter(|p| p.self_fuel > 0 || p.inclusive_fuel > 0 || p.calls > 0)
        .map(|p| FuncRow {
            index: p.func,
            name: names
                .get(&((p.func + n_imports) as u32))
                .cloned()
                .unwrap_or_else(|| format!("func[{}]", p.func + n_imports)),
            self_fuel: p.self_fuel,
            inclusive_fuel: p.inclusive_fuel,
            calls: p.calls,
        })
        .collect();
    functions.sort_by(|a, b| b.self_fuel.cmp(&a.self_fuel));

    let report = Report {
        wasm: wasm_path,
        entry,
        argument,
        ok,
        cpu_instructions: resources.instructions,
        mem_bytes: resources.mem_bytes,
        wasm_fuel_total,
        wasm_calls_total,
        budget: budget_text,
        functions,
    };
    if let Some(out) = arg(&args, "--out") {
        std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).expect("write report");
    }
    eprintln!(
        "{} {}: ok={} cpu={} wasm_fuel={} (x4 = {}) calls={}",
        report.entry, report.argument, report.ok, report.cpu_instructions, report.wasm_fuel_total,
        4 * report.wasm_fuel_total, report.wasm_calls_total
    );
    let top = arg(&args, "--top").map(|t| t.parse().unwrap()).unwrap_or(40usize);
    println!("{:>12} {:>6} {:>12} {:>9}  name", "self_fuel", "%", "incl_fuel", "calls");
    for f in report.functions.iter().take(top) {
        println!(
            "{:>12} {:>5.1}% {:>12} {:>9}  {}",
            f.self_fuel,
            100.0 * f.self_fuel as f64 / report.wasm_fuel_total.max(1) as f64,
            f.inclusive_fuel,
            f.calls,
            f.name
        );
    }
}
