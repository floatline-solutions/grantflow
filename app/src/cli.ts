#!/usr/bin/env node
/**
 * grantflow CLI: create | fund | submit | review | pay | ledger | export
 *
 * Every command composes the contract invocation offline and prints it (args,
 * operation XDR, unsigned transaction XDR and the equivalent stellar-cli
 * command). With --submit and a reachable SOROBAN_RPC_URL the transaction is
 * simulated, signed with the role's key from the environment and sent.
 */
import fs from "node:fs";
import path from "node:path";
import { Keypair } from "@stellar/stellar-sdk";
import { parseArgs, usage, validateCommand } from "./args.js";
import { loadConfig, type Config } from "./config.js";
import { PLACEHOLDER_CONTRACT_ID, compose, fetchWalletEvents, rpcReachable, submit, type ContractName, type Invocation } from "./chain/client.js";
import {
  budgetToPolicy,
  defaultSeedDir,
  loadBudget,
  loadGrant,
  loadPayees,
  normalizeCategory,
  parsePayments,
  readReport,
  resolvePayee,
  type PaymentRow,
  type SeedGrant,
} from "./seed.js";
import { formatAmount, parseAmount } from "./amount.js";
import { sha256Bytes, sha256Hex } from "./hash.js";
import { applyPayment, initPolicyState, ledgerRows } from "./policy.js";
import { createProvider, renderCheck } from "./ai/index.js";
import { categoryTotals, decodePaidEvents, ledgerCsv, type PaidEvent, type RpcEvent } from "./ledger/export.js";

type Flags = Record<string, string | boolean>;

function str(flags: Flags, key: string): string | undefined {
  const v = flags[key];
  return typeof v === "string" ? v : undefined;
}

function jsonReplacer(_k: string, v: unknown): unknown {
  if (typeof v === "bigint") return v.toString();
  if (v instanceof Uint8Array) return Buffer.from(v).toString("hex");
  // Buffer#toJSON runs before the replacer sees the value.
  if (v && typeof v === "object" && (v as { type?: string }).type === "Buffer" && Array.isArray((v as { data?: unknown }).data)) {
    return Buffer.from((v as { data: number[] }).data).toString("hex");
  }
  if (v && typeof v === "object" && "toXDR" in (v as object)) return "<xdr>";
  return v;
}

function printInvocation(inv: Invocation, out?: string): void {
  const doc = {
    contract: inv.contract,
    contract_id: inv.contractId,
    function: inv.fn,
    args: inv.args,
    operation_xdr: inv.operationXdr,
    transaction_xdr_unsigned: inv.transactionXdr,
    stellar_cli: inv.cli,
  };
  const text = JSON.stringify(doc, jsonReplacer, 2);
  if (out) {
    fs.writeFileSync(out, text + "\n");
    console.log(`wrote ${out}`);
  } else {
    console.log(text);
  }
}

async function maybeSubmit(config: Config, flags: Flags, inv: Invocation, secret: string | null, role: string): Promise<void> {
  if (!flags.submit) {
    console.log(`(composed offline; add --submit with SOROBAN_RPC_URL and ${role} to send)`);
    return;
  }
  if (!(await rpcReachable(config))) {
    console.log("RPC not reachable: transaction composed but not sent.");
    return;
  }
  if (!secret) throw new Error(`${role} is required to sign`);
  if (inv.contractId === PLACEHOLDER_CONTRACT_ID) throw new Error("set the contract id (env or --escrow/--wallet) before submitting");
  const res = await submit(config, inv.contract, inv.contractId, inv.fn, inv.args, secret);
  console.log(`sent: tx ${res.hash}`);
  console.log(`result: ${JSON.stringify(res.result, jsonReplacer)}`);
}

function seedDir(flags: Flags, config: Config): string {
  return str(flags, "seed-dir") ?? config.seedDir ?? defaultSeedDir();
}

function readGrantFile(p: string): SeedGrant {
  return JSON.parse(fs.readFileSync(p, "utf8")) as SeedGrant;
}

function escrowId(flags: Flags, config: Config): string {
  return str(flags, "escrow") ?? config.escrowId ?? PLACEHOLDER_CONTRACT_ID;
}
function walletId(flags: Flags, config: Config): string {
  return str(flags, "wallet") ?? config.walletId ?? PLACEHOLDER_CONTRACT_ID;
}
function publicKeyOf(secret: string | null): string | null {
  return secret ? Keypair.fromSecret(secret).publicKey() : null;
}

