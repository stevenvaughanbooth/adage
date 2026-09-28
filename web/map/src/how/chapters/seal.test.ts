import { webcrypto } from "node:crypto";
import { describe, expect, it } from "vitest";
import { compileSample, seal, sha256hex } from "./seal";

const stage = { dot() {}, link() {}, label: () => ({ remove() {} }) } as never;

describe("seal the index", () => {
  it("hashes as SubtleCrypto does", async () => {
    for (const s of ["", "abc", "x".repeat(1000), "ünïcode → ✓", compileSample().body]) {
      const d = new Uint8Array(await webcrypto.subtle.digest("SHA-256", new TextEncoder().encode(s)));
      expect(sha256hex(s)).toBe([...d].map((b) => b.toString(16).padStart(2, "0")).join(""));
    }
  });

  it("resolves the sample as Index::compile and the interlock do", () => {
    const c = compileSample();
    // Two duplicates dropped: the imported dead end and the instinct finish line.
    expect(c.dropped.map((d) => [d.lost.key, d.to.key]).sort()).toEqual([
      ["B1", "A3"],
      ["I1", "A1"],
    ]);
    expect(c.out).toHaveLength(7);
    expect(c.locked.map((l) => [l.by.key, l.route.key])).toEqual([["A3", "B2"]]);
    expect(c.demoted.map((d) => [d.winner.key, d.loser.key])).toEqual([["A1", "A2"]]);
    expect(c.out.find((r) => r.key === "A2")!.aspect).toBe("prime");
    expect(c.generation).toMatch(/^[0-9a-f]{16}$/);
  });

  it("states this repository's generation, not a recomputed one", () => {
    const route = { id: "x", track: "a/b", version: "0.1.0", imported: false, in_index: true, aspect: "green" } as never;
    const built = seal.build(stage, { graph: { generation: "0123456789abcdef", routes: [route] } as never, repo: true });
    expect(built.source).toBe("your 1 reflexes");
    expect(built.steps.at(-1)!.body).toContain("0123456789abcdef");
  });
});
