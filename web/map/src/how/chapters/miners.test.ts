import { describe, expect, it } from "vitest";
import type { Graph } from "../../api/types";
import { TRACES, wilson } from "../sample";
import { _recorded, aspectFor, ceiling, deadEnd, finishLine, miners } from "./miners";

// A stage that draws nothing: the chapter's words and states are what the tests read.
const quiet = { dot() {}, link() {}, label: () => ({}) } as never;

describe("condition with miners", () => {
  it("caps strength by effect, as model.rs does", () => {
    expect(ceiling("author", false)).toBeNull();
    expect(ceiling("publish", true)).toBeNull();
    expect(ceiling("environment", true)).toBe("prime");
    expect(ceiling("regenerate", false)).toBe("prime");
    expect(ceiling("regenerate", true)).toBe("fire");
    expect(aspectFor("environment", false, 0.99, 1, 0.8)).toBe("prime");
    expect(aspectFor("observe", false, 0.4, 0.45, 0.8)).toBeNull();
    // An existing reflex keeps its place down to the lower rate (hysteresis).
    expect(aspectFor("observe", false, 0.4, 0.45, 0.8, true)).toBe("prime");
  });

  it("reproduces the recorded run's lower bounds", () => {
    for (const r of _recorded) expect(wilson(r.support, r.opportunities)).toBeCloseTo(r.lower_bound, 9);
  });

  it("mines the sample finish line and dead end", () => {
    const fl = finishLine(TRACES, "E", "make gen");
    expect([fl.support.length, fl.opp, fl.ranWithout, fl.without]).toEqual([6, 8, 0, 4]);
    expect(aspectFor("regenerate", true, fl.lb, fl.rate, 0.8)).toBe("prime");
    const de = deadEnd(TRACES);
    expect([de.support, de.failedIn, de.ran, de.consistent]).toEqual([2, 2, 2, true]);
  });

  it("uses this repository's reflexes when asked", () => {
    const route = (id: string, aspect: string, support: number, opportunities: number) => ({
      id,
      aspect,
      effect: "observe",
      step: { kind: "run", label: id },
      evidence: { support, opportunities, rate: support / opportunities, lower_bound: wilson(support, opportunities), turns_saved: 0 },
    });
    const graph = { routes: [route("a", "green", 9, 10), route("b", "yellow", 5, 8)], bars: { min_support: 5, fire_lower_bound: 0.8 } } as unknown as Graph;
    const built = miners.build(quiet, { graph, repo: true });
    expect(built.source).toContain("your 2 reflexes");
    expect(built.steps.at(-1)!.title).toBe("Your reflexes against the bar");
    expect(miners.build(quiet, { graph: null, repo: false }).steps.at(-1)!.title).toBe("A recorded run");
  });
});
