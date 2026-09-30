import { describe, expect, it } from "vitest";
import type { Route } from "../api/types";
import { evidenceSource, prose } from "./words";

describe("prose", () => {
  it("reads stored summaries in the lexicon, as the binary does", () => {
    expect(prose("finish line route yellow → green (8 of 8 sessions, lower bound 0.50)")).toBe(
      "finish line reflex prime → fire (8 of 8 traces, lower bound 0.50)",
    );
    expect(prose("reduced credit")).toBe("reduced credit");
    expect(prose("3 spools in /home/me/sessions")).toBe("3 spools in /home/me/sessions");
    expect(prose("run `adage review promote` for 2 routes")).toBe("run `adage review promote` for 2 reflexes");
  });
});

describe("evidenceSource", () => {
  const r = (x: Partial<Route>) => ({ instinct: false, imported: false, ...x }) as Route;
  it("names where a reflex's evidence was counted", () => {
    expect(evidenceSource(r({}))).toBe("From conditioning");
    expect(evidenceSource(r({ imported: true }))).toBe("From the pack");
    expect(evidenceSource(r({ instinct: true }))).toBe("Shipped with Adage");
  });
});
