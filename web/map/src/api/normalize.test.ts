import { describe, expect, it } from "vitest";
import { normalizeEvent, normalizeGraph, normalizeLearnRun } from "./normalize";

const arm = (followed: number) => ({ sessions: 4, applied: 3, followed, met: 0, hit_avoided: 0 });
const route = (o: Record<string, unknown>) => ({
  id: "r",
  track: "acme/learned",
  version: "0.1.0",
  reference: "acme/learned",
  repo: "acme",
  imported: false,
  in_index: true,
  effect: "observe",
  authored: false,
  trigger: { kind: "prompt", label: "a prompt" },
  step: { kind: "run", label: "make test" },
  origin: { kind: "learned", label: null },
  directive: "run make test",
  ...o,
});

describe("reading the graph JSON", () => {
  it("reads the current words as they are", () => {
    const g = normalizeGraph({
      generation: "g",
      home: "~",
      sessions: { total: 5, treated: 4, control: 1 },
      stop_requests: 0,
      routes: [
        route({
          strength: "fire",
          ceiling: "fire",
          state: "dormant",
          evidence: { support: 3, opportunities: 4, rate: 0.75, lower_bound: 0.3, turns_saved: 1, dormant_since: "2026-09-01" },
          live: { emitted: 2, would_have: 1, followed: 1, ignored: 1, misfired: 0, obligation_met: 0, control: { unaided: 1, not_reached: 0, obligation_met: 0 }, sessions: 3 },
          arms: { treated: arm(2), control: arm(1) },
        }),
      ],
      observed: [],
      events: [{ ts: 1, session: "s", arm: "control", event: "Stop", repo: "acme", directed: [], would: ["r"], judged: [["ignored", "r"]], observed: [], continue: false }],
    });
    const r = g.routes[0]!;
    expect([r.strength, r.state, r.evidence.dormant_since, r.live.ignored, r.live.control.unaided, r.arms.control?.followed]).toEqual(["fire", "dormant", "2026-09-01", 1, 1, 1]);
    expect(g.sessions.control).toBe(1);
    expect(g.events[0]!.arm).toBe("control");
    expect(g.events[0]!.judged).toEqual([["ignored", "r"]]);
    expect([r.origin, r.directive]).toEqual([{ kind: "learned", label: null }, "run make test"]);
  });

  it("reads what older binaries wrote", () => {
    const g = normalizeGraph({
      generation: "g",
      home: "~",
      sessions: { total: 5, treated: 4, holdout: 1 },
      stop_requests: 0,
      routes: [
        route({
          aspect: "green",
          ceiling: "yellow",
          state: "stale",
          evidence: { support: 3, opportunities: 4, rate: 0.75, lower_bound: 0.3, turns_saved: 1, stale_since: "2026-08-01" },
          live: { emitted: 2, would_have: 1, followed: 1, derailed: 2, obligation_met: 0, holdout: { unaided: 1, not_reached: 3, obligation_met: 0 }, sessions: 3 },
          arms: { treated: arm(2), holdout: arm(1) },
        }),
        route({ id: "d", aspect: "yellow", state: "demoted", evidence: {}, live: {}, arms: {}, instinct: true, origin: "instinct U1 (from Makefile)", directive: undefined }),
      ],
      observed: [],
      events: [
        { ts: 1, session: "s", arm: "holdout", event: "PostToolBatch", repo: "acme", directed: [], would: ["r"], judged: [["derailed", "r"], ["followed_failed", "d"]], observed: [], continue: false },
      ],
      learning: [{ ts: 2, kind: "learn", changes: [{ route: "r", change: "aspect", from: "yellow", to: "green" }, { route: "d", change: "draft", from: null, to: "yellow" }] }],
    });
    const [r, d] = g.routes;
    expect([r!.strength, r!.ceiling, r!.state, r!.evidence.dormant_since]).toEqual(["fire", "prime", "dormant", "2026-08-01"]);
    expect([r!.live.ignored, r!.live.control.not_reached, r!.arms.control?.followed]).toEqual([2, 3, 1]);
    // There is no weakened state: a demoted route was still delivered.
    expect([d!.strength, d!.state]).toEqual(["prime", "firm"]);
    // The origin was a label string, and a missing directive reads as none.
    expect([d!.origin, d!.directive]).toEqual([{ kind: "instinct", label: "instinct U1 (from Makefile)" }, null]);
    expect(g.sessions.control).toBe(1);
    expect(g.events[0]!.arm).toBe("control");
    expect(g.events[0]!.judged).toEqual([
      ["ignored", "r"],
      ["misfired", "d"],
    ]);
    const run = g.learning![0]!;
    expect(run.kind).toBe("condition");
    expect(run.changes!.map((c) => [c.change, c.from, c.to])).toEqual([
      ["strength", "prime", "fire"],
      ["nascent", null, "prime"],
    ]);
  });

  it("reads streamed events and learn runs the same way", () => {
    expect(normalizeEvent({ ts: 1, session: "s", arm: "holdout", event: "Stop", repo: "r", judged: [] }).arm).toBe("control");
    expect(normalizeLearnRun({ ts: 1, kind: "learn_error" }).kind).toBe("condition_error");
  });
});

it("keeps absent conditioning counts unknown and separate from recorded sessions", () => {
  const g = normalizeGraph({ sessions: { total: 54 }, learning_history: {
    source: "recent_conditioning_runs", repositories: [{ repo: "acme", last_conditioned_at: 12, files_read: 144, sessions_used: 120, sessions_steered: null }],
  } });
  expect(g.sessions.total).toBe(54);
  expect(g.learning_history?.repositories[0]).toMatchObject({ files_read: 144, sessions_used: 120, sessions_steered: null, sessions_duplicates: null });
  expect(normalizeGraph({}).learning_history).toBeUndefined();
});
