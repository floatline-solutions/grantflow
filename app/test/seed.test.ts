import { test } from "node:test";
import assert from "node:assert/strict";
import path from "node:path";
import { budgetToPolicy, loadBudget, loadGrant, loadPayees, loadPayments, resolvePayee } from "../src/seed.js";
import { applyPayment, initPolicyState, ledgerRows } from "../src/policy.js";
import { formatAmount, parseAmount } from "../src/amount.js";
import { parseCsv } from "../src/csv.js";

const SEED = path.resolve(import.meta.dirname, "..", "..", "..", "data", "seed");

test("csv parser handles quotes, embedded commas and CRLF", () => {
  const rows = parseCsv('a,b,c\r\n1,"x, y","he said ""hi"""\r\n\r\n');
  assert.deepEqual(rows, [["a", "b", "c"], ["1", "x, y", 'he said "hi"']]);
});

test("seed grant, budget and payees are consistent", () => {
  const grant = loadGrant(SEED);
  assert.equal(grant.milestones.length, 3);
  assert.equal(grant.quorum, 2);
  assert.equal(grant.reviewers.length, 3);
  const total = grant.milestones.reduce((s, m) => s + parseAmount(m.amount_usdc), 0n);
  assert.equal(formatAmount(total), "37500.00");
  assert.equal(parseAmount(grant.milestones[0].amount_usdc), 125_000_000_000n);
  for (const m of grant.milestones) assert.match(m.spec_markdown, /^1\. /m);

  const budget = loadBudget(SEED);
  assert.equal(budget.categories.length, 5);
  const caps = budget.categories.reduce((s, c) => s + parseAmount(c.cap_usdc), 0n);
  assert.equal(caps, parseAmount(grant.milestones[0].amount_usdc), "caps sum to the milestone-1 tranche");

  const vendors = loadPayees(SEED);
  assert.equal(vendors.length, 8);
  assert.equal(vendors.filter((v) => v.status === "blocked").length, 1);
  assert.ok(vendors.every((v) => /^[GC][A-Z2-7]{55}$/.test(v.address)));
  assert.equal(resolvePayee(vendors, "v-004")?.id, "V-004");
  assert.equal(resolvePayee(vendors, vendors[2].address)?.id, "V-003");
  assert.equal(resolvePayee(vendors, "V-999"), null);
});

test("payments.csv parses 20 messy rows and the policy simulation matches the contract scenario", () => {
  const rows = loadPayments(SEED);
  assert.equal(rows.length, 20);
  assert.ok(rows.every((r) => r.amount !== null), rows.filter((r) => r.amount === null).map((r) => r.amountRaw).join(","));
  assert.equal(rows[1].category, "personnel", "category is trimmed and lower-cased");
  assert.equal(rows[9].category, "fieldwork");
  assert.equal(rows[2].amount, 18_755_000_000n);

  const vendors = loadPayees(SEED);
  const state = initPolicyState(budgetToPolicy(loadBudget(SEED), vendors));
  assert.equal(state.payees.size, 7, "the blocked vendor is not allowlisted");
  const decisions = rows.map((r) => applyPayment(state, { category: r.category, payee: resolvePayee(vendors, r.payeeId)!.address, amount: r.amount! }));
  const rejected = decisions.map((d, i) => ({ d, i })).filter((x) => !x.d.ok);
  assert.deepEqual(
    rejected.map((x) => [x.i + 1, (x.d as { reason: string }).reason]),
    [
      [8, "CategoryCapExceeded"],
      [14, "PayeeNotAllowed"],
    ],
  );
  assert.equal(state.paymentCount, 18);
  const ledger = Object.fromEntries(ledgerRows(state).map((l) => [l.name, formatAmount(l.spent)]));
  assert.deepEqual(ledger, {
    personnel: "5999.99",
    equipment: "2500.00",
    fieldwork: "2000.00",
    training: "1175.50",
    overhead: "800.00",
  });
  const spent = ledgerRows(state).reduce((s, l) => s + l.spent, 0n);
  assert.equal(formatAmount(parseAmount("12,500.00") - spent), "24.51");
});

test("policy simulation applies the checks in the contract's order", () => {
  const state = initPolicyState({ categories: [{ name: "a", cap: 100n }], payees: ["P"], perTxMax: 50n });
  assert.deepEqual(applyPayment(state, { category: "a", payee: "P", amount: 0n }), { ok: false, reason: "InvalidAmount" });
  assert.deepEqual(applyPayment(state, { category: "a", payee: "X", amount: 1n }), { ok: false, reason: "PayeeNotAllowed" });
  assert.deepEqual(applyPayment(state, { category: "a", payee: "P", amount: 51n }), { ok: false, reason: "AmountExceedsPerTxMax" });
  assert.deepEqual(applyPayment(state, { category: "b", payee: "P", amount: 1n }), { ok: false, reason: "CategoryUnknown" });
  assert.deepEqual(applyPayment(state, { category: "a", payee: "P", amount: 50n }), { ok: true, seq: 1 });
  assert.deepEqual(applyPayment(state, { category: "a", payee: "P", amount: 50n }), { ok: true, seq: 2 });
  assert.deepEqual(applyPayment(state, { category: "a", payee: "P", amount: 1n }), { ok: false, reason: "CategoryCapExceeded" });
});
