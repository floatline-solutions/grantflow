import { test } from "node:test";
import assert from "node:assert/strict";
import { COMMANDS, parseArgs, parseDeadline, usage, validateCommand } from "../src/args.js";

test("parseArgs handles --key value, --key=value, booleans and positionals", () => {
  const p = parseArgs(["pay", "--category", "fieldwork", "--amount=340.75", "--submit", "--payee", "V-004", "extra"]);
  assert.equal(p.command, "pay");
  assert.deepEqual(p.flags, { category: "fieldwork", amount: "340.75", submit: true, payee: "V-004" });
  assert.deepEqual(p.positionals, ["extra"]);
  assert.equal(parseArgs([]).command, null);
  assert.equal(parseArgs(["-h"]).flags.help, true);
  assert.deepEqual(parseArgs(["ledger", "--", "--not-a-flag"]).positionals, ["--not-a-flag"]);
});

test("validateCommand enforces required options per command", () => {
  const ok = validateCommand(parseArgs(["pay", "--category", "fieldwork", "--payee", "V-004", "--amount", "340.75", "--memo", "TN-301"]));
  assert.equal(ok.ok, true);

  const missing = validateCommand(parseArgs(["pay", "--category", "fieldwork"]));
  assert.equal(missing.ok, false);
  if (!missing.ok) assert.match(missing.error, /--payee, --amount/);

  const batch = validateCommand(parseArgs(["pay", "--batch", "payments.csv"]));
  assert.equal(batch.ok, true);

  const review = validateCommand(parseArgs(["review", "--grant-id", "0", "--milestone", "1", "--report", "r.md", "--approve", "--reject"]));
  assert.equal(review.ok, false);
  if (!review.ok) assert.match(review.error, /only one of --approve, --reject/);

  const unknownCmd = validateCommand(parseArgs(["frobnicate"]));
  assert.equal(unknownCmd.ok, false);
  if (!unknownCmd.ok) assert.match(unknownCmd.error, /unknown command/);

  const unknownOpt = validateCommand(parseArgs(["fund", "--grant-id", "0", "--amount", "10", "--bogus", "1"]));
  assert.equal(unknownOpt.ok, false);
  if (!unknownOpt.ok) assert.match(unknownOpt.error, /--bogus/);

  const valueless = validateCommand(parseArgs(["submit", "--grant-id", "--milestone", "0", "--report", "x"]));
  assert.equal(valueless.ok, false, "a required option given without a value is missing");

  for (const [name, spec] of Object.entries(COMMANDS)) {
    const argv = [name, ...spec.required.flatMap((r) => [`--${r}`, r === "grant-id" || r === "milestone" ? "0" : "v"])];
    if (name === "pay") argv.push("--batch", "x.csv");
    assert.equal(validateCommand(parseArgs(argv)).ok, true, name);
  }
  assert.match(usage(), /grantflow <command>/);
  for (const name of ["create", "fund", "submit", "review", "pay", "ledger", "export"]) assert.ok(name in COMMANDS, name);
});

test("escrow lifecycle commands validate their options", () => {
  const ok = (argv: string[]) => validateCommand(parseArgs(argv));
  assert.equal(ok(["release", "--grant-id", "0", "--milestone", "1"]).ok, true);
  assert.equal(ok(["clawback", "--grant-id", "0"]).ok, true);
  assert.equal(ok(["cancel", "--grant-id", "3"]).ok, true);
  assert.equal(ok(["extend-deadline", "--grant-id", "0", "--deadline", "2027-01-31"]).ok, true);
  const missing = ok(["extend-deadline", "--grant-id", "0"]);
  assert.equal(missing.ok, false);
  const bad = ok(["release", "--grant-id", "abc", "--milestone", "0"]);
  assert.equal(bad.ok, false);
  assert.match((bad as { error: string }).error, /--grant-id must be a non-negative integer/);
  assert.equal(ok(["clawback", "--grant-id", "-1"]).ok, false);
  for (const c of ["release", "clawback", "extend-deadline", "cancel"]) assert.match(usage(), new RegExp(c));
});

test("parseDeadline accepts unix seconds and dates, rejects the rest", () => {
  assert.equal(parseDeadline("1800000000"), 1_800_000_000n);
  assert.equal(parseDeadline("2027-01-31"), BigInt(Date.UTC(2027, 0, 31, 23, 59, 59) / 1000));
  assert.throws(() => parseDeadline("next week"), /cannot parse deadline/);
});
