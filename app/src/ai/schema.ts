/** JSON schema of the checker output, and a dependency-free validator. */
import type { DeliverableCheck, DeliverableStatus } from "./types.js";

export const STATUSES: DeliverableStatus[] = ["found", "missing", "uncertain"];

export const EVIDENCE_JSON_SCHEMA = {
  type: "object",
  additionalProperties: false,
  required: ["deliverables", "summary"],
  properties: {
    deliverables: {
      type: "array",
      items: {
        type: "object",
        additionalProperties: false,
        required: ["id", "status", "quote", "confidence"],
        properties: {
          id: { type: "integer", minimum: 1 },
          status: { type: "string", enum: STATUSES },
          quote: { type: ["string", "null"] },
          confidence: { type: "number", minimum: 0, maximum: 1 },
        },
      },
    },
    summary: { type: "string" },
  },
} as const;

export interface LlmEvidenceJson {
  deliverables: { id: number; status: DeliverableStatus; quote: string | null; confidence: number }[];
  summary: string;
}

export type Validation = { ok: true; value: LlmEvidenceJson } | { ok: false; errors: string[] };

/** Validate a parsed JSON value; `expectedIds` enforces one entry per deliverable. */
export function validateEvidenceJson(value: unknown, expectedIds?: number[]): Validation {
  const errors: string[] = [];
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return { ok: false, errors: ["root must be an object"] };
  }
  const obj = value as Record<string, unknown>;
  if (typeof obj.summary !== "string") errors.push("summary must be a string");
  if (!Array.isArray(obj.deliverables)) {
    errors.push("deliverables must be an array");
    return { ok: false, errors };
  }
  const seen = new Set<number>();
  obj.deliverables.forEach((d, i) => {
    if (typeof d !== "object" || d === null) {
      errors.push(`deliverables[${i}] must be an object`);
      return;
    }
    const e = d as Record<string, unknown>;
    if (!Number.isInteger(e.id) || (e.id as number) < 1) errors.push(`deliverables[${i}].id must be a positive integer`);
    else if (seen.has(e.id as number)) errors.push(`deliverables[${i}].id ${e.id} repeated`);
    else seen.add(e.id as number);
    if (!STATUSES.includes(e.status as DeliverableStatus)) errors.push(`deliverables[${i}].status must be one of ${STATUSES.join("|")}`);
    if (!(e.quote === null || typeof e.quote === "string")) errors.push(`deliverables[${i}].quote must be a string or null`);
    if (typeof e.confidence !== "number" || e.confidence < 0 || e.confidence > 1) errors.push(`deliverables[${i}].confidence must be a number in [0,1]`);
  });
  if (expectedIds) {
    for (const id of expectedIds) if (!seen.has(id)) errors.push(`missing entry for deliverable ${id}`);
    for (const id of seen) if (!expectedIds.includes(id)) errors.push(`unexpected deliverable id ${id}`);
  }
  if (errors.length) return { ok: false, errors };
  return { ok: true, value: value as LlmEvidenceJson };
}

/** Quotes must come from the report; drop fabricated ones. */
export function quoteIsVerbatim(quote: string | null, report: string): boolean {
  if (quote === null) return true;
  const norm = (s: string) => s.toLowerCase().replace(/\s+/g, " ").trim();
  return norm(report).includes(norm(quote));
}

export function toChecks(value: LlmEvidenceJson, texts: Map<number, string>, report: string): DeliverableCheck[] {
  return value.deliverables
    .slice()
    .sort((a, b) => a.id - b.id)
    .map((d) => {
      const verbatim = quoteIsVerbatim(d.quote, report);
      return {
        id: d.id,
        text: texts.get(d.id) ?? "",
        status: verbatim ? d.status : "uncertain",
        quote: verbatim ? d.quote : null,
        confidence: verbatim ? d.confidence : 0.4,
        ...(verbatim ? {} : { note: "model quote was not found verbatim in the report; downgraded" }),
      };
    });
}
