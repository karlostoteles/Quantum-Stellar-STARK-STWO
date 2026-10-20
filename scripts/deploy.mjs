// Deploys the verifier and sends verification transactions.
//
//   node scripts/deploy.mjs <testnet|mainnet> deploy                 uploads the WASM and creates the contract
//   node scripts/deploy.mjs <testnet|mainnet> verify <proof.bin>     sends a transaction that verifies a proof
//   node scripts/deploy.mjs <testnet|mainnet> reject <proof.bin>     sends a tampered proof on chain anyway,
//                                                                    to record that the contract rejects it
//
// Keys live in .keys/<network>.json (git-ignored). On testnet the account is funded by friendbot;
// on mainnet it must be funded by hand. Results are appended to deployments/<network>.json.
//
// VARIANT=keccak uses the build from `KECCAK=1 ./scripts/build-contract.sh` and logs to
// deployments/<network>-keccak.json, so it is deployed as a separate contract.
import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  Address, Contract, Keypair, Networks, Operation, TransactionBuilder, nativeToScVal, scValToNative, rpc,
} from "@stellar/stellar-sdk";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const VARIANT = process.env.VARIANT || "";
if (!["", "keccak"].includes(VARIANT)) throw new Error(`unknown VARIANT ${VARIANT}`);
const WASM = join(ROOT, `target/wasm32v1-none/${VARIANT ? `contract-${VARIANT}` : "contract"}/stwo_soroban_verifier.wasm`);
const NETS = {
  testnet: { rpc: "https://soroban-testnet.stellar.org", passphrase: Networks.TESTNET, explorer: "https://stellar.expert/explorer/testnet" },
  mainnet: { rpc: "https://mainnet.sorobanrpc.com", passphrase: Networks.PUBLIC, explorer: "https://stellar.expert/explorer/public" },
};

const [netName, cmd, file] = process.argv.slice(2);
const net = NETS[netName];
if (!net || !["deploy", "verify", "reject"].includes(cmd)) {
  console.error("usage: node scripts/deploy.mjs <testnet|mainnet> <deploy|verify|reject> [proof.bin]");
  process.exit(1);
}
const server = new rpc.Server(net.rpc);
const keysDir = join(ROOT, ".keys");
const logDir = join(ROOT, "deployments");
const logPath = join(logDir, `${netName}${VARIANT ? `-${VARIANT}` : ""}.json`);
const log = existsSync(logPath) ? JSON.parse(readFileSync(logPath, "utf8")) : { network: netName, events: [] };

function record(entry) {
  mkdirSync(logDir, { recursive: true });
  log.events.push({ at: new Date().toISOString(), ...entry });
  writeFileSync(logPath, JSON.stringify(log, null, 2) + "\n");
}

async function keypair() {
  mkdirSync(keysDir, { recursive: true });
  const path = join(keysDir, `${netName}.json`);
  if (existsSync(path)) return Keypair.fromSecret(JSON.parse(readFileSync(path, "utf8")).secret);
  const kp = Keypair.random();
  writeFileSync(path, JSON.stringify({ public: kp.publicKey(), secret: kp.secret() }, null, 2), { mode: 0o600 });
  if (netName === "testnet") {
    const r = await fetch(`https://friendbot.stellar.org/?addr=${kp.publicKey()}`);
    if (!r.ok) throw new Error(`friendbot failed: ${r.status} ${await r.text()}`);
  } else {
    console.error(`Fund ${kp.publicKey()} on mainnet, then run again.`);
    process.exit(2);
  }
  return kp;
}

async function waitFor(hash) {
  for (let i = 0; i < 60; i++) {
    const r = await server.getTransaction(hash);
    if (r.status !== rpc.Api.GetTransactionStatus.NOT_FOUND) return r;
    await new Promise((s) => setTimeout(s, 2000));
  }
  throw new Error(`transaction ${hash} not found after 120 s`);
}

function resourcesOf(sim) {
  const data = sim.transactionData.build();
  const res = data.resources;
  return {
    instructions: Number(res.instructions),
    disk_read_bytes: Number(res.diskReadBytes),
    write_bytes: Number(res.writeBytes),
    resource_fee_stroops: Number(data.resourceFee),
    min_resource_fee_stroops: Number(sim.minResourceFee),
  };
}

