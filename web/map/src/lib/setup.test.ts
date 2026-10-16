import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { Graph } from "../api/types";
import { actionWords, paletteHint, setupCopy, versionText } from "./setup";

const graph = (adage?: Graph["adage"]) => ({ adage }) as Graph;
const words = (g: Graph | null) => {
  const c = setupCopy(g);
  return [c.lead, c.intro, ...c.steps.flatMap((s) => [s.title, s.action, s.detail]), c.review("verify"), paletteHint(g), ...Object.values(actionWords(g))].join("\n");
};

// T-O10-02: the Setup view by channel.
describe("setupCopy", () => {
  it("tells a plugin user what to do in Claude Code, never to open a terminal", () => {
    const g = graph({ version: "0.1.0", channel: "plugin" });
    const all = words(g);
    expect(setupCopy(g).lead).toBe("In Claude Code: /adage review, /adage health; the pane's Explore in browser opened this page.");
    expect(all).not.toMatch(/adage ui/);
    expect(all).not.toMatch(/in your terminal/);
    expect(all).not.toMatch(/start adage/);
    for (const s of setupCopy(g).steps) expect(s.action.startsWith("/adage")).toBe(true);
  });

  it("names the commands for anyone else, and for a binary that does not say", () => {
    for (const g of [graph({ version: "0.1.0", channel: "npm" }), graph(undefined), null]) {
      const c = setupCopy(g);
      expect(c.lead).toBe("In a terminal: adage ui, adage review, adage update.");
      expect(c.steps.map((s) => s.action)).toEqual(["adage init", "adage ui", "adage review", "adage update"]);
      expect(words(g)).not.toMatch(/once in your terminal/);
    }
  });
});

describe("versionText", () => {
  it("shows the version, and a newer release with the command that brings it", () => {
    expect(versionText(graph({ version: "0.1.0", latest: null, channel: "source" }))).toEqual({ version: "adage 0.1.0", update: null });
    expect(versionText(graph({ version: "0.1.0", latest: "0.2.0", channel: "npm" }))?.update).toBe("0.2.0 is out · adage update");
    expect(versionText(graph({ version: "0.1.0", latest: "0.2.0", channel: "plugin" }))?.update).toBe("0.2.0 is out · /adage update");
    expect(versionText(graph(undefined))).toBeNull();
  });
});

// T-O10-04: `adage map`'s default port and the demo dashboard's differ, so both run at once.
describe("ports", () => {
  it("keeps the demo dashboard off adage map's default port", () => {
    const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
    const map = Number(/const DEFAULT_PORT: u16 = (\d+);/.exec(read("../../../../crates/adage-cli/src/map/mod.rs"))?.[1]);
    const demo = Number(/DEMO_PORT", "(\d+)"/.exec(read("../../../../demo/dashboard/serve.py"))?.[1]);
    expect(map).toBe(8765);
    expect(demo).toBe(8766);
  });
});
