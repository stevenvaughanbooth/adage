import { describe, expect, it } from "vitest";
import { betaGrid, wilson } from "./sample";

describe("the statistics How it works shows", () => {
  it("computes the Wilson lower bound as the learner does", () => {
    // evals/datasets/demo-haiku45/recorded/index-round2.json: lookup-work-item, 23/33, LB 0.527.
    expect(wilson(23, 33)).toBeCloseTo(0.527, 3);
    // read-linked-files, 15/16, LB 0.717.
    expect(wilson(15, 16)).toBeCloseTo(0.717, 3);
    expect(wilson(0, 0)).toBe(0);
  });

  it("finds a Beta posterior's mean and 5% quantile", () => {
    const b = betaGrid(8, 2);
    expect(b.mean).toBeCloseTo(0.8, 5);
    expect(b.q05).toBeGreaterThan(0.5);
    expect(b.q05).toBeLessThan(b.mean);
  });
});
