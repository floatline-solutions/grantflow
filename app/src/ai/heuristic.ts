/**
 * Deterministic evidence checker: keyword and phrase coverage with sentence
 * citations. It is the provider used in tests and the fallback when no LLM
 * key is configured. It cannot judge quality; it can only say whether the
 * report talks about each deliverable in concrete terms.
 */
import { parseDeliverables } from "./spec.js";
import type {
  DeliverableCheck,
  DeliverableStatus,
  EvidenceCheck,
  EvidenceInput,
  EvidenceProvider,
} from "./types.js";

const STOPWORDS = new Set([
  "a", "an", "the", "and", "or", "of", "in", "on", "at", "to", "for", "with", "by",
  "from", "as", "is", "are", "was", "were", "be", "been", "being", "this", "that",
  "these", "those", "it", "its", "into", "per", "than", "then", "each", "all", "any",
  "if", "not", "no", "we", "our", "they", "their", "have", "has", "had", "will",
  "which", "who", "whom", "where", "when", "also", "least", "least", "one", "two",
  "three", "via", "using", "use", "such", "within", "between", "over", "under",
  "about", "after", "before", "during", "including", "etc", "e", "g", "i",
]);

/** Language that describes intent rather than a completed deliverable. */
const HEDGES = [
  "will be", "will follow", "will deliver", "ongoing", "not yet", "to be delivered",
  "in the next", "pending", "has not", "have not", "was not", "were not", "could not",
  "in progress", "is being", "are being", "to follow", "planned for", "expected to",
  "postponed", "delayed", "did not",
];

export const FOUND_THRESHOLD = 0.6;
export const UNCERTAIN_THRESHOLD = 0.3;

export function stem(word: string): string {
  let w = word;
  if (/^\d+$/.test(w)) return w;
  if (w.length > 5 && w.endsWith("ing")) w = w.slice(0, -3);
  else if (w.length > 4 && w.endsWith("ies")) w = w.slice(0, -3) + "i";
  else if (w.length > 4 && w.endsWith("ed")) w = w.slice(0, -2);
  else if (w.length > 3 && w.endsWith("s") && !w.endsWith("ss")) w = w.slice(0, -1);
  if (w.length > 4 && w.endsWith("y")) w = w.slice(0, -1) + "i";
  if (w.length > 3 && w.endsWith("e")) w = w.slice(0, -1);
  return w;
}