async function send(kp, op, label) {
  const account = await server.getAccount(kp.publicKey());
  const tx = new TransactionBuilder(account, { fee: "1000000", networkPassphrase: net.passphrase })
    .addOperation(op).setTimeout(120).build();
  const sim = await server.simulateTransaction(tx);
  if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`${label}: simulation failed: ${sim.error}`);
  const prepared = rpc.assembleTransaction(tx, sim).build();
  prepared.sign(kp);
  const sent = await server.sendTransaction(prepared);
  if (sent.status === "ERROR") throw new Error(`${label}: send failed: ${JSON.stringify(sent.errorResult)}`);
  const done = await waitFor(sent.hash);
  return { hash: sent.hash, done, sim, size: prepared.toEnvelope().toXDR().length };
}

const kp = await keypair();

if (cmd === "deploy") {
  const wasm = readFileSync(WASM);
  const wasmHash = createHash("sha256").update(wasm).digest();
  const up = await send(kp, Operation.uploadContractWasm({ wasm }), "upload");
  console.log(`upload: ${up.done.status} ${net.explorer}/tx/${up.hash}`);
  const create = await send(kp, Operation.createCustomContract({
    address: Address.fromString(kp.publicKey()), wasmHash, salt: createHash("sha256").update(`stwo-${Date.now()}`).digest(),
  }), "create");
  const contractId = scValToNative(create.done.returnValue);
  console.log(`contract: ${contractId} ${net.explorer}/contract/${contractId}`);
  log.contract = contractId;
  log.wasm_sha256 = wasmHash.toString("hex");
  log.wasm_bytes = wasm.length;
  record({ kind: "deploy", upload_tx: up.hash, create_tx: create.hash, contract: contractId, wasm_bytes: wasm.length });
} else {
  if (!log.contract) throw new Error(`no contract in ${logPath}; run deploy first`);
  const proof = readFileSync(file);
  const op = new Contract(log.contract).call("verify", nativeToScVal(proof, { type: "bytes" }));
  if (cmd === "verify") {
    const r = await send(kp, op, "verify");
    const value = r.done.status === "SUCCESS" ? scValToNative(r.done.returnValue) : null;
    const out = {
      kind: "verify", proof: file, proof_bytes: proof.length, tx: r.hash, status: r.done.status, tx_envelope_bytes: r.size,
      ledger: r.done.ledger, ...resourcesOf(r.sim),
      result: value && { ...value, proof_sha256: Buffer.from(value.proof_sha256).toString("hex") },
    };
    console.log(JSON.stringify(out, null, 2));
    console.log(`${net.explorer}/tx/${r.hash}`);
    record(out);
  } else {
    // Simulation refuses a failing call, so reuse the footprint of a valid one and submit anyway.
    const valid = log.events.filter((e) => e.kind === "verify" && e.status === "SUCCESS").pop();
    if (!valid) throw new Error("verify a valid proof first; its footprint is reused");
    const goodProof = readFileSync(valid.proof);
    const account = await server.getAccount(kp.publicKey());
    const goodTx = new TransactionBuilder(account, { fee: "1000000", networkPassphrase: net.passphrase })
      .addOperation(new Contract(log.contract).call("verify", nativeToScVal(goodProof, { type: "bytes" })))
      .setTimeout(120).build();
    const sim = await server.simulateTransaction(goodTx);
    if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`reference simulation failed: ${sim.error}`);
    const builder = sim.transactionData;
    const base = builder.build();
    const instructions = Math.min(400_000_000, Number(base.resources.instructions) * 2);
    const resourceFee = Number(base.resourceFee) * 2 + 200_000;
    builder.setResources(instructions, Number(base.resources.diskReadBytes), Number(base.resources.writeBytes));
    builder.setResourceFee(resourceFee);
    const account2 = await server.getAccount(kp.publicKey());
    const tx = new TransactionBuilder(account2, { fee: (resourceFee + 100000).toString(), networkPassphrase: net.passphrase })
      .addOperation(op).setSorobanData(builder.build()).setTimeout(120).build();
    tx.sign(kp);
    const sent = await server.sendTransaction(tx);
    if (sent.status === "ERROR") throw new Error(`send failed: ${JSON.stringify(sent.errorResult)}`);
    const done = await waitFor(sent.hash);
    const out = { kind: "reject", proof: file, proof_bytes: proof.length, tx: sent.hash, status: done.status, ledger: done.ledger };
    console.log(JSON.stringify(out, null, 2));
    console.log(`${net.explorer}/tx/${sent.hash}`);
    record(out);
  }
}
