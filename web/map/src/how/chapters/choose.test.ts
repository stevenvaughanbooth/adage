import { describe, expect, test } from "vitest";
import { apply, run, shapeOf, CANDS } from "./choose";

describe("the sample tells the chapter's story", () => {
  const r = run();
  const by = new Map(r.scored.map((s) => [s.cand.key, s]));
  const comp = new Map(r.compiled.map((c) => [c.sc.cand.key, c]));

  test("the gates keep six and stop git status (contrast) and curl (support)", () => {
    expect(r.scored.filter((s) => s.passes).map((s) => s.cand.key).sort()).toEqual(["avoid", "ctx", "npm", "obl", "seq", "seq2"]);
    expect(by.get("status")!.gate).toMatch(/^contrast/);
    expect(by.get("curl")!.gate).toMatch(/^support/);
  });

  test("shapes are structural", () => {
    expect(CANDS.map((c) => shapeOf(c))).toEqual(["sequence", "obligation", "run", "sequence", "avoid", "run", "run", "run"]);
  });

  test("the calibration map is monotone", () => {
    const ys = r.cal.points.map((p) => p[1]);
    expect(ys.every((y, i) => i === 0 || y >= ys[i - 1]!)).toBe(true);
    expect(apply(r.cal, 0.9)).toBeGreaterThanOrEqual(apply(r.cal, 0.6));
  });

  test("the longer sequence saves no bits once the shorter one is in its table", () => {
    const seq = r.tables.find((t) => t.shape === "sequence")!;
    const e = new Map(seq.entries.map((x) => [x.sc.cand.key, x]));
    expect(e.get("seq")!.kept).toBe(true);
    expect(e.get("seq2")!.kept).toBe(false);
    expect(e.get("seq2")!.bound).toBeGreaterThan(0);
  });

  test("strengths: one fire, provisional primes, an inhibiting avoid, an interlocked run", () => {
    expect(comp.get("ctx")!.lamp).toBe("fire");
    expect(comp.get("seq")!.lamp).toBe("prime");
    expect(comp.get("obl")!.lamp).toBe("prime");
    expect(comp.get("avoid")!.lamp).toBe("inhibit");
    expect(comp.get("avoid")!.atStart).toBe(true);
    expect(comp.get("npm")!.dropped).toMatch(/avoid/);
    expect(comp.get("seq2")!.dropped).toMatch(/context/);
  });
});
