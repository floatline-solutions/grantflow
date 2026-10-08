import { test } from "node:test";
import assert from "node:assert/strict";
import { COMMANDS, parseArgs, usage, validateCommand } from "../src/args.js";

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
    const argv = [name, ...spec.required.flatMap((r) => [`--${r}`, "v"])];
    if (name === "pay") argv.push("--batch", "x.csv");
    assert.equal(validateCommand(parseArgs(argv)).ok, true, name);
  }
  assert.match(usage(), /grantflow <command>/);
  for (const name of ["create", "fund", "submit", "review", "pay", "ledger", "export"]) assert.ok(name in COMMANDS, name);
});
