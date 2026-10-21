// Seed-recovery prototype on testnet (contracts/recovery), with fake seeds only.
//
//   node scripts/recovery.mjs testnet deploy [delay] [cutoff]   uploads and creates the contract
//                                                               (delay in ledgers, default 60; cutoff
//                                                               ledger, default the current one)
//   node scripts/recovery.mjs testnet race                      a recovery in which a thief learns the seed
//
// The race: the owner commits and, once the commitment is `delay` ledgers old, broadcasts a reveal.
// A thief who reads the seed from it commits with their own key, but their commitment is too young
// to reveal; the owner's reveal lands first and is final; the thief's reveal, once old enough, is
// refused. Takes about two delays (60 ledgers is about 5 minutes).
//
// Build it first with ./scripts/build-recovery.sh. Fees are paid by the key in .keys/<network>.json.
// The recovered "account" is a fresh random seed: its key is never funded, and the seed is public by
// design once revealed. Logs to deployments/<network>-recovery.json.
import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { createHash, randomBytes } from "node:crypto";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  Address, Contract, Keypair, Networks, Operation, TransactionBuilder, nativeToScVal, scValToNative, rpc,
} from "@stellar/stellar-sdk";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const WASM = join(ROOT, "target/wasm32v1-none/contract/recovery_default.wasm");
const NETS = {
  testnet: { rpc: "https://soroban-testnet.stellar.org", passphrase: Networks.TESTNET, explorer: "https://stellar.expert/explorer/testnet" },
};
const TAG = Buffer.from("stellar-seed-recovery-v0");
const ML_DSA_44_PUBLIC_KEY_BYTES = 1312;

const [netName, cmd, ...args] = process.argv.slice(2);
const net = NETS[netName];
if (!net || !["deploy", "race"].includes(cmd)) {
  console.error("usage: node scripts/recovery.mjs testnet <deploy [delay] [cutoff]|race>");
  process.exit(1);
}
const server = new rpc.Server(net.rpc);
const logPath = join(ROOT, "deployments", `${netName}-recovery.json`);
const log = existsSync(logPath) ? JSON.parse(readFileSync(logPath, "utf8")) : { network: netName, events: [] };
const save = () => writeFileSync(logPath, JSON.stringify(log, null, 2) + "\n");
const kp = Keypair.fromSecret(JSON.parse(readFileSync(join(ROOT, ".keys", `${netName}.json`), "utf8")).secret);
const sha256 = (...parts) => createHash("sha256").update(Buffer.concat(parts)).digest();
const hex = (b) => Buffer.from(b).toString("hex");
const plain = (v) => JSON.parse(JSON.stringify(v, function (key, x) {
  const raw = this[key]; // before toJSON, so Buffers and Uint8Arrays are still bytes
  return typeof raw === "bigint" ? Number(raw) : ArrayBuffer.isView(raw) ? hex(raw) : x;
}));

async function build(op) {
  const account = await server.getAccount(kp.publicKey());
  return new TransactionBuilder(account, { fee: "1000000", networkPassphrase: net.passphrase })
    .addOperation(op).setTimeout(120).build();
}

