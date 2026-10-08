import fs from "node:fs";
import path from "node:path";
import { parseCsv } from "./csv.js";
import { parseAmount, tryParseAmount } from "./amount.js";

export interface SeedMilestone {
  idx: number;
  title: string;
  amount_usdc: string;
  due: string;
  due_unix: number;
  spec_markdown: string;
  report_file: string | null;
  rejected_report_file?: string;
}

export interface SeedGrant {
  title: string;
  funder: { name: string; address: string };
  grantee: {
    institution: string;
    pi: string;
    wallet_signer_public_key: string;
    wallet_signer_secret_key_for_local_demo_only?: string;
  };
  token: { code: string; issuer_testnet: string; decimals: number };
  reviewers: { name: string; address: string }[];
  quorum: number;
  deadline: string;
  deadline_unix: number;
  milestones: SeedMilestone[];
}

export interface SeedBudget {
  milestone: number;
  per_tx_max_usdc: string;
  categories: { name: string; label: string; cap_usdc: string }[];
}

export interface SeedVendor {
  id: string;
  name: string;
  address: string;
  country: string;
  status: "active" | "blocked";
  categories: string[];
  notes: string;
}

export interface PaymentRow {
  line: number;
  date: string;
  category: string;
  categoryRaw: string;
  payeeId: string;
  amountRaw: string;
  amount: bigint | null;
  amountError: string | null;
  invoiceRef: string;
  memo: string;
}

export function defaultSeedDir(): string {
  // dist/src -> app -> project root
  return path.resolve(import.meta.dirname, "..", "..", "..", "data", "seed");
}

export function loadGrant(seedDir: string): SeedGrant {
  return JSON.parse(fs.readFileSync(path.join(seedDir, "grant.json"), "utf8")) as SeedGrant;
}

export function loadBudget(seedDir: string): SeedBudget {
  return JSON.parse(fs.readFileSync(path.join(seedDir, "budget.json"), "utf8")) as SeedBudget;
}

export function loadPayees(seedDir: string): SeedVendor[] {
  const doc = JSON.parse(fs.readFileSync(path.join(seedDir, "payees.json"), "utf8")) as {
    vendors: SeedVendor[];
  };
  return doc.vendors;
}

export function normalizeCategory(raw: string): string {
  return raw.trim().toLowerCase().replace(/\s+/g, "_");
}

export function parsePayments(csvText: string): PaymentRow[] {
  const rows = parseCsv(csvText);
  if (rows.length === 0) return [];
  const header = rows[0].map((h) => h.trim().toLowerCase());
  const col = (name: string): number => {
    const i = header.indexOf(name);
    if (i < 0) throw new Error(`payments.csv: missing column "${name}"`);
    return i;
  };
  const cDate = col("date");
  const cCat = col("category");
  const cPayee = col("payee_id");
  const cAmount = col("amount_usdc");
  const cRef = col("invoice_ref");
  const cMemo = col("memo");
  return rows.slice(1).map((r, i) => {
    const amountRaw = r[cAmount] ?? "";
    const parsed = tryParseAmount(amountRaw);
    return {
      line: i + 2,
      date: (r[cDate] ?? "").trim(),
      category: normalizeCategory(r[cCat] ?? ""),
      categoryRaw: r[cCat] ?? "",
      payeeId: (r[cPayee] ?? "").trim(),
      amountRaw,
      amount: "value" in parsed ? parsed.value : null,
      amountError: "error" in parsed ? parsed.error : null,
      invoiceRef: (r[cRef] ?? "").trim(),
      memo: (r[cMemo] ?? "").trim(),
    };
  });
}

export function loadPayments(seedDir: string): PaymentRow[] {
  return parsePayments(fs.readFileSync(path.join(seedDir, "payments.csv"), "utf8"));
}

/** The wallet policy a funder derives from the budget and the vendor list. */
export interface PolicySpec {
  categories: { name: string; cap: bigint }[];
  payees: string[];
  perTxMax: bigint;
}

export function budgetToPolicy(budget: SeedBudget, vendors: SeedVendor[]): PolicySpec {
  return {
    categories: budget.categories.map((c) => ({
      name: normalizeCategory(c.name),
      cap: parseAmount(c.cap_usdc),
    })),
    payees: vendors.filter((v) => v.status === "active").map((v) => v.address),
    perTxMax: parseAmount(budget.per_tx_max_usdc),
  };
}

/** Resolve a vendor id ("V-004") or raw address to an address. */
export function resolvePayee(vendors: SeedVendor[], idOrAddress: string): SeedVendor | null {
  const key = idOrAddress.trim();
  return (
    vendors.find((v) => v.id.toLowerCase() === key.toLowerCase()) ??
    vendors.find((v) => v.address === key) ??
    null
  );
}

export function readReport(seedDir: string, file: string): string {
  const p = path.isAbsolute(file) ? file : path.join(seedDir, file);
  return fs.readFileSync(p, "utf8");
}
