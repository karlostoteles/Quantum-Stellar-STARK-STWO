// Measures on testnet what the key check [s]B = A costs two ways, by simulation (nothing is sent
// for the measurement itself): through the host's ed25519_verify with nonce point R = B, and with
// Curve25519 arithmetic compiled into the contract.
//
//   node scripts/recovery-measure.mjs testnet
//
// Deploys the wasm-ec build once (target/wasm32v1-none/contract/recovery_wasm_ec.wasm, from
// ./scripts/build-recovery.sh), then simulates check_key and check_key_wasm on a fresh fake seed.
// Fees for the deployment come from .keys/<network>.json. Logs to deployments/<network>-recovery-measure.json.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { createHash, randomBytes } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Address, Contract, Keypair, Networks, Operation, TransactionBuilder, nativeToScVal, scValToNative, rpc } from "@stellar/stellar-sdk";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const WASM = join(ROOT, "target/wasm32v1-none/contract/recovery_wasm_ec.wasm");
const NETS = { testnet: { rpc: "https://soroban-testnet.stellar.org", passphrase: Networks.TESTNET } };
const netName = process.argv[2];
const net = NETS[netName];
if (!net) { console.error("usage: node scripts/recovery-measure.mjs testnet"); process.exit(1); }
const server = new rpc.Server(net.rpc);
const logPath = join(ROOT, "deployments", `${netName}-recovery-measure.json`);
const log = existsSync(logPath) ? JSON.parse(readFileSync(logPath, "utf8")) : { network: netName, runs: [] };
const kp = Keypair.fromSecret(JSON.parse(readFileSync(join(ROOT, ".keys", `${netName}.json`), "utf8")).secret);

async function build(op) {
  const account = await server.getAccount(kp.publicKey());
  return new TransactionBuilder(account, { fee: "1000000", networkPassphrase: net.passphrase }).addOperation(op).setTimeout(120).build();
}

async function send(op, label) {
  const tx = await build(op);
  const sim = await server.simulateTransaction(tx);
  if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`${label}: simulation failed: ${sim.error}`);
  const prepared = rpc.assembleTransaction(tx, sim).build();
  prepared.sign(kp);
  const sent = await server.sendTransaction(prepared);
  if (sent.status === "ERROR") throw new Error(`${label}: send failed: ${JSON.stringify(sent.errorResult)}`);
  for (let i = 0; i < 60; i++) {
    const r = await server.getTransaction(sent.hash);
    if (r.status === rpc.Api.GetTransactionStatus.SUCCESS) return { tx: sent.hash, result: r.returnValue && scValToNative(r.returnValue) };
    if (r.status === rpc.Api.GetTransactionStatus.FAILED) throw new Error(`${label}: failed ${sent.hash}`);
    await new Promise((s) => setTimeout(s, 2000));
  }
  throw new Error(`${label}: ${sent.hash} not found`);
}

async function instructions(op) {
  const sim = await server.simulateTransaction(await build(op));
  if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`simulation failed: ${sim.error}`);
  return Number(sim.transactionData.build().resources.instructions);
}

if (!log.contract) {
  const wasm = readFileSync(WASM);
  const wasmHash = createHash("sha256").update(wasm).digest();
  const up = await send(Operation.uploadContractWasm({ wasm }), "upload");
  const create = await send(Operation.createCustomContract({
    address: Address.fromString(kp.publicKey()), wasmHash,
    salt: createHash("sha256").update(`seed-recovery-measure-${Date.now()}`).digest(),
    constructorArgs: [nativeToScVal(0, { type: "u32" }), nativeToScVal(60, { type: "u32" })],
  }), "create");
  Object.assign(log, { contract: create.result, wasm_bytes: wasm.length, wasm_sha256: wasmHash.toString("hex"), upload_tx: up.tx, create_tx: create.tx });
}
const k = randomBytes(32);
const a = Keypair.fromRawEd25519Seed(k).rawPublicKey();
const bytes = (b) => nativeToScVal(b, { type: "bytes" });
const c = new Contract(log.contract);
const host = await instructions(c.call("check_key", bytes(a), bytes(k), bytes(randomBytes(32))));
const wasm = await instructions(c.call("check_key_wasm", bytes(a), bytes(k)));
const run = { at: new Date().toISOString(), seed_hex: k.toString("hex"), host_ed25519_verify: host, wasm_curve25519: wasm, ratio: +(wasm / host).toFixed(1) };
log.runs.push(run);
writeFileSync(logPath, JSON.stringify(log, null, 2) + "\n");
console.log(JSON.stringify(run));
