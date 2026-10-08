/**
 * USDC amounts. On Stellar the asset has 7 decimals; contracts take i128
 * stroops. Seed data and human input arrive in every imaginable format
 * ("1,250.00", "USD 340.75", "$800", " 120 ", "10 000.0"), so parsing is
 * strict about meaning and lenient about formatting.
 */
export const USDC_DECIMALS = 7;
const SCALE = 10n ** BigInt(USDC_DECIMALS);

export class AmountError extends Error {}

/** Parse a human amount into stroops (bigint). Throws AmountError. */
export function parseAmount(raw: string): bigint {
  if (typeof raw !== "string") throw new AmountError("amount must be a string");
  let s = raw.trim();
  if (s === "") throw new AmountError("empty amount");
  s = s
    .replace(/^(usdc|usd|\$)\s*/i, "")
    .replace(/\s*(usdc|usd)$/i, "")
    .trim();
  if (/^\(.*\)$/.test(s) || s.startsWith("-")) {
    throw new AmountError(`negative amount not allowed: "${raw}"`);
  }
  if (s.startsWith("+")) s = s.slice(1);
  // Thousands separators: commas anywhere, or a space between digit groups.
  s = s.replace(/,/g, "").replace(/(\d)\s+(?=\d{3}(\D|$))/g, "$1");
  if (!/^\d+(\.\d+)?$/.test(s)) throw new AmountError(`unparseable amount: "${raw}"`);
  const [whole, frac = ""] = s.split(".");
  if (frac.length > USDC_DECIMALS) {
    throw new AmountError(`more than ${USDC_DECIMALS} decimals: "${raw}"`);
  }
  return BigInt(whole) * SCALE + BigInt(frac.padEnd(USDC_DECIMALS, "0") || "0");
}

/** Format stroops as a decimal string with at least `minDecimals` places. */
export function formatAmount(stroops: bigint, minDecimals = 2): string {
  const negative = stroops < 0n;
  const abs = negative ? -stroops : stroops;
  const whole = abs / SCALE;
  let frac = (abs % SCALE).toString().padStart(USDC_DECIMALS, "0").replace(/0+$/, "");
  if (frac.length < minDecimals) frac = frac.padEnd(minDecimals, "0");
  return `${negative ? "-" : ""}${whole}${frac ? "." + frac : ""}`;
}

/** Try to parse; returns null instead of throwing. */
export function tryParseAmount(raw: string): { value: bigint } | { error: string } {
  try {
    return { value: parseAmount(raw) };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}
