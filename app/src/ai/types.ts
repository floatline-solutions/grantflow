/**
 * Milestone evidence checking.
 *
 * Input: the milestone specification (markdown with a numbered deliverables
 * list) and the grantee's report. Output: one verdict per deliverable with a
 * citation into the report. Reviewers decide; this module never approves,
 * rejects or signs anything.
 */
export type DeliverableStatus = "found" | "missing" | "uncertain";

export interface Deliverable {
  id: number;
  text: string;
}

export interface DeliverableCheck {
  id: number;
  text: string;
  status: DeliverableStatus;
  /** Verbatim sentence(s) from the report supporting the verdict, or null. */
  quote: string | null;
  /** 0..1, confidence in the assigned status. */
  confidence: number;
  /** Provider-specific note (e.g. why a match was downgraded). */
  note?: string;
}

export interface EvidenceCheck {
  provider: string;
  deliverables: DeliverableCheck[];
  found: number;
  missing: number;
  uncertain: number;
  summary: string;
}

export interface EvidenceInput {
  spec: string;
  report: string;
}

export interface EvidenceProvider {
  readonly name: string;
  check(input: EvidenceInput): Promise<EvidenceCheck>;
}
