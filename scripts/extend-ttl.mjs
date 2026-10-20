// Keeps contracts live: extends the TTL of each contract's instance and code to the network maximum.
//
//   node scripts/extend-ttl.mjs <testnet|mainnet> [--days=N] <contract id>...
//
// Without --days it extends to the network maximum (180 days). Rent grows with the entry size and
// the TTL: in October 2026, 180 days for the 126 KB verifier cost about 2,100 XLM on testnet, more
// than one transaction's fee can carry, so the testnet verifiers were extended by 35 days.
//
// A new contract lives for the network's minimum TTL (7 days on testnet in October 2026) and is
// archived afterwards, so calls fail until it is restored. Uses the key in .keys/<network>.json.
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { Contract, Keypair, Networks, Operation, SorobanDataBuilder, TransactionBuilder, rpc, xdr } from "@stellar/stellar-sdk";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const NETS = {
  testnet: { rpc: "https://soroban-testnet.stellar.org", passphrase: Networks.TESTNET },
  mainnet: { rpc: "https://mainnet.sorobanrpc.com", passphrase: Networks.PUBLIC },
};
const [netName, ...rest] = process.argv.slice(2);
const days = Number(rest.find((a) => a.startsWith("--days="))?.slice(7) || 0);
const ids = rest.filter((a) => !a.startsWith("--"));
const net = NETS[netName];
if (!net || !ids.length) {
  console.error("usage: node scripts/extend-ttl.mjs <testnet|mainnet> <contract id>...");
  process.exit(1);
}
const server = new rpc.Server(net.rpc);
const kp = Keypair.fromSecret(JSON.parse(readFileSync(join(ROOT, ".keys", `${netName}.json`), "utf8")).secret);

// The SDK's ledger entries serialize to JSON with snake_case fields and hex bytes.
const json = (v) => JSON.parse(JSON.stringify(v, (k, x) => (typeof x === "bigint" ? x.toString() : x)));

// The network's maximum entry TTL, read from its state-archival settings.
const archival = await server.getLedgerEntries(xdr.LedgerKey.configSetting(new xdr.LedgerKeyConfigSetting({
  configSettingId: xdr.ConfigSettingId.fromName("configSettingStateArchival"),
})));
const maxTtl = Number(json(archival.entries[0].val).config_setting.state_archival.max_entry_ttl);
// About 17,280 ledgers a day (5 s per ledger).
const extendTo = days ? Math.min(maxTtl - 1, Math.round(days * 17_280)) : maxTtl - 1;

for (const id of ids) {
  const instanceKey = new Contract(id).getFootprint();
  const instance = await server.getContractData(id, xdr.ScVal.scvLedgerKeyContractInstance(), rpc.Durability.Persistent);
  const wasmHash = Buffer.from(json(instance.val).contract_data.val.contract_instance.executable.wasm, "hex");
  const codeKey = xdr.LedgerKey.contractCode(new xdr.LedgerKeyContractCode({ hash: wasmHash }));
  const account = await server.getAccount(kp.publicKey());
  const tx = new TransactionBuilder(account, { fee: "1000000", networkPassphrase: net.passphrase })
    .addOperation(Operation.extendFootprintTtl({ extendTo }))
    .setSorobanData(new SorobanDataBuilder().setReadOnly([instanceKey, codeKey]).build())
    .setTimeout(120).build();
  const sim = await server.simulateTransaction(tx);
  if (!rpc.Api.isSimulationSuccess(sim)) throw new Error(`${id}: simulation failed: ${sim.error}`);
  const prepared = rpc.assembleTransaction(tx, sim).build();
  prepared.sign(kp);
  const sent = await server.sendTransaction(prepared);
  if (sent.status === "ERROR") throw new Error(`${id}: send failed: ${JSON.stringify(sent.errorResult)}`);
  let done;
  for (let i = 0; i < 60 && !done; i++) {
    const r = await server.getTransaction(sent.hash);
    if (r.status !== rpc.Api.GetTransactionStatus.NOT_FOUND) done = r;
    else await new Promise((s) => setTimeout(s, 2000));
  }
  const after = await server.getContractData(id, xdr.ScVal.scvLedgerKeyContractInstance(), rpc.Durability.Persistent);
  console.log(`${id}: ${done?.status} tx ${sent.hash}; instance live until ledger ${after.liveUntilLedgerSeq}`);
}