async function send(op, label) {
  let tx, sim;
  // The RPC endpoint is served by several nodes, and the one that simulates can be a ledger or two
  // behind the one that reported the commitment, so a reveal is retried while it is TooEarly (#2).
  for (let i = 0; ; i++) {
    tx = await build(op);
    sim = await server.simulateTransaction(tx);
    if (rpc.Api.isSimulationSuccess(sim) || !/Error\(Contract, #2\)/.test(sim.error) || i === 20) break;
    console.log(`  ${label}: simulated at ledger ${sim.latestLedger}, too early; retrying`);
    await new Promise((s) => setTimeout(s, 2000));
  }
  if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`${label}: simulation failed: ${sim.error}`);
  const prepared = rpc.assembleTransaction(tx, sim).build();
  prepared.sign(kp);
  const sent = await server.sendTransaction(prepared);
  if (sent.status === "ERROR") throw new Error(`${label}: send failed: ${JSON.stringify(sent.errorResult)}`);
  for (let i = 0; i < 60; i++) {
    const r = await server.getTransaction(sent.hash);
    if (r.status === rpc.Api.GetTransactionStatus.SUCCESS) {
      const data = sim.transactionData.build();
      return {
        step: label, tx: sent.hash, ledger: r.ledger,
        instructions: Number(data.resources.instructions), resource_fee_stroops: Number(data.resourceFee),
        envelope_bytes: prepared.toEnvelope().toXDR().length,
        result: r.returnValue ? plain(scValToNative(r.returnValue)) : null,
      };
    }
    if (r.status === rpc.Api.GetTransactionStatus.FAILED) throw new Error(`${label}: failed ${sent.hash}`);
    await new Promise((s) => setTimeout(s, 2000));
  }
  throw new Error(`${label}: ${sent.hash} not found after 120 s`);
}

/** Simulates a call that should fail and returns the contract error it reports. */
async function refused(op) {
  const sim = await server.simulateTransaction(await build(op));
  if (rpc.Api.isSimulationSuccess(sim)) throw new Error("expected the call to be refused");
  return sim.error.match(/Error\(Contract, #\d+\)/)?.[0] ?? sim.error.slice(0, 200);
}

async function read(fn, ...a) {
  const sim = await server.simulateTransaction(await build(new Contract(log.contract).call(fn, ...a)));
  if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`${fn}: ${sim.error}`);
  return plain(scValToNative(sim.result.retval));
}

const bytes = (b) => nativeToScVal(b, { type: "bytes" });

/** Waits until the network's latest closed ledger is at least `ledger`. */
async function reach(ledger) {
  let shown = false;
  for (;;) {
    const latest = (await server.getLatestLedger()).sequence;
    if (latest >= ledger) break;
    if (!shown) { process.stdout.write(`  waiting for ledger ${ledger}`); shown = true; }
    process.stdout.write(".");
    await new Promise((s) => setTimeout(s, 5000));
  }
  if (shown) process.stdout.write("\n");
}

if (cmd === "deploy") {
  const delay = Number(args[0] ?? 60);
  const cutoff = Number(args[1] ?? (await server.getLatestLedger()).sequence);
  const wasm = readFileSync(WASM);
  const wasmHash = createHash("sha256").update(wasm).digest();
  const up = await send(Operation.uploadContractWasm({ wasm }), "upload");
  const create = await send(Operation.createCustomContract({
    address: Address.fromString(kp.publicKey()), wasmHash,
    salt: createHash("sha256").update(`seed-recovery-${Date.now()}`).digest(),
    constructorArgs: [nativeToScVal(cutoff, { type: "u32" }), nativeToScVal(delay, { type: "u32" })],
  }), "create");
  log.contract = create.result;
  log.cutoff_ledger = cutoff;
  log.delay_ledgers = delay;
  log.wasm_bytes = wasm.length;
  log.wasm_sha256 = hex(wasmHash);
  log.events.push({ at: new Date().toISOString(), kind: "deploy", upload_tx: up.tx, create_tx: create.tx });
  save();
  console.log(`recovery contract: ${log.contract} ${net.explorer}/contract/${log.contract}`);
} else {
  if (!log.contract) throw new Error(`no contract in ${logPath}; run deploy first`);
  const networkId = sha256(Buffer.from(net.passphrase));
  const k = randomBytes(32);
  const a = Keypair.fromRawEd25519Seed(k).rawPublicKey();
  const ownerKey = randomBytes(ML_DSA_44_PUBLIC_KEY_BYTES);
  const thiefKey = randomBytes(ML_DSA_44_PUBLIC_KEY_BYTES);
  const commitment = (key) => sha256(TAG, networkId, a, k, sha256(key));
  const c = new Contract(log.contract);
  const [, delay] = await read("params");
  const steps = [];
  const step = (s) => { steps.push(s); console.log(`${s.step}: ledger ${s.ledger}, ${s.instructions} instructions, ${s.envelope_bytes} B, ${net.explorer}/tx/${s.tx}`); };
  const ownerReveal = () => c.call("reveal", bytes(a), bytes(k), bytes(ownerKey));
  const thiefReveal = () => c.call("reveal", bytes(a), bytes(k), bytes(thiefKey));

  step(await send(c.call("commit", bytes(commitment(ownerKey))), "owner commits"));
  await reach(steps[0].ledger + delay);
  // The owner's reveal is now broadcast; from here on the thief knows k, as if they had read it in transit.
  step(await send(c.call("commit", bytes(commitment(thiefKey))), "thief commits to the seed with their key"));
  const thiefTooEarly = await refused(thiefReveal());
  console.log(`thief reveals at once: refused with ${thiefTooEarly} (TooEarly is #2)`);
  step(await send(ownerReveal(), "owner's reveal lands"));
  await reach(steps[1].ledger + delay);
  const thiefTooLate = await refused(thiefReveal());
  console.log(`thief reveals once their commitment is old enough: refused with ${thiefTooLate} (AlreadyRecovered is #3)`);
  const recovered = await read("recovered", bytes(a));
  const ownerKeyHash = hex(sha256(ownerKey));
  console.log(`recovered(A): ${JSON.stringify(recovered)}; owner's key hash ${ownerKeyHash}`);
  if (recovered.new_key_sha256 !== ownerKeyHash) throw new Error("the owner's key was not recorded");
  log.events.push({
    at: new Date().toISOString(), kind: "race", account: Keypair.fromRawEd25519Seed(k).publicKey(),
    seed_hex: hex(k), owner_key_sha256: ownerKeyHash, thief_key_sha256: hex(sha256(thiefKey)),
    steps, thief_reveals_at_once: thiefTooEarly, thief_reveals_after_the_delay: thiefTooLate, recovered,
  });
  save();
}
