import { createHash } from "node:crypto";

/** Line endings and trailing whitespace must not change a document's hash. */
export function normalizeText(text: string): string {
  return (
    text
      .replace(/\r\n?/g, "\n")
      .split("\n")
      .map((line) => line.replace(/[ \t]+$/, ""))
      .join("\n")
      .trim() + "\n"
  );
}

export function sha256Hex(text: string): string {
  return createHash("sha256").update(normalizeText(text), "utf8").digest("hex");
}

export function sha256Bytes(text: string): Buffer {
  return Buffer.from(sha256Hex(text), "hex");
}

export function hexToBytes(hex: string): Buffer {
  const clean = hex.replace(/^0x/, "");
  if (!/^[0-9a-fA-F]{64}$/.test(clean)) throw new Error(`expected 32-byte hex, got "${hex}"`);
  return Buffer.from(clean, "hex");
}