/** Arguments for grant_escrow.create_grant from a grant JSON file. */
export function createGrantArgs(grant: SeedGrant, walletAddress: string, tokenAddress: string): Record<string, unknown> {
  return {
    funder: grant.funder.address,
    grantee_wallet: walletAddress,
    token: tokenAddress,
    milestones: grant.milestones.map((m) => ({
      amount: parseAmount(m.amount_usdc),
      spec_hash: sha256Bytes(m.spec_markdown),
      due: BigInt(m.due_unix),
      state: { tag: "Pending", values: undefined },
      evidence_hash: undefined,
    })),
    reviewers: grant.reviewers.map((r) => r.address),
    quorum: grant.quorum,
    deadline: BigInt(grant.deadline_unix),
  };
}

async function cmdCreate(flags: Flags, config: Config): Promise<number> {
  const grant = readGrantFile(str(flags, "grant")!);
  const args = createGrantArgs(grant, walletId(flags, config), str(flags, "token") ?? config.tokenId ?? PLACEHOLDER_CONTRACT_ID);
  const inv = compose("grant_escrow", escrowId(flags, config), "create_grant", args, config.networkPassphrase, publicKeyOf(config.funderSecret) ?? undefined);
  console.log(`grant "${grant.title}": ${grant.milestones.length} milestones, quorum ${grant.quorum}/${grant.reviewers.length}, total ${formatAmount(grant.milestones.reduce((s, m) => s + parseAmount(m.amount_usdc), 0n))} USDC`);
  for (const m of grant.milestones) console.log(`  #${m.idx} ${m.title}: ${formatAmount(parseAmount(m.amount_usdc))} USDC, spec sha256 ${sha256Hex(m.spec_markdown)}`);
  printInvocation(inv, str(flags, "out"));
  await maybeSubmit(config, flags, inv, config.funderSecret, "FUNDER_SECRET");
  return 0;
}

async function cmdFund(flags: Flags, config: Config): Promise<number> {
  const from = str(flags, "from") ?? publicKeyOf(config.funderSecret);
  if (!from) throw new Error("fund: --from or FUNDER_SECRET is required");
  const args = { id: BigInt(str(flags, "grant-id")!), from, amount: parseAmount(str(flags, "amount")!) };
  const inv = compose("grant_escrow", escrowId(flags, config), "fund", args, config.networkPassphrase, from);
  printInvocation(inv, str(flags, "out"));
  await maybeSubmit(config, flags, inv, config.funderSecret, "FUNDER_SECRET");
  return 0;
}

async function cmdSubmit(flags: Flags, config: Config): Promise<number> {
  const report = fs.readFileSync(str(flags, "report")!, "utf8");
  const args = {
    escrow: escrowId(flags, config),
    grant_id: BigInt(str(flags, "grant-id")!),
    idx: Number(str(flags, "milestone")),
    evidence_hash: sha256Bytes(report),
  };
  console.log(`report ${str(flags, "report")}: sha256 ${sha256Hex(report)}`);
  const inv = compose("policy_wallet", walletId(flags, config), "submit_evidence", args, config.networkPassphrase, publicKeyOf(config.granteeSecret) ?? undefined);
  printInvocation(inv, str(flags, "out"));
  await maybeSubmit(config, flags, inv, config.granteeSecret, "GRANTEE_SECRET");
  return 0;
}

async function cmdReview(flags: Flags, config: Config): Promise<number> {
  const idx = Number(str(flags, "milestone"));
  const report = fs.readFileSync(str(flags, "report")!, "utf8");
  let spec: string;
  if (str(flags, "spec")) {
    spec = fs.readFileSync(str(flags, "spec")!, "utf8");
  } else {
    const grant = readGrantFile(str(flags, "grant") ?? path.join(seedDir(flags, config), "grant.json"));
    const m = grant.milestones.find((x) => x.idx === idx);
    if (!m) throw new Error(`milestone ${idx} not in grant file`);
    spec = m.spec_markdown;
  }
  const provider = createProvider();
  const check = await provider.check({ spec, report });
  if (flags.json) console.log(JSON.stringify(check, null, 2));
  else console.log(renderCheck(check));

  if (flags.approve === undefined && flags.reject === undefined) {
    console.log("\nNo decision requested (--approve or --reject --reason <text>). The checklist is advice; a reviewer signs the decision.");
    return 0;
  }
  const reviewer = str(flags, "reviewer") ?? publicKeyOf(config.reviewerSecret);
  if (!reviewer) throw new Error("review: --reviewer or REVIEWER_SECRET is required for a decision");
  const id = BigInt(str(flags, "grant-id")!);
  let inv: Invocation;
  if (flags.approve !== undefined) {
    inv = compose("grant_escrow", escrowId(flags, config), "approve", { id, idx, reviewer }, config.networkPassphrase, reviewer);
  } else {
    const reason = str(flags, "reason") ?? "rejected by reviewer";
    inv = compose("grant_escrow", escrowId(flags, config), "reject", { id, idx, reviewer, reason_hash: sha256Bytes(reason) }, config.networkPassphrase, reviewer);
    console.log(`reason sha256 ${sha256Hex(reason)}`);
  }
  printInvocation(inv, str(flags, "out"));
  await maybeSubmit(config, flags, inv, config.reviewerSecret, "REVIEWER_SECRET");
  return 0;
}

