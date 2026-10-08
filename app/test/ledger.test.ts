import { test } from "node:test";
import assert from "node:assert/strict";
import { Address, Keypair, nativeToScVal, xdr } from "@stellar/stellar-sdk";
import { LEDGER_CSV_HEADER, categoryTotals, decodePaidEvent, decodePaidEvents, ledgerCsv, type RpcEvent } from "../src/ledger/export.js";
import { parseCsv } from "../src/csv.js";

/** Encode a `paid` event exactly as the policy wallet emits it. */
function paidEvent(seq: number, category: string, payee: string, amount: bigint, spent: bigint, ledger: number): RpcEvent {
  const memo = Buffer.alloc(32, seq);
  const value = xdr.ScVal.scvMap(
    [
      ["amount", nativeToScVal(amount, { type: "i128" })],
      ["category_spent", nativeToScVal(spent, { type: "i128" })],
      ["memo_hash", xdr.ScVal.scvBytes(memo)],
      ["seq", nativeToScVal(BigInt(seq), { type: "u64" })],
    ].map(([k, v]) => new xdr.ScMapEntry({ key: xdr.ScVal.scvSymbol(k as string), val: v as xdr.ScVal })),
  );
  return {
    ledger,
    ledgerClosedAt: `2025-12-0${(seq % 9) + 1}T10:00:00Z`,
    id: `${ledger}-${seq}`,
    txHash: "ab".repeat(32),
    topic: [xdr.ScVal.scvSymbol("paid"), xdr.ScVal.scvSymbol(category), new Address(payee).toScVal()].map((t) => t.toXDR("base64")),
    value: value.toXDR("base64"),
  };
}

const A = Keypair.random().publicKey();
const B = Keypair.random().publicKey();

test("decodePaidEvent decodes topics and data map; ignores other events", () => {
  const ev = decodePaidEvent(paidEvent(3, "fieldwork", A, 3_407_500_000n, 4_607_500_000n, 1200));
  assert.ok(ev);
  assert.equal(ev.seq, 3);
  assert.equal(ev.category, "fieldwork");
  assert.equal(ev.payee, A);
  assert.equal(ev.amount, 3_407_500_000n);
  assert.equal(ev.categorySpent, 4_607_500_000n);
  assert.equal(ev.memoHash, "03".repeat(32));
  assert.equal(ev.ledger, 1200);
  const transfer: RpcEvent = { topic: [xdr.ScVal.scvSymbol("transfer").toXDR("base64"), new Address(A).toScVal().toXDR("base64"), new Address(B).toScVal().toXDR("base64")], value: nativeToScVal(1n, { type: "i128" }).toXDR("base64") };
  assert.equal(decodePaidEvent(transfer), null);
  assert.equal(decodePaidEvent({ topic: [], value: "" }), null);
});

test("ledgerCsv has the documented shape, sorted by seq, amounts in USDC", () => {
  const events = decodePaidEvents([
    paidEvent(2, "personnel", B, 20_000_000_000n, 32_500_000_000n, 1001),
    paidEvent(1, "personnel", A, 12_500_000_000n, 12_500_000_000n, 1000),
    paidEvent(3, "equipment", A, 18_755_000_000n, 18_755_000_000n, 1002),
  ]);
  const csv = ledgerCsv(events);
  const rows = parseCsv(csv);
  assert.deepEqual(rows[0], LEDGER_CSV_HEADER);
  assert.equal(rows.length, 4);
  assert.deepEqual(rows.slice(1).map((r) => r[0]), ["1", "2", "3"]);
  assert.equal(rows[1][3], "personnel");
  assert.equal(rows[1][4], A);
  assert.equal(rows[1][5], "1250.00");
  assert.equal(rows[3][5], "1875.50");
  assert.equal(rows[2][6], "02".repeat(32));
  assert.equal(rows[1][7], "ab".repeat(32));
  assert.ok(csv.endsWith("\n"));

  const totals = categoryTotals(events);
  assert.equal(totals.get("personnel"), 32_500_000_000n);
  assert.equal(totals.get("equipment"), 18_755_000_000n);
});
