import { describe, expect, test } from "vitest";
import { normalize, run } from "./intent";

describe("the normaliser matches crates/adage-intent/src/normalize.rs", () => {
  test("spans become placeholders", () => {
    expect(normalize("pick up W-4821").text).toBe("pick up {ticket}");
    expect(normalize("pick up ENG-4821").text).toBe("pick up {ticket}");
    expect(normalize("fix src/auth/token.py please").text).toBe("fix {path} please");
    expect(normalize("review #482 at https://x.dev/pr/1").text).toBe("review {pr} at {url}");
    expect(normalize("bump react to 18.3.1").text).toBe("bump react to {version}");
    expect(normalize("set DATABASE_URL now").text).toBe("set {env_var} now");
    expect(normalize("revert a3f9c21").text).toBe("revert {sha}");
  });

  test("code is stripped and the head kept", () => {
    const n = normalize("why?\n```\nTraceback\n  boom\n```\nthanks");
    expect(n.fence).toBe(true);
    expect(n.text).toBe("why? [code] thanks");
    const long = Array.from({ length: 500 }, (_, i) => `w${i}`).join(" ");
    expect(normalize(long).text.split(" ")).toHaveLength(128);
  });
});

describe("the sample tells the chapter's story", () => {
  const r = run();

  test("one family splits by route and two merge", () => {
    expect(r.splitNotes).toHaveLength(1);
    expect(r.splitNotes[0]!.sep).toBeGreaterThanOrEqual(0.85);
    expect(r.splitNotes[0]!.groups.map((g) => g.route).sort()).toEqual(["go list -m -u all", "pnpm outdated"]);
    expect(r.mergeNotes).toHaveLength(1);
    expect(r.mergeNotes[0]!.route).toBe("git log --oneline");
    expect(r.mergeNotes[0]!.cos).toBeGreaterThanOrEqual(0.8);
  });

  test("git status is ubiquitous and skipped", () => {
    expect(r.ubiquitous.map((u) => u.cmd)).toEqual(["git status"]);
    expect(r.routes).not.toContain("git status");
  });

  test("the incident family misses the bar; the test family is calibrated below its outsider", () => {
    const dropped = r.families.filter((f) => !f.kept);
    expect(dropped).toHaveLength(1);
    expect(dropped[0]!.share).toBeLessThan(0.6);
    const fam = r.families.find((f) => f.route === "make test")!;
    expect(fam.reflex).toBe(true);
    const last = fam.curve[fam.curve.length - 1]!;
    expect(last.sup).toBeLessThan(last.opp);
    expect(last.lb).toBeLessThan(fam.lb);
    expect(fam.t).toBeGreaterThanOrEqual(0.5);
  });

  test("the new prompt lands inside the test family's threshold", () => {
    expect(r.match?.f.route).toBe("make test");
    expect(r.match!.s).toBeGreaterThanOrEqual(r.match!.f.t);
  });
});