interface BatchResult {
  row: PaymentRow;
  payee: string | null;
  decision: string;
  invocation?: Invocation;
}

/** Preview a payments CSV against the wallet policy, exactly as the contract would. */
export function previewBatch(rows: PaymentRow[], seed: string, walletAddr: string, networkPassphrase: string): { results: BatchResult[]; ledger: ReturnType<typeof ledgerRows> } {
  const vendors = loadPayees(seed);
  const policy = budgetToPolicy(loadBudget(seed), vendors);
  const state = initPolicyState(policy);
  const results: BatchResult[] = rows.map((row) => {
    const vendor = resolvePayee(vendors, row.payeeId);
    const payee = vendor?.address ?? (row.payeeId.startsWith("G") || row.payeeId.startsWith("C") ? row.payeeId : null);
    if (row.amount === null) return { row, payee, decision: `skipped: ${row.amountError}` };
    if (!payee) return { row, payee, decision: "skipped: unknown payee id" };
    const d = applyPayment(state, { category: row.category, payee, amount: row.amount });
    if (!d.ok) return { row, payee, decision: `rejected: ${d.reason}` };
    const invocation = compose("policy_wallet", walletAddr, "pay", { category: row.category, payee, amount: row.amount, memo_hash: sha256Bytes(row.memo || row.invoiceRef) }, networkPassphrase);
    return { row, payee, decision: `accepted (seq ${d.seq})`, invocation };
  });
  return { results, ledger: ledgerRows(state) };
}

async function cmdPay(flags: Flags, config: Config): Promise<number> {
  const seed = seedDir(flags, config);
  const wallet = walletId(flags, config);
  if (str(flags, "batch")) {
    const rows = parsePayments(fs.readFileSync(str(flags, "batch")!, "utf8"));
    const { results, ledger } = previewBatch(rows, seed, wallet, config.networkPassphrase);
    for (const r of results) {
      const amt = r.row.amount === null ? r.row.amountRaw : formatAmount(r.row.amount);
      console.log(`${String(r.row.line).padStart(3)}  ${r.row.category.padEnd(10)} ${r.row.payeeId.padEnd(6)} ${amt.padStart(10)}  ${r.decision}`);
    }
    console.log("\nledger after batch:");
    for (const l of ledger) console.log(`  ${l.name.padEnd(10)} spent ${formatAmount(l.spent).padStart(9)} of ${formatAmount(l.cap).padStart(9)}  (remaining ${formatAmount(l.remaining)})`);
    const accepted = results.filter((r) => r.invocation);
    console.log(`\n${accepted.length} accepted, ${results.length - accepted.length} rejected or skipped`);
    if (str(flags, "out")) {
      fs.writeFileSync(str(flags, "out")!, JSON.stringify(accepted.map((r) => ({ line: r.row.line, function: "pay", args: r.invocation!.args, operation_xdr: r.invocation!.operationXdr, stellar_cli: r.invocation!.cli })), jsonReplacer, 2) + "\n");
      console.log(`wrote ${str(flags, "out")}`);
    }
    if (flags.submit) console.log("batch submission is intentionally manual: submit each accepted payment with `pay --category ... --submit` so the grantee signs one at a time.");
    return 0;
  }
  const vendors = loadPayees(seed);
  const vendor = resolvePayee(vendors, str(flags, "payee")!);
  const payee = vendor?.address ?? str(flags, "payee")!;
  const memo = str(flags, "memo") ?? "";
  const args = {
    category: normalizeCategory(str(flags, "category")!),
    payee,
    amount: parseAmount(str(flags, "amount")!),
    memo_hash: sha256Bytes(memo),
  };
  if (vendor?.status === "blocked") console.log(`warning: ${vendor.id} ${vendor.name} is blocked in payees.json; the wallet will reject it unless the funder allowlisted it`);
  const inv = compose("policy_wallet", wallet, "pay", args, config.networkPassphrase, publicKeyOf(config.granteeSecret) ?? undefined);
  printInvocation(inv, str(flags, "out"));
  await maybeSubmit(config, flags, inv, config.granteeSecret, "GRANTEE_SECRET");
  return 0;
}

