/** CLI argument parsing, pure and testable. */

export interface ParsedArgs {
  command: string | null;
  flags: Record<string, string | boolean>;
  positionals: string[];
}

export function parseArgs(argv: string[]): ParsedArgs {
  const out: ParsedArgs = { command: null, flags: {}, positionals: [] };
  let i = 0;
  if (argv.length > 0 && !argv[0].startsWith("-")) {
    out.command = argv[0];
    i = 1;
  }
  for (; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--") {
      out.positionals.push(...argv.slice(i + 1));
      break;
    }
    if (a.startsWith("--")) {
      const eq = a.indexOf("=");
      if (eq > 0) {
        out.flags[a.slice(2, eq)] = a.slice(eq + 1);
      } else {
        const key = a.slice(2);
        const next = argv[i + 1];
        if (next !== undefined && !next.startsWith("--")) {
          out.flags[key] = next;
          i++;
        } else {
          out.flags[key] = true;
        }
      }
    } else if (a === "-h") {
      out.flags.help = true;
    } else {
      out.positionals.push(a);
    }
  }
  return out;
}

export interface CommandSpec {
  summary: string;
  required: string[];
  optional: string[];
  oneOf?: string[][];
}

export const COMMANDS: Record<string, CommandSpec> = {
  create: {
    summary: "Compose create_grant from a grant JSON file (hashes each milestone spec).",
    required: ["grant"],
    optional: ["escrow", "wallet", "token", "submit", "out"],
  },
  fund: {
    summary: "Compose fund(id, from, amount) on the escrow.",
    required: ["grant-id", "amount"],
    optional: ["from", "escrow", "submit", "out"],
  },
  submit: {
    summary: "Hash a milestone report and compose wallet.submit_evidence(escrow, id, idx, hash).",
    required: ["grant-id", "milestone", "report"],
    optional: ["escrow", "wallet", "submit", "out"],
  },
  review: {
    summary: "Run the evidence checker on a report; with --approve or --reject compose the reviewer transaction.",
    required: ["grant-id", "milestone", "report"],
    optional: ["grant", "spec", "approve", "reject", "reason", "reviewer", "escrow", "submit", "json", "out"],
    oneOf: [["approve", "reject"]],
  },
  pay: {
    summary: "Compose wallet.pay(category, payee, amount, memo_hash); --batch previews a payments CSV against the policy.",
    required: [],
    optional: ["category", "payee", "amount", "memo", "batch", "budget", "payees", "wallet", "submit", "out"],
  },
  ledger: {
    summary: "Spend per category from paid events (RPC when reachable, --events JSON, or --simulate a payments CSV).",
    required: [],
    optional: ["events", "simulate", "budget", "payees", "wallet", "json"],
  },
  export: {
    summary: "Export paid events to CSV.",
    required: [],
    optional: ["events", "simulate", "budget", "payees", "wallet", "out"],
  },
};

export type Validated =
  | { ok: true; command: string; flags: Record<string, string | boolean> }
  | { ok: false; error: string };

export function validateCommand(parsed: ParsedArgs): Validated {
  if (!parsed.command || parsed.flags.help) {
    return { ok: false, error: usage() };
  }
  const spec = COMMANDS[parsed.command];
  if (!spec) return { ok: false, error: `unknown command "${parsed.command}"\n\n${usage()}` };
  const missing = spec.required.filter((k) => parsed.flags[k] === undefined || parsed.flags[k] === true);
  if (missing.length) {
    return { ok: false, error: `${parsed.command}: missing required option(s): ${missing.map((m) => "--" + m).join(", ")}` };
  }
  const known = new Set([...spec.required, ...spec.optional, "help", "seed-dir"]);
  const unknown = Object.keys(parsed.flags).filter((k) => !known.has(k));
  if (unknown.length) {
    return { ok: false, error: `${parsed.command}: unknown option(s): ${unknown.map((m) => "--" + m).join(", ")}` };
  }
  for (const group of spec.oneOf ?? []) {
    const set = group.filter((k) => parsed.flags[k] !== undefined);
    if (set.length > 1) return { ok: false, error: `${parsed.command}: use only one of ${group.map((m) => "--" + m).join(", ")}` };
  }
  if (parsed.command === "pay" && parsed.flags.batch === undefined) {
    const need = ["category", "payee", "amount"].filter((k) => parsed.flags[k] === undefined);
    if (need.length) return { ok: false, error: `pay: missing required option(s): ${need.map((m) => "--" + m).join(", ")} (or --batch <csv>)` };
  }
  return { ok: true, command: parsed.command, flags: parsed.flags };
}

export function usage(): string {
  const lines = ["usage: grantflow <command> [options]", ""];
  for (const [name, spec] of Object.entries(COMMANDS)) {
    const req = spec.required.map((r) => `--${r} <v>`).join(" ");
    lines.push(`  ${name.padEnd(8)} ${req}`.trimEnd());
    lines.push(`           ${spec.summary}`);
  }
  lines.push("", "Global: --seed-dir <dir> (default: ../data/seed), --submit (send when SOROBAN_RPC_URL is reachable), --out <file>");
  return lines.join("\n");
}
