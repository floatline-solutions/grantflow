import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { Address, Keypair, Networks, StrKey, TransactionBuilder, buildAuthorizationEntryPreimage, hash, scValToNative, xdr } from "@stellar/stellar-sdk";
import { PLACEHOLDER_CONTRACT_ID, authorizeWalletEntry, compose, specFor } from "../src/chain/client.js";
import { createGrantArgs, previewBatch } from "../src/cli.js";
import { loadGrant, loadPayments } from "../src/seed.js";
import { sha256Bytes } from "../src/hash.js";

const SEED = path.resolve(import.meta.dirname, "..", "..", "..", "data", "seed");
const WALLET = StrKey.encodeContract(Buffer.alloc(32, 7));
const ESCROW = StrKey.encodeContract(Buffer.alloc(32, 9));

function mapKeys(v: xdr.ScVal): string[] {
  assert.equal(v.type, "scvMap");
  return (v as xdr.ScValMap).map!.map((e) => (e.key as xdr.ScValSymbol).sym.toString());
}
function mapGet(v: xdr.ScVal, key: string): xdr.ScVal {
  const entry = (v as xdr.ScValMap).map!.find((e) => (e.key as xdr.ScValSymbol).sym.toString() === key);
  assert.ok(entry, key);
  return entry.val;
}

test("embedded specs expose every contract function", () => {
  const escrow = specFor("grant_escrow").funcs().map((f) => f.name.toString());
  for (const fn of ["create_grant", "fund", "submit_evidence", "approve", "release", "reject", "clawback", "grant", "approvals"]) assert.ok(escrow.includes(fn), fn);
  const wallet = specFor("policy_wallet").funcs().map((f) => f.name.toString());
  for (const fn of ["pay", "submit_evidence", "set_policy", "add_payee", "remove_payee", "rotate_key", "policy", "ledger", "__check_auth"]) assert.ok(wallet.includes(fn), fn);
});

test("compose builds a pay invocation offline with correctly typed ScVals", () => {
  const payee = Keypair.random().publicKey();
  const inv = compose("policy_wallet", WALLET, "pay", { category: "fieldwork", payee, amount: 3_407_500_000n, memo_hash: sha256Bytes("TN-301") }, Networks.TESTNET);
  assert.deepEqual(
    inv.scArgs.map((a) => a.type),
    ["scvSymbol", "scvAddress", "scvI128", "scvBytes"],
  );
  const op = xdr.Operation.fromXDR(inv.operationXdr, "base64");
  assert.equal(op.body.type, "invokeHostFunction");
  const host = (op.body as xdr.OperationBodyInvokeHostFunction).invokeHostFunctionOp.hostFunction;
  assert.equal(host.type, "hostFunctionTypeInvokeContract");
  const call = (host as xdr.HostFunctionInvokeContract).invokeContract;
  assert.equal(call.functionName.toString(), "pay");
  assert.equal(call.args.length, 4);
  const tx = TransactionBuilder.fromXDR(inv.transactionXdr, Networks.TESTNET);
  assert.equal(tx.operations.length, 1);
  assert.match(inv.cli, /stellar contract invoke --id C.* -- pay --category fieldwork/);
});

