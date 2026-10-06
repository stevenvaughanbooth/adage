import { describe, expect, it } from "vitest";
import {
  normalizeEvent,
  normalizeGraph,
  normalizeRoute,
} from "../api/normalize";
import { findRoute, firstFired } from "./graph";
import { exactRouteKey, measurement, relates, traceInventory } from "./traces";
import { parse, serialize } from "../state/url";

const route = normalizeRoute({
  id: "test",
  repo: "repo",
  reference: "local/test",
  version: "2",
  track: "test",
  step: { kind: "run", label: "cargo test" },
});
describe("recorded trace identity", () => {
  it("does not attach old or duplicated ids to an arbitrary installed reflex", () => {
    const copy = { ...route, reference: "pack/test" };
    const g = normalizeGraph({ routes: [route, copy] });
    expect(findRoute(g, "test")).toBeUndefined();
    expect(findRoute(g, exactRouteKey(copy))).toEqual(copy);
    const e = normalizeEvent({
      repo: "repo",
      directed: ["test"],
      directives: [
        { route: "test", version: "1", route_key: exactRouteKey(route) },
      ],
    });
    expect(relates(e, route, "1")).toBe(true);
    expect(relates(e, copy)).toBe(false);
    expect(relates(e, route, "2")).toBe(false);
    expect(
      relates(normalizeEvent({ repo: "repo", directed: ["test"] }), route),
    ).toBe(false);
  });
  it("replay does not reuse an id's first exposure for another version or copy", () => {
    const current = exactRouteKey(route);
    const e = normalizeEvent({ ts: 42, repo: route.repo, directed: [route.id], directives: [{ route: route.id, version: "1", route_key: current }] });
    const fired = firstFired([e]);
    expect(fired.get(JSON.stringify([current, "1"]))).toBe(42);
    expect(fired.get(JSON.stringify([current, "2"]))).toBeUndefined();
    expect(firstFired([normalizeEvent({ ts: 1, repo: route.repo, directed: [route.id] })]).size).toBe(0);
  });
  it("keeps older sessions with no events in the recent window", () => {
    const old = {
      repo: "repo",
      session: "old",
      arm: "treated",
      events_total: 620,
      events_visible: 0,
    };
    expect(
      traceInventory(normalizeGraph({ events: [], trace_sessions: [old] })),
    ).toEqual([old]);
  });
  it("round trips a complete trace and selected sequence without losing special characters", () => {
    const selected = parse(
      "?view=traces&session=old&traceRepo=repo%2Fa%2Bb&seq=511&reflex=local%2Ftest%3A%3Atest&version=1",
    );
    expect(parse(serialize(selected))).toEqual(selected);
    expect(selected.traceRepo).toBe("repo/a+b");
    expect(parse("?seq=-1").sequence).toBeNull();
  });
  it("keeps unavailable historical avoidance counts distinct from zero", () => {
    const r = normalizeRoute({
      ...route,
      arms: { treated: { applied: 2, hit_avoided: null } },
      measurement_history: [
        { version: "1", arms: { treated: { applied: 2, hit_avoided: null } } },
      ],
    });
    expect(r.arms.treated?.hit_avoided).toBeNull();
    expect(measurement(r, "1").arms.treated?.hit_avoided).toBeNull();
  });
});
