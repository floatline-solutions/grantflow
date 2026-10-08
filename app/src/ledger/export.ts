/**
 * The funder's live ledger: every accepted payment is a `paid` event of the
 * policy wallet with topics [ "paid", category, payee ] and a data map
 * { amount, category_spent, memo_hash, seq }. This module decodes events as
 * returned by Soroban RPC `getEvents` and turns them into CSV.
 */
import { scValToNative, xdr } from "@stellar/stellar-sdk";
import { csvEscape } from "../csv.js";
import { formatAmount } from "../amount.js";

export interface PaidEvent {
  seq: number;
  ledger: number | null;
  closedAt: string | null;
  category: string;
  payee: string;
  amount: bigint;
  categorySpent: bigint | null;
  memoHash: string;
  txHash: string | null;
}

/** Shape of one entry in a Soroban RPC getEvents response. */
export interface RpcEvent {
  ledger?: number;
  ledgerClosedAt?: string;
  contractId?: string;
  id?: string;
  txHash?: string;
  topic: string[];
  value: string;
}

export const LEDGER_CSV_HEADER = ["seq", "ledger", "closed_at", "category", "payee", "amount_usdc", "memo_hash", "tx_hash"];

function toBigInt(v: unknown): bigint {
  if (typeof v === "bigint") return v;
  if (typeof v === "number") return BigInt(v);
  if (typeof v === "string") return BigInt(v);
  throw new Error(`cannot convert ${typeof v} to bigint`);
}

function toHex(v: unknown): string {
  if (Buffer.isBuffer(v)) return v.toString("hex");
  if (v instanceof Uint8Array) return Buffer.from(v).toString("hex");
  if (typeof v === "string") return v;
  throw new Error("memo_hash is not bytes");
}

/** Decode one RPC event; returns null for events that are not `paid`. */
export function decodePaidEvent(ev: RpcEvent): PaidEvent | null {
  if (!ev.topic || ev.topic.length < 3) return null;
  const topics = ev.topic.map((t) => scValToNative(xdr.ScVal.fromXDR(t, "base64")));
  if (topics[0] !== "paid") return null;
  const data = scValToNative(xdr.ScVal.fromXDR(ev.value, "base64")) as Record<string, unknown>;
  return {
    seq: Number(toBigInt(data.seq)),
    ledger: ev.ledger ?? null,
    closedAt: ev.ledgerClosedAt ?? null,
    category: String(topics[1]),
    payee: String(topics[2]),
    amount: toBigInt(data.amount),
    categorySpent: data.category_spent === undefined ? null : toBigInt(data.category_spent),
    memoHash: toHex(data.memo_hash),
    txHash: ev.txHash ?? null,
  };
}

export function decodePaidEvents(events: RpcEvent[]): PaidEvent[] {
  return events
    .map(decodePaidEvent)
    .filter((e): e is PaidEvent => e !== null)
    .sort((a, b) => a.seq - b.seq);
}

export function ledgerCsv(events: PaidEvent[]): string {
  const rows = [LEDGER_CSV_HEADER.join(",")];
  for (const e of [...events].sort((a, b) => a.seq - b.seq)) {
    rows.push(
      [
        String(e.seq),
        e.ledger === null ? "" : String(e.ledger),
        e.closedAt ?? "",
        e.category,
        e.payee,
        formatAmount(e.amount),
        e.memoHash,
        e.txHash ?? "",
      ]
        .map(csvEscape)
        .join(","),
    );
  }
  return rows.join("\n") + "\n";
}

export function categoryTotals(events: PaidEvent[]): Map<string, bigint> {
  const totals = new Map<string, bigint>();
  for (const e of events) totals.set(e.category, (totals.get(e.category) ?? 0n) + e.amount);
  return totals;
}
