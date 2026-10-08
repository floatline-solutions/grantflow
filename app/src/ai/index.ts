import { HeuristicProvider } from "./heuristic.js";
import { LlmProvider } from "./llm.js";
import type { EvidenceCheck, EvidenceProvider } from "./types.js";

export * from "./types.js";
export { HeuristicProvider } from "./heuristic.js";
export { LlmProvider, DEFAULT_LLM_MODEL } from "./llm.js";
export { parseDeliverables } from "./spec.js";
export { validateEvidenceJson, EVIDENCE_JSON_SCHEMA } from "./schema.js";

/** LLM when a key is configured, deterministic heuristic otherwise. */
export function createProvider(env: NodeJS.ProcessEnv = process.env): EvidenceProvider {
  const apiKey = env.LLM_API_KEY?.trim();
  if (apiKey) {
    return new LlmProvider({
      apiKey,
      model: env.LLM_MODEL?.trim() || undefined,
      baseURL: env.LLM_BASE_URL?.trim() || undefined,
    });
  }
  return new HeuristicProvider();
}

export async function checkEvidence(spec: string, report: string, provider: EvidenceProvider): Promise<EvidenceCheck> {
  return provider.check({ spec, report });
}

/** Plain-text rendering for the terminal. */
export function renderCheck(check: EvidenceCheck): string {
  const mark = { found: "[found]    ", missing: "[missing]  ", uncertain: "[uncertain]" } as const;
  const lines = [`Evidence check (${check.provider}): ${check.summary}`];
  for (const d of check.deliverables) {
    lines.push(`${mark[d.status]} #${d.id} ${d.text}  (confidence ${d.confidence.toFixed(2)})`);
    if (d.quote) lines.push(`            > ${d.quote}`);
    if (d.note) lines.push(`            note: ${d.note}`);
  }
  return lines.join("\n");
}
