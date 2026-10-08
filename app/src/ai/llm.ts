/**
 * LLM-backed evidence checker. Used only when LLM_API_KEY is set; every test
 * runs against the heuristic provider instead. The model returns strict
 * JSON validated against EVIDENCE_JSON_SCHEMA; one retry with the validation
 * errors appended; quotes that are not verbatim in the report are downgraded.
 *
 * Failure mode when the model is wrong: a deliverable marked "found" that is
 * not in the report. The mitigation is structural: the checklist is advice
 * shown to reviewers next to the citation, and only a reviewer signature
 * moves a milestone. A wrong "missing" costs a reviewer a minute; a wrong
 * "found" is caught when the cited quote does not support the claim.
 */
import Anthropic from "@anthropic-ai/sdk";
import { parseDeliverables } from "./spec.js";
import { summarize } from "./heuristic.js";
import { EVIDENCE_JSON_SCHEMA, toChecks, validateEvidenceJson } from "./schema.js";
import type { EvidenceCheck, EvidenceInput, EvidenceProvider } from "./types.js";

export const DEFAULT_LLM_MODEL = "claude-sonnet-5";

export interface LlmProviderOptions {
  apiKey: string;
  model?: string;
  baseURL?: string;
  /** Injected for tests; never used by the CLI. */
  client?: Pick<Anthropic, "messages">;
  maxRetries?: number;
}

const SYSTEM_PROMPT = `You check whether a grantee's milestone report contains evidence for each deliverable in the milestone specification.
Rules:
- Judge each deliverable independently. "found" only when the report states the deliverable is done and gives concrete detail (numbers, dates, file or annex names). Promises, plans or "will be delivered" are "uncertain". Nothing relevant is "missing".
- "quote" must be a verbatim sentence copied from the report that supports your verdict, or null.
- "confidence" is your confidence in the status, between 0 and 1.
- Return only JSON matching the schema, with exactly one entry per deliverable id.`;

export class LlmProvider implements EvidenceProvider {
  readonly name: string;
  private readonly client: Pick<Anthropic, "messages">;
  private readonly model: string;
  private readonly maxRetries: number;

  constructor(opts: LlmProviderOptions) {
    this.model = opts.model ?? DEFAULT_LLM_MODEL;
    this.name = `llm:${this.model}`;
    this.maxRetries = opts.maxRetries ?? 1;
    this.client =
      opts.client ??
      new Anthropic({ apiKey: opts.apiKey, ...(opts.baseURL ? { baseURL: opts.baseURL } : {}) });
  }

  async check(input: EvidenceInput): Promise<EvidenceCheck> {
    const deliverables = parseDeliverables(input.spec);
    if (deliverables.length === 0) return summarize(this.name, []);
    const ids = deliverables.map((d) => d.id);
    const texts = new Map(deliverables.map((d) => [d.id, d.text]));
    const userPrompt = [
      "Milestone deliverables:",
      ...deliverables.map((d) => `${d.id}. ${d.text}`),
      "",
      "Grantee report:",
      "<report>",
      input.report,
      "</report>",
    ].join("\n");

    let lastErrors: string[] = [];
    for (let attempt = 0; attempt <= this.maxRetries; attempt++) {
      const content =
        attempt === 0
          ? userPrompt
          : `${userPrompt}\n\nYour previous answer was rejected: ${lastErrors.join("; ")}. Return corrected JSON.`;
      const response = await this.client.messages.create({
        model: this.model,
        max_tokens: 4096,
        system: SYSTEM_PROMPT,
        messages: [{ role: "user", content }],
        output_config: { format: { type: "json_schema", schema: EVIDENCE_JSON_SCHEMA as unknown as Record<string, unknown> } },
      });
      if (response.stop_reason === "refusal") {
        throw new Error("evidence check refused by the model; fall back to the heuristic provider or review manually");
      }
      const text = response.content
        .filter((b): b is Extract<typeof b, { type: "text" }> => b.type === "text")
        .map((b) => b.text)
        .join("");
      let parsed: unknown;
      try {
        parsed = JSON.parse(text);
      } catch {
        lastErrors = ["response was not valid JSON"];
        continue;
      }
      const validation = validateEvidenceJson(parsed, ids);
      if (!validation.ok) {
        lastErrors = validation.errors;
        continue;
      }
      return summarize(this.name, toChecks(validation.value, texts, input.report));
    }
    throw new Error(`evidence check failed schema validation after retry: ${lastErrors.join("; ")}`);
  }
}
