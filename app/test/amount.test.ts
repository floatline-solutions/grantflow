import { test } from "node:test";
import assert from "node:assert/strict";
import { AmountError, formatAmount, parseAmount } from "../src/amount.js";

test("parseAmount accepts the messy formats in the seed data", () => {
  assert.equal(parseAmount("1,250.00"), 12_500_000_000n);
  assert.equal(parseAmount("2000"), 20_000_000_000n);
  assert.equal(parseAmount("USD 1875.50"), 18_755_000_000n);
  assert.equal(parseAmount("$450"), 4_500_000_000n);
  assert.equal(parseAmount(" 120 "), 1_200_000_000n);
  assert.equal(parseAmount("10 000.0"), 100_000_000_000n);
  assert.equal(parseAmount("499.99"), 4_999_900_000n);
  assert.equal(parseAmount("24.50 USDC"), 245_000_000n);
  assert.equal(parseAmount("0.0000001"), 1n);
  assert.equal(parseAmount("+5"), 50_000_000n);
});

test("parseAmount rejects what it cannot interpret safely", () => {
  for (const bad of ["", "   ", "1.2k", "(50.00)", "-3", "abc", "1.00000001", "1,2,3.4.5", "12 34"]) {
    assert.throws(() => parseAmount(bad), AmountError, bad);
  }
});

test("formatAmount round-trips with at least two decimals", () => {
  assert.equal(formatAmount(12_500_000_000n), "1250.00");
  assert.equal(formatAmount(4_999_900_000n), "499.99");
  assert.equal(formatAmount(1n), "0.0000001");
  assert.equal(formatAmount(0n), "0.00");
  assert.equal(formatAmount(-245_000_000n), "-24.50");
  assert.equal(formatAmount(parseAmount("1,250.00")), "1250.00");
});
