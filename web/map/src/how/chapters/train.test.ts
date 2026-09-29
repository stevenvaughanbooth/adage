import { describe, expect, it } from "vitest";
import { _internal, stable, train } from "./train";

describe("train on a skill", () => {
  it("derives route ids as the compiler does", () => {
    // adage-instincts safety.rs `ids_match_the_learner`; docs/training.md (acme fixture).
    expect(`dead-end-${stable(["exec make test"])}`).toBe("dead-end-d41b1e");
    expect(`finish-line-${stable(["schema/*.json", "make gen"])}`).toBe("finish-line-e7e809");
  });

  it("plans tasks by the rules of tasks.rs", () => {
    const tasks = _internal.plan();
    const controls = tasks.filter((t) => !t.claim).length;
    expect(tasks).toHaveLength(24);
    expect(controls * 4).toBeGreaterThanOrEqual(tasks.length);
    const triggers = tasks.length - controls;
    expect(tasks.filter((t) => t.explicit)).toHaveLength(Math.floor((triggers * 3 + 5) / 10));
    // Unsafe and unmatched claims get no tasks.
    expect(tasks.some((t) => t.claim?.id === "c4" || t.claim?.id === "c5")).toBe(false);
  });

  it("states one claim of each verdict", () => {
    const stage = { dot() {}, link() {}, label: () => ({ remove() {} }) } as never;
    const built = train.build(stage, { graph: null, repo: false });
    expect(built.steps.length).toBeGreaterThanOrEqual(4);
    expect(built.steps.length).toBeLessThanOrEqual(8);
    const verdicts = built.steps[6]!.chips.map((c) => c.text).join(" ");
    expect(verdicts).toContain("1 compiled · 1 contradicted · 1 never exercised · 1 not compiled · 1 unsafe");
  });
});
