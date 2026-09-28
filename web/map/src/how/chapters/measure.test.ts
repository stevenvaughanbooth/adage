import { describe, expect, it } from "vitest";
import { CORPUS, TASKS, holm, median, permTest, rng, signFlip, taskMeanDiff } from "./measure";

describe("the measure chapter's statistics", () => {
  it("adjusts p-values as Holm's step-down does (demo/report.py holm)", () => {
    // Sorted 0.01, 0.02, 0.04 → ×3, ×2, ×1 = 0.03, 0.04, 0.04; order kept; capped at 1.
    const out = holm([0.04, 0.01, 0.02]);
    expect(out[0]).toBeCloseTo(0.04, 12);
    expect(out[1]).toBeCloseTo(0.03, 12);
    expect(out[2]).toBeCloseTo(0.04, 12);
    expect(holm([0.5, 0.6])).toEqual([1, 1]);
  });

  it("flips signs exactly: the smallest p with k tasks is 2/2^k", () => {
    const all: [number[], number[]][] = Array.from({ length: 6 }, () => [[1], [5]]);
    expect(signFlip(all)).toBeCloseTo(2 / 64, 12);
    const none: [number[], number[]][] = [
      [[1], [2]],
      [[2], [1]],
    ];
    expect(signFlip(none)).toBe(1);
  });

  it("permutes within tasks, deterministically", () => {
    const g = TASKS.map((t) => [t.treated.map((r) => r.turns), t.control.map((r) => r.turns)] as [number[], number[]]);
    const a = permTest(g, 2000);
    const b = permTest(g, 2000);
    expect(a.p).toBe(b.p);
    expect(a.obs).toBeCloseTo(taskMeanDiff(g), 12);
    // Shuffling within a task keeps each task's values.
    a.first.forEach(([x, y], t) => expect([...x, ...y].sort()).toEqual([...g[t]![0], ...g[t]![1]].sort()));
    // A null with no effect: p near 1 for identical arms.
    const flat: [number[], number[]][] = g.map(([x]) => [x, [...x]]);
    expect(permTest(flat, 500).p).toBeGreaterThan(0.9);
  });

  it("draws uniforms and medians", () => {
    const r = rng(7);
    const xs = Array.from({ length: 1000 }, r);
    expect(Math.min(...xs)).toBeGreaterThanOrEqual(0);
    expect(Math.max(...xs)).toBeLessThan(1);
    expect(median([3, 1, 2])).toBe(2);
    expect(median([4, 1, 2, 3])).toBe(2.5);
  });

  it("adds up to the corpus's 354 sessions", () => {
    expect(CORPUS.reduce((a, c) => a + c.sessions, 0)).toBe(354);
    expect(CORPUS.filter((c) => c.learned && !c.sonnet).length).toBe(4);
  });
});