/** Paid events from --events (RPC JSON), --simulate (payments CSV) or RPC. */
async function loadEvents(flags: Flags, config: Config): Promise<PaidEvent[]> {
  if (str(flags, "events")) {
    const raw = JSON.parse(fs.readFileSync(str(flags, "events")!, "utf8")) as RpcEvent[] | { events: RpcEvent[] };
    return decodePaidEvents(Array.isArray(raw) ? raw : raw.events);
  }
  if (str(flags, "simulate")) {
    const seed = seedDir(flags, config);
    const rows = parsePayments(fs.readFileSync(str(flags, "simulate")!, "utf8"));
    const { results } = previewBatch(rows, seed, walletId(flags, config), config.networkPassphrase);
    return results
      .filter((r) => r.invocation)
      .map((r, i) => ({
        seq: i + 1,
        ledger: null,
        closedAt: r.row.date,
        category: r.row.category,
        payee: r.payee!,
        amount: r.row.amount!,
        categorySpent: null,
        memoHash: sha256Hex(r.row.memo || r.row.invoiceRef),
        txHash: null,
      }));
  }
  if (config.walletId && (await rpcReachable(config))) {
    return decodePaidEvents((await fetchWalletEvents(config, config.walletId)) as RpcEvent[]);
  }
  throw new Error("no event source: pass --events <rpc-events.json>, --simulate <payments.csv>, or set SOROBAN_RPC_URL and POLICY_WALLET_ID");
}

async function cmdLedger(flags: Flags, config: Config): Promise<number> {
  const events = await loadEvents(flags, config);
  const totals = categoryTotals(events);
  let caps = new Map<string, bigint>();
  try {
    caps = new Map(loadBudget(seedDir(flags, config)).categories.map((c) => [normalizeCategory(c.name), parseAmount(c.cap_usdc)]));
  } catch {
    /* budget is optional */
  }
  if (flags.json) {
    console.log(JSON.stringify({ payments: events.length, categories: [...totals].map(([name, spent]) => ({ name, spent: formatAmount(spent), cap: caps.has(name) ? formatAmount(caps.get(name)!) : null })) }, null, 2));
    return 0;
  }
  console.log(`${events.length} payments`);
  let total = 0n;
  for (const [name, spent] of totals) {
    total += spent;
    const cap = caps.get(name);
    console.log(`  ${name.padEnd(10)} ${formatAmount(spent).padStart(10)}${cap !== undefined ? ` of ${formatAmount(cap)} (${formatAmount(cap - spent)} remaining)` : ""}`);
  }
  console.log(`  ${"total".padEnd(10)} ${formatAmount(total).padStart(10)}`);
  return 0;
}

async function cmdExport(flags: Flags, config: Config): Promise<number> {
  const events = await loadEvents(flags, config);
  const csv = ledgerCsv(events);
  if (str(flags, "out")) {
    fs.writeFileSync(str(flags, "out")!, csv);
    console.log(`wrote ${str(flags, "out")} (${events.length} rows)`);
  } else {
    process.stdout.write(csv);
  }
  return 0;
}

export async function main(argv: string[], env: NodeJS.ProcessEnv = process.env): Promise<number> {
  const parsed = parseArgs(argv);
  const v = validateCommand(parsed);
  if (!v.ok) {
    console.error(v.error);
    return parsed.flags.help ? 0 : 2;
  }
  const config = loadConfig(env);
  switch (v.command) {
    case "create":
      return cmdCreate(v.flags, config);
    case "fund":
      return cmdFund(v.flags, config);
    case "submit":
      return cmdSubmit(v.flags, config);
    case "review":
      return cmdReview(v.flags, config);
    case "pay":
      return cmdPay(v.flags, config);
    case "ledger":
      return cmdLedger(v.flags, config);
    case "export":
      return cmdExport(v.flags, config);
    default:
      console.error(usage());
      return 2;
  }
}

const isEntrypoint = process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname);
if (isEntrypoint) {
  main(process.argv.slice(2))
    .then((code) => process.exit(code))
    .catch((err: unknown) => {
      console.error(err instanceof Error ? err.message : String(err));
      process.exit(1);
    });
}
