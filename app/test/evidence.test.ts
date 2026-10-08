import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { HeuristicProvider, createProvider, parseDeliverables, validateEvidenceJson } from "../src/ai/index.js";
import { hedged, stem, tokenize } from "../src/ai/heuristic.js";
import { LlmProvider } from "../src/ai/llm.js";
import { loadGrant } from "../src/seed.js";

const APP = path.resolve(import.meta.dirname, "..", "..");
const SEED = path.resolve(APP, "..", "data", "seed");
const FIXTURES = path.join(APP, "fixtures", "evidence");

interface Fixture {
  name: string;
  spec_from_grant_milestone: number;
  report_file: string;
  expected: string[];
}

function loadFixture(name: string): { fixture: Fixture; spec: string; report: string } {
  const fixture = JSON.parse(fs.readFileSync(path.join(FIXTURES, `${name}.json`), "utf8")) as Fixture;
  const grant = loadGrant(SEED);
  const spec = grant.milestones[fixture.spec_from_grant_milestone].spec_markdown;
  const report = fs.readFileSync(path.join(SEED, fixture.report_file), "utf8");
  return { fixture, spec, report };
}

test("parseDeliverables reads numbered lists and folds continuation lines", () => {
  const items = parseDeliverables("# M\n\nDeliverables:\n1. First thing\n   continued here\n2) Second thing\n\n3. Third\nHeading-free line ignored after blank? no: folded\n");
  assert.equal(items.length, 3);
  assert.equal(items[0].text, "First thing continued here");
  assert.equal(items[1].text, "Second thing");
  assert.equal(items[2].id, 3);
  const bullets = parseDeliverables("- [ ] alpha\n- beta\n* gamma\n");
  assert.deepEqual(bullets.map((b) => b.text), ["alpha", "beta", "gamma"]);
  assert.deepEqual(parseDeliverables("no list here"), []);
});

test("tokenizer stems consistently and drops stopwords", () => {
  assert.equal(stem("training"), stem("trained"));
  assert.equal(stem("enumerators"), stem("enumerator"));
  assert.equal(stem("completed"), stem("complete"));
  assert.equal(stem("categories"), stem("category"));
  assert.equal(stem("sampling"), stem("sample"));
  assert.deepEqual(tokenize("The 12 enumerators, at least, were trained (90%)."), ["12", "enumerator", "train", "90"]);
  assert.equal(hedged("The dataset will be delivered next week"), "will be");
  assert.equal(hedged("The dataset is attached as Annex A"), null);
});

for (const name of ["complete", "partial", "empty"]) {
  test(`heuristic provider on fixture "${name}"`, async () => {
    const { fixture, spec, report } = loadFixture(name);
    const check = await new HeuristicProvider().check({ spec, report });
    assert.equal(check.provider, "heuristic");
    assert.equal(check.deliverables.length, fixture.expected.length, fixture.name);
    assert.deepEqual(
      check.deliverables.map((d) => d.status),
      fixture.expected,
      `${fixture.name}\n${JSON.stringify(check.deliverables, null, 2)}`,
    );
    for (const d of check.deliverables) {
      assert.ok(d.confidence >= 0 && d.confidence <= 1);
      if (d.status === "found") {
        assert.ok(d.quote, `found deliverable #${d.id} must cite the report`);
        assert.ok(report.replace(/\s+/g, " ").includes(d.quote!.slice(0, 40)), "quote is verbatim");
      }
    }
    assert.equal(check.found + check.missing + check.uncertain, check.deliverables.length);
    assert.match(check.summary, /deliverables found/);
  });
}

test("heuristic provider handles an empty report and an empty spec", async () => {
  const { spec } = loadFixture("empty");
  const check = await new HeuristicProvider().check({ spec, report: "" });
  assert.equal(check.missing, 5);
  assert.ok(check.deliverables.every((d) => d.quote === null));
  const none = await new HeuristicProvider().check({ spec: "nothing numbered", report: "some report" });
  assert.equal(none.deliverables.length, 0);
  assert.match(none.summary, /No numbered deliverables/);
});

test("schema validator accepts valid output and rejects malformed output", () => {
  const good = { deliverables: [{ id: 1, status: "found", quote: "x", confidence: 0.9 }, { id: 2, status: "missing", quote: null, confidence: 0.8 }], summary: "ok" };
  assert.equal(validateEvidenceJson(good, [1, 2]).ok, true);
  const bad = validateEvidenceJson({ deliverables: [{ id: 1, status: "maybe", quote: 3, confidence: 2 }], summary: 1 }, [1, 2]);
  assert.equal(bad.ok, false);
  if (!bad.ok) {
    assert.ok(bad.errors.some((e) => e.includes("status")));
    assert.ok(bad.errors.some((e) => e.includes("confidence")));
    assert.ok(bad.errors.some((e) => e.includes("summary")));
    assert.ok(bad.errors.some((e) => e.includes("missing entry for deliverable 2")));
  }
  assert.equal(validateEvidenceJson("nope").ok, false);
  assert.equal(validateEvidenceJson({ deliverables: [{ id: 1, status: "found", quote: null, confidence: 1 }, { id: 1, status: "found", quote: null, confidence: 1 }], summary: "" }).ok, false);
});

test("createProvider picks the heuristic without a key and the LLM with one", () => {
  assert.equal(createProvider({}).name, "heuristic");
  assert.equal(createProvider({ LLM_API_KEY: "   " }).name, "heuristic");
  const llm = createProvider({ LLM_API_KEY: "test-key", LLM_MODEL: "some-model" });
  assert.equal(llm.name, "llm:some-model");
  assert.equal(createProvider({ LLM_API_KEY: "test-key" }).name, "llm:claude-sonnet-5");
});

test("LLM provider validates strict JSON, retries once and downgrades fabricated quotes (stubbed client)", async () => {
  const { spec, report } = loadFixture("partial");
  const calls: string[] = [];
  const answers = [
    JSON.stringify({ deliverables: [{ id: 1, status: "found", quote: "The survey was completed with 561 households", confidence: 0.9 }], summary: "incomplete" }),
    JSON.stringify({
      deliverables: [
        { id: 1, status: "found", quote: "The survey was completed with 561 households out of the 600 sampled (93.5% response rate).", confidence: 0.9 },
        { id: 2, status: "found", quote: "The cleaned dataset was delivered on 2 January.", confidence: 0.8 },
        { id: 3, status: "missing", quote: null, confidence: 0.85 },
        { id: 4, status: "uncertain", quote: null, confidence: 0.6 },
        { id: 5, status: "uncertain", quote: null, confidence: 0.6 },
      ],
      summary: "1 found, 2 uncertain, 1 missing, 1 fabricated",
    }),
  ];
  const client = {
    messages: {
      create: async (req: { messages: { content: string }[] }) => {
        calls.push(req.messages[0].content);
        return { stop_reason: "end_turn", content: [{ type: "text", text: answers[calls.length - 1] }] };
      },
    },
  };
  const provider = new LlmProvider({ apiKey: "k", model: "m", client: client as never });
  const check = await provider.check({ spec, report });
  assert.equal(calls.length, 2, "first answer lacked entries, so exactly one retry");
  assert.match(calls[1], /previous answer was rejected/);
  assert.equal(check.deliverables[0].status, "found");
  assert.equal(check.deliverables[1].status, "uncertain", "quote not in the report is downgraded");
  assert.equal(check.deliverables[1].quote, null);
  assert.equal(check.found, 1);
});
