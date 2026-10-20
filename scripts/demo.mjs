// Keeps proofs on chain (contracts/demo) so anyone can re-run the verifier from a short link.
//
//   node scripts/demo.mjs testnet deploy                          uploads and creates the demo contract
//   node scripts/demo.mjs testnet put <slot> <verifier> <proof>   stores a proof for a verifier contract
//   node scripts/demo.mjs testnet links                           prints a Stellar Lab link per slot
//
// Build it first with DEMO=1 ./scripts/build-contract.sh. Uses the same key as deploy.mjs
// (.keys/<network>.json) and logs to deployments/<network>-demo.json.
//
// A link opens Stellar Lab's simulateTransaction form with a transaction that calls run(slot).
// Pressing Submit makes an RPC node run the verifier on the stored proof. Nothing is signed or
// sent. The proof itself cannot go in the link: Lab's server refuses URLs of 17 KB (HTTP 431).
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  Account, Address, Contract, Keypair, Networks, Operation, TransactionBuilder, nativeToScVal, scValToNative, rpc,
} from "@stellar/stellar-sdk";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const WASM = join(ROOT, "target/wasm32v1-none/contract/stwo_soroban_demo.wasm");
const NETS = {
  testnet: {
    rpc: "https://soroban-testnet.stellar.org", horizon: "https://horizon-testnet.stellar.org",
    passphrase: Networks.TESTNET, label: "Testnet", explorer: "https://stellar.expert/explorer/testnet",
  },
};
const CHUNK = 60_000; // a ledger entry holds at most 64 KB

const [netName, cmd, ...args] = process.argv.slice(2);
const net = NETS[netName];
if (!net || !["deploy", "put", "links"].includes(cmd)) {
  console.error("usage: node scripts/demo.mjs testnet <deploy|put <slot> <verifier> <proof.bin>|links>");
  process.exit(1);
}
const server = new rpc.Server(net.rpc);
const logPath = join(ROOT, "deployments", `${netName}-demo.json`);
const log = existsSync(logPath) ? JSON.parse(readFileSync(logPath, "utf8")) : { network: netName, slots: {}, events: [] };
const save = () => writeFileSync(logPath, JSON.stringify(log, null, 2) + "\n");
const kp = Keypair.fromSecret(JSON.parse(readFileSync(join(ROOT, ".keys", `${netName}.json`), "utf8")).secret);

async function send(op, label) {
  const account = await server.getAccount(kp.publicKey());
  const tx = new TransactionBuilder(account, { fee: "1000000", networkPassphrase: net.passphrase })
    .addOperation(op).setTimeout(120).build();
  const sim = await server.simulateTransaction(tx);
  if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`${label}: simulation failed: ${sim.error}`);
  const prepared = rpc.assembleTransaction(tx, sim).build();
  prepared.sign(kp);
  const sent = await server.sendTransaction(prepared);
  if (sent.status === "ERROR") throw new Error(`${label}: send failed: ${JSON.stringify(sent.errorResult)}`);
  for (let i = 0; i < 60; i++) {
    const r = await server.getTransaction(sent.hash);
    if (r.status === rpc.Api.GetTransactionStatus.SUCCESS) return { hash: sent.hash, done: r };
    if (r.status === rpc.Api.GetTransactionStatus.FAILED) throw new Error(`${label}: failed ${sent.hash}`);
    await new Promise((s) => setTimeout(s, 2000));
  }
  throw new Error(`${label}: ${sent.hash} not found after 120 s`);
}

// Stellar Lab keeps its form state in the query string, encoded by zustand-querystring.
function labState(value) {
  const enc = (s, re) => encodeURI(s.replace(re, "/$1"));
  const str = (v) => typeof v === "object"
    ? "$" + Object.entries(v).map(([k, x]) => enc(k, /([=:@$/])/g) + str(x)).join("&") + ";"
    : "=" + enc(String(v), /([&;/])/g);
  return str(value).replace(/;+$/g, "").replace(/^\$/, "");
}

function labLink(tx) {
  const state = {
    network: { id: netName, label: net.label, horizonUrl: net.horizon, rpcUrl: net.rpc, passphrase: net.passphrase },
    endpoints: { params: { tx } },
  };
  return `https://lab.stellar.org/endpoints/rpc/simulate-transaction?$=${labState(state)};;`;
}

if (cmd === "deploy") {
  const wasm = readFileSync(WASM);
  const wasmHash = createHash("sha256").update(wasm).digest();
  const up = await send(Operation.uploadContractWasm({ wasm }), "upload");
  const create = await send(Operation.createCustomContract({
    address: Address.fromString(kp.publicKey()), wasmHash,
    salt: createHash("sha256").update(`stwo-demo-${Date.now()}`).digest(),
    constructorArgs: [nativeToScVal(Address.fromString(kp.publicKey()), { type: "address" })],
  }), "create");
  log.contract = scValToNative(create.done.returnValue);
  log.events.push({ at: new Date().toISOString(), kind: "deploy", upload_tx: up.hash, create_tx: create.hash, wasm_bytes: wasm.length });
  save();
  console.log(`demo contract: ${log.contract} ${net.explorer}/contract/${log.contract}`);
} else if (cmd === "put") {
  const [slot, verifier, file] = args;
  const proof = readFileSync(file);
  const chunks = Math.ceil(proof.length / CHUNK);
  for (let i = 0; i < chunks; i++) {
    const data = proof.subarray(i * CHUNK, (i + 1) * CHUNK);
    const op = new Contract(log.contract).call("put",
      nativeToScVal(Number(slot), { type: "u32" }), nativeToScVal(Address.fromString(verifier), { type: "address" }),
      nativeToScVal(chunks, { type: "u32" }), nativeToScVal(i, { type: "u32" }), nativeToScVal(data, { type: "bytes" }));
    const r = await send(op, `put ${slot}.${i}`);
    console.log(`slot ${slot} chunk ${i + 1}/${chunks}: ${net.explorer}/tx/${r.hash}`);
    log.events.push({ at: new Date().toISOString(), kind: "put", slot: Number(slot), chunk: i, tx: r.hash });
  }
  log.slots[slot] = { verifier, proof: file, proof_bytes: proof.length, proof_sha256: createHash("sha256").update(proof).digest("hex") };
  save();
} else {
  // Any existing account can be the source of a simulation, and its sequence number is not checked.
  for (const [slot, s] of Object.entries(log.slots)) {
    const tx = new TransactionBuilder(new Account(kp.publicKey(), "1"), { fee: "100", networkPassphrase: net.passphrase })
      .addOperation(new Contract(log.contract).call("run", nativeToScVal(Number(slot), { type: "u32" })))
      .setTimeout(0).build().toXDR();
    s.lab = labLink(tx);
    console.log(`slot ${slot} (${s.proof}): ${s.lab.length} chars\n${s.lab}\n`);
  }
  save();
}
