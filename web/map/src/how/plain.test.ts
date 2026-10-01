import { describe, expect, it } from "vitest";
import { CHAPTERS } from "./chapters";
import { chipMark, chipText } from "./chip";
import { TERMS, segments } from "./glossary";
import type { MathId } from "./types";

// A stage that draws nothing: the chapter's words are what these tests read.
const quiet = { dot() {}, link() {}, label: () => ({}), clear() {}, to() {} } as never;
const MATH: MathId[] = ["read", "graph", "evidence", "uplift", "intent", "cards", "train", "seal", "hook", "measure"];
const words = (s: string) => s.trim().split(/\s+/).length;
const built = CHAPTERS.map((c) => ({ c, steps: c.build(quiet, { graph: null, repo: false }).steps }));

describe("the plain layer", () => {
  it("names every chapter in plain words, with its technical name beside it", () => {
    for (const c of CHAPTERS) {
      expect(c.title, c.id).not.toBe(c.technical);
      expect(c.technical.length, c.id).toBeGreaterThan(0);
      expect(words(c.summary), c.id).toBeLessThanOrEqual(40);
    }
  });

  it("gives every step a short plain account, apart from its precise one", () => {
    for (const { c, steps } of built)
      for (const s of steps) {
        const at = `${c.id}: ${s.title}`;
        expect(s.plain.length, at).toBeGreaterThan(20);
        expect(words(s.plain), at).toBeLessThanOrEqual(50);
        expect(s.plain, at).not.toBe(s.body);
        expect(s.plain, at).toMatch(/[.!?]$/);
        if (s.math) expect(MATH, at).toContain(s.math);
      }
  });

  it("stops the quick tour at one to three key steps in every chapter", () => {
    let total = 0;
    for (const { c, steps } of built) {
      const keys = steps.filter((s) => s.key).length;
      expect(keys, c.id).toBeGreaterThanOrEqual(1);
      expect(keys, c.id).toBeLessThanOrEqual(3);
      total += keys;
    }
    expect(total).toBeLessThanOrEqual(25);
  });

  it("links every block of the math from at least one step", () => {
    const linked = new Set(built.flatMap(({ steps }) => steps.map((s) => s.math)));
    for (const m of MATH) expect(linked, m).toContain(m);
  });
});

describe("the glossary", () => {
  it("defines each term once, in a sentence", () => {
    const forms = TERMS.flatMap((t) => t.forms.map((f) => f.toLowerCase()));
    expect(new Set(forms).size).toBe(forms.length);
    for (const t of TERMS) expect(t.def, t.id).toMatch(/^[A-Z].*[.!?"]$/);
  });

  it("marks the first mention of a term only, whole words, the longest form first", () => {
    const seen = new Set<string>();
    const a = segments("A reflex fires past the fire bar; reflexes then fire.", seen);
    const marked = a.filter((p) => typeof p !== "string").map((p) => (p as { text: string }).text);
    expect(marked).toEqual(["reflex", "fires", "fire bar"]);
    // Later text sharing the set marks nothing it already marked.
    expect(segments("Every reflex fires.", seen).every((p) => typeof p === "string")).toBe(true);
    // Inside a longer word nothing matches.
    expect(segments("prefire indexing tracker", new Set()).every((p) => typeof p === "string")).toBe(true);
  });

  it("keeps the text intact around the marks", () => {
    const text = "The Wilson lower bound of a dead end, in the index.";
    const joined = segments(text).map((p) => (typeof p === "string" ? p : p.text)).join("");
    expect(joined).toBe(text);
  });
});

describe("chips", () => {
  it("read a failed gate as failing, and mark passes and failures", () => {
    expect(chipText({ text: "support 2 ≥ 5", ok: false })).toBe("support 2 < 5");
    expect(chipText({ text: "support 6 ≥ 5", ok: true })).toBe("support 6 ≥ 5");
    expect(chipText({ text: "keep ≥ 0.30" })).toBe("keep ≥ 0.30");
    expect([chipMark({ text: "", ok: true }), chipMark({ text: "", ok: false }), chipMark({ text: "" })]).toEqual(["✓", "✗", null]);
  });
});

describe("the site's added terms", () => {
  it("define each form once across both lists, in a sentence, and mark through the segmenter", async () => {
    const { MORE_TERMS, segmenter } = await import("./glossary");
    const forms = [...TERMS, ...MORE_TERMS].flatMap((t) => t.forms.map((f) => f.toLowerCase()));
    expect(new Set(forms).size).toBe(forms.length);
    for (const t of MORE_TERMS) expect(t.def, t.id).toMatch(/^[A-Z].*[.!?"]$/);
    const split = segmenter([...TERMS, ...MORE_TERMS]);
    const marked = split("Held-out tasks: fewer model requests per reflex, with hidden tests.")
      .filter((p) => typeof p !== "string").map((p) => (p as { text: string }).text);
    expect(marked).toEqual(["Held-out", "model requests", "reflex", "hidden tests"]);
  });
});