test("compose builds create_grant from the seed grant, including the milestone struct and enum", () => {
  const grant = loadGrant(SEED);
  const args = createGrantArgs(grant, WALLET, PLACEHOLDER_CONTRACT_ID);
  const inv = compose("grant_escrow", ESCROW, "create_grant", args, Networks.TESTNET);
  assert.equal(inv.scArgs.length, 7);
  assert.equal(inv.scArgs[3].type, "scvVec");
  const milestones = (inv.scArgs[3] as xdr.ScValVec).vec!;
  assert.equal(milestones.length, 3);
  assert.deepEqual(mapKeys(milestones[0]), ["amount", "due", "evidence_hash", "spec_hash", "state"]);
  const state = mapGet(milestones[0], "state");
  assert.equal(state.type, "scvVec");
  assert.equal(((state as xdr.ScValVec).vec![0] as xdr.ScValSymbol).sym.toString(), "Pending");
  assert.equal(mapGet(milestones[0], "evidence_hash").type, "scvVoid");
  assert.equal(mapGet(milestones[0], "amount").type, "scvI128");
  assert.equal(mapGet(milestones[0], "spec_hash").type, "scvBytes");
  assert.equal(inv.scArgs[4].type, "scvVec");
  assert.equal((inv.scArgs[5] as xdr.ScValU32).u32, 2);
  assert.equal(inv.scArgs[6].type, "scvU64");
});

test("compose rejects arguments that do not match the spec", () => {
  assert.throws(() => compose("policy_wallet", WALLET, "pay", { category: "x" }, Networks.TESTNET));
  assert.throws(() => compose("policy_wallet", WALLET, "no_such_fn", {}, Networks.TESTNET));
});

test("authorizeWalletEntry signs the wallet's auth entry with the grantee key as a 64-byte signature", async () => {
  const grantee = Keypair.random();
  const inv = compose("policy_wallet", WALLET, "pay", { category: "fieldwork", payee: Keypair.random().publicKey(), amount: 3_407_500_000n, memo_hash: sha256Bytes("TN-301") }, Networks.TESTNET);
  const walletAddr = new Address(WALLET).toScAddress();
  const rootInvocation = new xdr.SorobanAuthorizedInvocation({
    function: xdr.SorobanAuthorizedFunction.sorobanAuthorizedFunctionTypeContractFn(
      new xdr.InvokeContractArgs({ contractAddress: walletAddr, functionName: "pay", args: inv.scArgs }),
    ),
    subInvocations: [],
  });
  const unsigned = new xdr.SorobanAuthorizationEntry({
    credentials: xdr.SorobanCredentials.sorobanCredentialsAddress(
      new xdr.SorobanAddressCredentials({ address: walletAddr, nonce: 7001n, signatureExpirationLedger: 0, signature: xdr.ScVal.scvVoid() }),
    ),
    rootInvocation,
  });

  const signed = await authorizeWalletEntry(unsigned, grantee, 500, Networks.TESTNET, WALLET);
  assert.equal(signed.credentials.type, "sorobanCredentialsAddress");
  const creds = (signed.credentials as xdr.SorobanCredentialsAddress).address;
  assert.equal(creds.signatureExpirationLedger, 500);
  assert.equal(creds.nonce, 7001n);
  assert.equal(creds.signature.type, "scvBytes", "the wallet's Signature type is BytesN<64>, not an account signature vector");
  const sig = Buffer.from(scValToNative(creds.signature) as Uint8Array);
  assert.equal(sig.length, 64);
  // The payload is sha256(HashIdPreimage::SorobanAuthorization{network_id, nonce, expiration, invocation}),
  // the same preimage the contract test signs with ed25519-dalek.
  const payload = Buffer.from(hash(buildAuthorizationEntryPreimage(signed, 500, Networks.TESTNET).toXdr()));
  assert.ok(grantee.verify(payload, sig));
  assert.ok(!Keypair.random().verify(payload, sig));
});

test("previewBatch composes one pay invocation per accepted seed payment", () => {
  const rows = loadPayments(SEED);
  const { results, ledger } = previewBatch(rows, SEED, WALLET, Networks.TESTNET);
  assert.equal(results.length, 20);
  assert.equal(results.filter((r) => r.invocation).length, 18);
  assert.equal(results[7].decision, "rejected: CategoryCapExceeded");
  assert.equal(results[13].decision, "rejected: PayeeNotAllowed");
  assert.equal(results[0].invocation!.fn, "pay");
  assert.equal(ledger.find((l) => l.name === "fieldwork")!.remaining, 0n);
});
