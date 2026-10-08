/**
 * Offline mirror of the policy_wallet `pay` rules, in the same order the
 * contract applies them. Used to preview a batch of payments and to build a
 * ledger without a network. The contract is the source of truth.
 */
import type { PolicySpec } from "./seed.js";

export type PolicyRejection =
  | "InvalidAmount"
  | "PayeeNotAllowed"
  | "AmountExceedsPerTxMax"
  | "CategoryUnknown"
  | "CategoryCapExceeded";

export type PolicyDecision = { ok: true; seq: number } | { ok: false; reason: PolicyRejection };

export interface PolicyState {
  categories: Map<string, { cap: bigint; spent: bigint }>;
  payees: Set<string>;
  perTxMax: bigint;
  paymentCount: number;
}

export function initPolicyState(spec: PolicySpec): PolicyState {
  return {
    categories: new Map(spec.categories.map((c) => [c.name, { cap: c.cap, spent: 0n }])),
    payees: new Set(spec.payees),
    perTxMax: spec.perTxMax,
    paymentCount: 0,
  };
}

export function applyPayment(
  state: PolicyState,
  payment: { category: string; payee: string; amount: bigint },
): PolicyDecision {
  if (payment.amount <= 0n) return { ok: false, reason: "InvalidAmount" };
  if (!state.payees.has(payment.payee)) return { ok: false, reason: "PayeeNotAllowed" };
  if (payment.amount > state.perTxMax) return { ok: false, reason: "AmountExceedsPerTxMax" };
  const line = state.categories.get(payment.category);
  if (!line) return { ok: false, reason: "CategoryUnknown" };
  if (line.spent + payment.amount > line.cap) return { ok: false, reason: "CategoryCapExceeded" };
  line.spent += payment.amount;
  state.paymentCount += 1;
  return { ok: true, seq: state.paymentCount };
}

export function ledgerRows(state: PolicyState): { name: string; cap: bigint; spent: bigint; remaining: bigint }[] {
  return [...state.categories.entries()].map(([name, l]) => ({
    name,
    cap: l.cap,
    spent: l.spent,
    remaining: l.cap - l.spent,
  }));
}
