// The newer concepts in the map: quiet reflexes, procedure cards, value-based evidence, rewrites
// and savings, read from the graph JSON (and from older binaries that sent none of it).

import { describe, expect, it } from "vitest";
import { normalizeEvent, normalizeGraph, normalizeRoute } from "../api/normalize";
import type { Route } from "../api/types";
import { buildMap, counts, visible } from "./graph";
import { evidenceShort, lifecycleWord, pretoolWords, signed, usd, verdictWord } from "./words";

const base = {
  id: "r",
  track: "acme/learned",
  version: "0.1.0",
  reference: "acme/learned",
  repo: "acme",
  imported: false,
  in_index: true,
  effect: "observe",
  authored: false,
  strength: "prime",
  ceiling: "fire",
  trigger: { kind: "prompt", label: "a prompt" },
  step: { kind: "run", label: "make lint" },
  directive: "",
  evidence: { support: 9, opportunities: 10, rate: 0.9, lower_bound: 0.6, turns_saved: 0 },
};
const uplift = { kind: "run", p_follow: 0.95, p_self: 0.9, mean: 0.05, lower: -0.02, gated: true };
const card = {
  sessions: 4,
  passed: 3,
  files: [{ path: "flags/flag_<x>.go", sessions: 4 }],
  commands: [{ command: "go test ./flags", sessions: 4, runs: 5, failed: 1, verified: true }],
  avoid: [],
  finish: "go vet ./...",
  threshold: 0.811,
  grey_from: 0.711,
  shared: false,
};

describe("quiet reflexes", () => {
  it("are quiet when the binary says so, or (older binaries) when a nascent reflex's uplift was gated", () => {
    expect(normalizeRoute({ ...base, state: "nascent", quiet: true }).quiet).toBe(true);
    expect(normalizeRoute({ ...base, state: "nascent", evidence: { ...base.evidence, uplift } }).quiet).toBe(true);
    expect(normalizeRoute({ ...base, state: "draft", review: { verbs: [], reasons: [{ kind: "quiet", why: "…" }] } }).quiet).toBe(true);
    expect(normalizeRoute({ ...base, state: "firm", evidence: { ...base.evidence, uplift } }).quiet).toBe(false);
    expect(normalizeRoute({ ...base, state: "nascent" }).quiet).toBe(false);
    expect(lifecycleWord(normalizeRoute({ ...base, state: "nascent", quiet: true }))).toBe("quiet");
  });

  it("are hidden by default, shown on their own chip, and not counted as nascent", () => {
    const r = normalizeRoute({ ...base, state: "nascent", quiet: true });
    const f = { repo: null, strengths: new Set(["fire", "prime", "inhibit"] as const), query: "" };
    expect(visible(r, { ...f, show: new Set() })).toBe(false);
    expect(visible(r, { ...f, show: new Set(["nascent"]) })).toBe(false);
    expect(visible(r, { ...f, show: new Set(["quiet"]) })).toBe(true);
    const g = normalizeGraph({ routes: [r, { ...base, id: "n", state: "nascent" }] });
    const c = counts(g, null);
    expect(c.lifecycles.get("quiet")).toBe(1);
    expect(c.lifecycles.get("nascent")).toBe(1);
    const m = buildMap(g, { mode: "aggregated", filters: { ...f, show: new Set(["quiet"]) } });
    const node = m.nodes.find((n) => n.kind === "reflex");
    expect(node?.quiet).toBe(true);
    expect(node?.tag).toBe("REFLEX · QUIET");
    expect(node?.sub).toContain("quiet");
  });

  it("say uplift with its sign, never −0.00", () => {
    expect(signed(0.05)).toBe("+0.05");
    expect(signed(-0.02)).toBe("−0.02");
    expect(signed(-0.001)).toBe("+0.00");
  });
});

describe("procedure cards", () => {
  const r = normalizeRoute({ ...base, state: "firm", step: { kind: "card", label: "procedure card: 1 file · from 4 traces", card } });
  it("read by their family, not a follow count", () => {
    expect(evidenceShort(r)).toBe("3/4 passed");
  });
  it("can be turned off on the map", () => {
    const f = { repo: null, strengths: new Set(["fire", "prime", "inhibit"] as const), query: "", show: new Set<never>() };
    expect(visible(r, f)).toBe(true);
    expect(visible(r, { ...f, cards: false })).toBe(false);
    expect(counts(normalizeGraph({ routes: [r] }), null).cards).toBe(1);
  });
});

describe("value-based evidence", () => {
  it("shows calls saved, not a 0/7 follow rate", () => {
    const r = normalizeRoute({
      ...base,
      state: "firm",
      evidence: { ...base.evidence, support: 0, opportunities: 7, basis: "value", why: { shape: "run", pattern: "p", gain: 1.25, lift: 0, conf: 0.93, conf_lb: 0.4, support: 0, days: 0 } },
    }) as Route;
    expect(evidenceShort(r)).toBe("~1.25 calls");
  });
});

describe("rewrites and savings", () => {
  it("keep PreToolUse records in the timeline, in words", () => {
    const e = normalizeEvent({ ts: 1, session: "s", arm: "holdout", event: "PreToolUse", repo: "acme", kind: "pretool", withheld: true, pretool: { rewrite: "test", interpreter: ".venv/bin/python" } });
    expect(e.kind).toBe("pretool");
    expect(e.arm).toBe("control");
    expect(e.directed).toEqual([]);
    expect(pretoolWords(e.pretool, false)).toBe("pointed this test run at .venv/bin/python, the project's environment");
    expect(pretoolWords(e.pretool, true)).toContain("control");
    expect(pretoolWords({ advise: "pip-install" }, false)).toBe("advised on pip install into the system Python");
  });

  it("read older graphs with neither, and fill a summary's gaps", () => {
    const old = normalizeGraph({});
    expect(old.savings).toEqual([]);
    expect(old.pretool).toEqual([]);
    const g = normalizeGraph({ savings: [{ repo: "acme", verdict: "measuring", n_control: 4 }] });
    expect(g.savings[0]?.need).toEqual({ control: 10, treated: 30 });
    expect(g.savings[0]?.counts.rewritten).toBe(0);
    expect(g.savings[0]?.gate_met).toBe(false);
  });

  it("word the verdict and dollars as the CLI does", () => {
    expect(verdictWord("adds_cost")).toBe("adds cost");
    expect(verdictWord("no_difference")).toBe("no measurable difference yet");
    expect(verdictWord("anything")).toBe("measuring");
    expect(usd(0.0341)).toBe("$0.034");
    expect(usd(1.2)).toBe("$1.20");
    expect(usd(-140.2)).toBe("$140");
  });
});
