import { describe, expect, it } from "vitest";
import type { Graph } from "../../api/types";
import { TRACES, wilson } from "../sample";
import { _recorded, ceiling, deadEnd, finishLine, judge, strengthFor, uplift } from "./judge";

// A stage that draws nothing: the chapter's words and states are what the tests read.
const quiet = { dot() {}, link() {}, label: () => ({}) } as never;

describe("judge the evidence", () => {
  it("caps strength by effect, as model.rs does", () => {
    expect(ceiling("author", false)).toBeNull();
    expect(ceiling("publish", true)).toBeNull();
    expect(ceiling("environment", true)).toBe("prime");
    expect(ceiling("regenerate", false)).toBe("prime");
    expect(ceiling("regenerate", true)).toBe("fire");
    expect(strengthFor("environment", false, 0.99, 1, 0.8)).toBe("prime");
    expect(strengthFor("observe", false, 0.4, 0.45, 0.8)).toBeNull();
    // An existing reflex keeps its place down to the lower rate (hysteresis).
    expect(strengthFor("observe", false, 0.4, 0.45, 0.8, true)).toBe("prime");
  });

  it("reproduces the recorded run's lower bounds", () => {
    for (const r of _recorded) expect(wilson(r.support, r.opportunities)).toBeCloseTo(r.lower_bound, 9);
  });

  it("mines the sample finish line and dead end", () => {
    const fl = finishLine(TRACES, "E", "make gen");
    expect([fl.support.length, fl.opp, fl.ranWithout, fl.without]).toEqual([6, 8, 0, 4]);
    expect(strengthFor("regenerate", true, fl.lb, fl.rate, 0.8)).toBe("prime");
    const de = deadEnd(TRACES);
    expect([de.support, de.failedIn, de.ran, de.consistent]).toEqual([2, 2, 2, true]);
  });

  it("estimates uplift as value.rs does, and gates only on the repository's own evidence", () => {
    // No evidence of its own: the kind's priors alone (obligation 0.975 − 0.88), never gated.
    const prior = uplift("obligation", { traces: 0, took: 0 });
    expect(prior.mean).toBeCloseTo(0.975 - 0.88, 9);
    expect(prior.gated).toBe(false);
    // A read batch agents always do unaided: quiet once three situations say so.
    expect(uplift("read-batch", { traces: 3, took: 3 }).gated).toBe(true);
    expect(uplift("read-batch", { traces: 2, took: 2 }).gated).toBe(false);
    // The demo preset turns the gate off.
    expect(uplift("read-batch", { traces: 10, took: 10 }, 0).gated).toBe(false);
    // An avoid agents rarely find unaided: delivered.
    expect(uplift("avoid", { traces: 10, took: 1 }).gated).toBe(false);
  });

  it("uses this repository's reflexes when asked", () => {
    const route = (id: string, strength: string, support: number, opportunities: number) => ({
      id,
      strength,
      effect: "observe",
      step: { kind: "run", label: id },
      evidence: { support, opportunities, rate: support / opportunities, lower_bound: wilson(support, opportunities), turns_saved: 0 },
    });
    const graph = { routes: [route("a", "fire", 9, 10), route("b", "prime", 5, 8)], bars: { min_support: 5, fire_lower_bound: 0.8 } } as unknown as Graph;
    const built = judge.build(quiet, { graph, repo: true });
    expect(built.source).toContain("your 2 reflexes");
    expect(built.steps.at(-1)!.title).toBe("Your reflexes against the bar");
    expect(judge.build(quiet, { graph: null, repo: false }).steps.at(-1)!.title).toBe("A recorded index");
  });
});