export function tokenize(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/[’']/g, "")
    .split(/[^a-z0-9%]+/)
    .filter((t) => t.length > 0)
    .map((t) => t.replace(/%$/, ""))
    .filter((t) => (/^\d+$/.test(t) ? t.length >= 2 : t.length >= 3))
    .filter((t) => !STOPWORDS.has(t))
    .map(stem);
}

export function keywords(text: string): string[] {
  return [...new Set(tokenize(text))];
}

function cleanSentence(s: string): string {
  return s
    .replace(/^\s*#{1,6}\s*/, "")
    .replace(/^\s*[-*]\s+/, "")
    .replace(/\*\*/g, "")
    .replace(/`/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

export function sentences(report: string): string[] {
  return report
    .replace(/\r\n?/g, "\n")
    .split(/(?<=[.!?])\s+|\n+/)
    .map(cleanSentence)
    .filter((s) => s.length > 0);
}

function coverage(keys: string[], tokens: Set<string>): number {
  if (keys.length === 0) return 0;
  let hit = 0;
  for (const k of keys) if (tokens.has(k)) hit++;
  return hit / keys.length;
}

export function hedged(text: string): string | null {
  const lower = text.toLowerCase();
  for (const h of HEDGES) if (lower.includes(h)) return h;
  return null;
}

export interface DeliverableScore {
  global: number;
  local: number;
  score: number;
  bestIndex: number;
}

export function scoreDeliverable(keys: string[], sents: string[], sentTokens: Set<string>[], reportTokens: Set<string>): DeliverableScore {
  const global = coverage(keys, reportTokens);
  let local = 0;
  let bestIndex = -1;
  for (let i = 0; i < sents.length; i++) {
    const window = new Set(sentTokens[i]);
    if (i + 1 < sents.length) for (const t of sentTokens[i + 1]) window.add(t);
    const c = coverage(keys, window);
    if (c > local) {
      local = c;
      bestIndex = i;
    }
  }
  return { global, local, score: 0.6 * global + 0.4 * local, bestIndex };
}

export class HeuristicProvider implements EvidenceProvider {
  readonly name = "heuristic";

  async check(input: EvidenceInput): Promise<EvidenceCheck> {
    return this.checkSync(input);
  }

  checkSync(input: EvidenceInput): EvidenceCheck {
    const deliverables = parseDeliverables(input.spec);
    const sents = sentences(input.report);
    const sentTokens = sents.map((s) => new Set(tokenize(s)));
    const reportTokens = new Set(tokenize(input.report));

    const checks: DeliverableCheck[] = deliverables.map((d) => {
      const keys = keywords(d.text);
      const { global, local, score } = scoreDeliverable(keys, sents, sentTokens, reportTokens);

      // Sentences that carry the match: at least two keywords (or 40% of them).
      const hits = sentTokens.map((st) => keys.filter((k) => st.has(k)).length);
      const minHits = Math.min(keys.length, Math.max(2, Math.ceil(keys.length * 0.4)));
      const evidenceIdx = hits.map((h, i) => (h >= minHits ? i : -1)).filter((i) => i >= 0);

      // Cite the single sentence with the most keywords; ties go to the richer sentence.
      let quoteIndex = -1;
      for (let i = 0; i < sents.length; i++) {
        if (hits[i] === 0) continue;
        if (quoteIndex < 0 || hits[i] > hits[quoteIndex] || (hits[i] === hits[quoteIndex] && sentTokens[i].size > sentTokens[quoteIndex].size)) {
          quoteIndex = i;
        }
      }

      let status: DeliverableStatus;
      let confidence: number;
      let note: string | undefined;
      if (score >= FOUND_THRESHOLD) {
        status = "found";
        confidence = Math.min(0.95, 0.55 + score / 2);
      } else if (score >= UNCERTAIN_THRESHOLD) {
        status = "uncertain";
        confidence = 0.5;
      } else {
        status = "missing";
        confidence = Math.min(0.9, 0.9 - score);
      }
      // A matching sentence that promises the deliverable is not evidence of it.
      let hedge: string | null = null;
      let hedgeIndex = -1;
      if (status === "found") {
        for (const i of evidenceIdx) {
          hedge = hedged(sents[i]);
          if (hedge) {
            hedgeIndex = i;
            break;
          }
        }
      }
      if (hedge) {
        status = "uncertain";
        confidence = 0.5;
        quoteIndex = hedgeIndex;
        note = `matched text reads like a promise ("${hedge}"), not delivered evidence`;
      }
      const quote = status !== "missing" && quoteIndex >= 0 ? sents[quoteIndex].slice(0, 240) : null;
      if (process.env.GRANTFLOW_DEBUG) {
        note = `${note ? note + "; " : ""}global=${global.toFixed(2)} local=${local.toFixed(2)} score=${score.toFixed(2)}`;
      }
      return {
        id: d.id,
        text: d.text,
        status,
        quote,
        confidence: Math.round(confidence * 100) / 100,
        ...(note ? { note } : {}),
      };
    });

    return summarize("heuristic", checks);
  }
}

export function summarize(provider: string, checks: DeliverableCheck[]): EvidenceCheck {
  const found = checks.filter((c) => c.status === "found").length;
  const missing = checks.filter((c) => c.status === "missing").length;
  const uncertain = checks.filter((c) => c.status === "uncertain").length;
  const parts = [`${found} of ${checks.length} deliverables found`];
  if (uncertain) parts.push(`${uncertain} uncertain`);
  if (missing) parts.push(`${missing} missing`);
  const flagged = checks
    .filter((c) => c.status !== "found")
    .map((c) => `#${c.id} (${c.status})`)
    .join(", ");
  const summary =
    checks.length === 0
      ? "No numbered deliverables were found in the milestone spec."
      : `${parts.join(", ")}.${flagged ? ` Needs reviewer attention: ${flagged}.` : ""}`;
  return { provider, deliverables: checks, found, missing, uncertain, summary };
}
