import { describe, expect, it } from "vitest";
import { _data, readTraces, subActionFailed } from "./read";

// A stage that draws nothing: the chapter's words and states are what the tests read.
const quiet = { dot() {}, link() {}, label: () => ({}) } as never;

describe("read traces", () => {
  it("blames a failure on the last part, or on every part after ||", () => {
    expect([0, 1].map((i) => subActionFailed(i, 2, true, false))).toEqual([false, true]);
    expect([0, 1].map((i) => subActionFailed(i, 2, true, true))).toEqual([true, true]);
    expect(subActionFailed(0, 1, false, true)).toBe(false);
  });

  it("replays the recorded trace consistently", () => {
    const results = _data.RECORDS.filter((r) => r.t === "R");
    for (const u of _data.RECORDS) {
      if (u.t !== "U") continue;
      expect(_data.STEPS[u.id]?.length).toBeGreaterThan(0);
      expect(results.filter((r) => r.t === "R" && r.id === u.id)).toHaveLength(1);
    }
    // The build checks each recorded failure flag against the rule, and throws if one breaks it.
    const built = readTraces.build(quiet, { graph: null, repo: false });
    expect(built.steps.length).toBeGreaterThanOrEqual(4);
    expect(built.steps.length).toBeLessThanOrEqual(8);
    // What `read_session` returns for train-W-1041-r1.jsonl: 18 calls in 17 turns, 21 steps.
    expect(built.steps[1]!.body).toContain("make 17 turns");
    expect(built.steps[3]!.body).toContain("18 calls become 21 steps");
  });
});
