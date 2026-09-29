// Graph JSON → the map: stimulus → reflex → response, filtered, aggregated or expanded, focused.
// Pure functions, unit-tested in graph.test.ts.

import type { Graph, Route, TraceEvent } from "../api/types";
import {
  type Lifecycle,
  type Showable,
  type Strength,
  evidenceShort,
  isCard,
  isInstinct,
  isQuiet,
  lifecycle,
  responseKind,
  stimulusKind,
  strength,
} from "./words";

export type NodeKind = "stimulus" | "reflex" | "instinct" | "response";
export type Mode = "aggregated" | "expanded";

export interface MapNode {
  id: string;
  kind: NodeKind;
  /** Upper-case kind line: PROMPT, REFLEX, RUN INSTEAD… */
  tag: string;
  title: string;
  sub: string;
  /** Reflexes and instincts only. */
  strength?: Strength | null;
  lifecycle?: Lifecycle;
  /** The reflex this node is (reflex nodes) or the reflexes it belongs to. */
  routes: string[];
  /** The avoided command, struck through (dead ends). */
  strike?: boolean;
  /** Replay: not fired yet at the scrubbed time. */
  unformed?: boolean;
  /** Reflex nodes: kept quiet on its uplift; a procedure card. */
  quiet?: boolean;
  card?: boolean;
}

export interface MapEdge {
  id: string;
  source: string;
  target: string;
  /** The reflex whose arc this edge is part of. */
  route: string;
  strength?: Strength | null;
}

export interface Filters {
  repo: string | null;
  strengths: ReadonlySet<Strength>;
  /** Lifecycles hidden unless asked for: nascent, quiet, dormant, extinguished by default. */
  show: ReadonlySet<Showable>;
  query: string;
  /** Procedure cards on the map (shown unless turned off). */
  cards?: boolean;
}

export const HIDDEN_BY_DEFAULT: Showable[] = ["nascent", "quiet", "dormant", "extinguished"];

/** The filter bucket a reflex falls in: its lifecycle, or quiet. */
export function showKey(r: Route): Showable {
  return isQuiet(r) ? "quiet" : lifecycle(r.state);
}

/** A reflex's key: its track reference and id (ids repeat across tracks and repositories). */
export function routeKey(r: Pick<Route, "reference" | "id">): string {
  return `${r.reference}::${r.id}`;
}

export function findRoute(g: Graph | null | undefined, key: string | null): Route | undefined {
  if (!g || !key) return undefined;
  return g.routes.find((r) => routeKey(r) === key) ?? g.routes.find((r) => r.id === key);
}

export function repos(g: Graph): string[] {
  return [...new Set(g.routes.map((r) => r.repo).concat(g.events.map((e) => e.repo)).filter(Boolean))].sort();
}

function matches(r: Route, q: string): boolean {
  if (!q) return true;
  const hay = [r.id, r.summary ?? "", r.trigger.label, r.step.label, r.step.avoid ?? "", r.track, r.origin ?? ""]
    .join("\n")
    .toLowerCase();
  return q
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((w) => hay.includes(w));
}

/** Whether a reflex passes the filters. */
export function visible(r: Route, f: Filters): boolean {
  if (f.repo && r.repo !== f.repo) return false;
  const l = showKey(r);
  if (HIDDEN_BY_DEFAULT.includes(l) && !f.show.has(l)) return false;
  if (f.cards === false && isCard(r)) return false;
  const s = strength(r.strength);
  if (s && !f.strengths.has(s)) return false;
  return matches(r, f.query.trim());
}

/** Counts for the filter chips, within the repository. */
export function counts(g: Graph, repo: string | null) {
  const rs = g.routes.filter((r) => !repo || r.repo === repo);
  const by = <K extends string>(k: (r: Route) => K | null) => {
    const m = new Map<K, number>();
    for (const r of rs) {
      const v = k(r);
      if (v) m.set(v, (m.get(v) ?? 0) + 1);
    }
    return m;
  };
  const liveOnly = rs.filter((r) => {
    const l = lifecycle(r.state);
    return l !== "nascent" && l !== "extinguished";
  });
  const strengths = new Map<Strength, number>();
  for (const r of liveOnly) {
    const s = strength(r.strength);
    if (s) strengths.set(s, (strengths.get(s) ?? 0) + 1);
  }
  return {
    strengths,
    lifecycles: by<Showable>(showKey),
    cards: rs.filter(isCard).length,
    reflexes: rs.filter((r) => !isInstinct(r)).length,
    instincts: rs.filter(isInstinct).length,
  };
}

function strip(s: string): string {
  return s.replace(/`/g, "");
}

/** The first time each reflex applied (directed, or would have in the control arm). */
export function firstFired(events: TraceEvent[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const e of events) {
    for (const id of [...e.directed, ...e.would]) {
      const k = `${e.repo}::${id}`;
      const at = m.get(k);
      if (at === undefined || e.ts < at) m.set(k, e.ts);
    }
  }
  return m;
}

export interface BuildOptions {
  mode: Mode;
  filters: Filters;
  /** Replay: the scrubbed time; reflexes that had not applied by then are unformed. */
  at?: number | null;
}

/** The map's nodes and edges for the visible reflexes. */
export function buildMap(g: Graph, o: BuildOptions): { nodes: MapNode[]; edges: MapEdge[] } {
  const routes = g.routes.filter((r) => visible(r, o.filters));
  const fired = o.at != null ? firstFired(g.events) : null;
  const nodes = new Map<string, MapNode>();
  const edges: MapEdge[] = [];
  const sessionsOf = new Map<string, number>();

  for (const r of routes) {
    const key = routeKey(r);
    const s = strength(r.strength);
    const l = lifecycle(r.state);
    const inst = isInstinct(r);
    const firstAt = fired?.get(`${r.repo}::${r.id}`);
    const unformed = o.at != null && !inst && (firstAt === undefined || firstAt > o.at);
    const rid = `r:${key}`;
    nodes.set(rid, {
      id: rid,
      kind: inst ? "instinct" : "reflex",
      tag: inst ? "INSTINCT" : isQuiet(r) ? "REFLEX · QUIET" : isCard(r) ? "REFLEX · CARD" : "REFLEX",
      title: r.id,
      sub: [s ?? "", inst ? "instinct" : isQuiet(r) ? "quiet" : l, evidenceShort(r)].filter(Boolean).join(" · "),
      strength: s,
      lifecycle: l,
      quiet: isQuiet(r),
      card: isCard(r),
      routes: [key],
      unformed,
    });

    const agg = o.mode === "aggregated";
    const sid = agg ? `s:${r.repo}:${r.trigger.kind}:${r.trigger.label}` : `s:${key}`;
    const st = nodes.get(sid);
    sessionsOf.set(sid, (sessionsOf.get(sid) ?? 0) + (r.live?.sessions ?? 0));
    if (st) {
      st.routes.push(key);
    } else {
      nodes.set(sid, {
        id: sid,
        kind: "stimulus",
        tag: stimulusKind(r.trigger).toUpperCase(),
        title: strip(r.trigger.label),
        sub: "",
        routes: [key],
      });
    }

    const step = r.step;
    const pid = agg ? `p:${r.repo}:${step.kind}:${step.label}:${step.avoid ?? ""}` : `p:${key}`;
    const pt = nodes.get(pid);
    if (pt) {
      pt.routes.push(key);
    } else {
      nodes.set(pid, {
        id: pid,
        kind: "response",
        tag: responseKind(step).toUpperCase(),
        title: strip(step.label),
        sub: step.kind === "avoid" ? strip(step.avoid ?? "") : "",
        strike: step.kind === "avoid",
        routes: [key],
      });
    }
    edges.push({ id: `e:${sid}->${rid}`, source: sid, target: rid, route: key, strength: s });
    edges.push({ id: `e:${rid}->${pid}`, source: rid, target: pid, route: key, strength: s });
  }

  // Subtitles that depend on everything the node gathers.
  for (const n of nodes.values()) {
    if (n.kind === "stimulus") {
      const k = sessionsOf.get(n.id) ?? 0;
      const reflexes = n.routes.length;
      n.sub = [reflexes > 1 ? `starts ${reflexes} reflexes` : "", k ? `in ${k} trace${k === 1 ? "" : "s"}` : "not seen live yet"]
        .filter(Boolean)
        .join(" · ");
    } else if (n.kind === "response" && !n.strike) {
      const reflexes = n.routes.length;
      n.sub = reflexes > 1 ? `from ${reflexes} reflexes` : n.tag === "READ TOGETHER" ? "files the last result listed" : "";
    }
    if (n.kind === "stimulus" || n.kind === "response") {
      const all = n.routes.map((k) => nodes.get(`r:${k}`));
      n.unformed = all.length > 0 && all.every((x) => x?.unformed);
    }
  }
  return { nodes: [...nodes.values()], edges };
}

/** Nodes within `hops` of `from` (edges followed both ways); everything when `from` is absent. */
export function neighbourhood(edges: MapEdge[], from: string | null, hops: number): Set<string> | null {
  if (!from || hops <= 0) return null;
  const adj = new Map<string, string[]>();
  for (const e of edges) {
    adj.set(e.source, [...(adj.get(e.source) ?? []), e.target]);
    adj.set(e.target, [...(adj.get(e.target) ?? []), e.source]);
  }
  const seen = new Set([from]);
  let frontier = [from];
  for (let i = 0; i < hops; i++) {
    const next: string[] = [];
    for (const n of frontier) {
      for (const m of adj.get(n) ?? []) {
        if (!seen.has(m)) {
          seen.add(m);
          next.push(m);
        }
      }
    }
    frontier = next;
  }
  return seen;
}

/** Keep only the nodes (and edges between them) in `keep`. */
export function focus(
  map: { nodes: MapNode[]; edges: MapEdge[] },
  keep: Set<string> | null,
): { nodes: MapNode[]; edges: MapEdge[] } {
  if (!keep) return map;
  return {
    nodes: map.nodes.filter((n) => keep.has(n.id)),
    edges: map.edges.filter((e) => keep.has(e.source) && keep.has(e.target)),
  };
}

export const NODE_W = 232;
export const NODE_H = 76;
export const COL_GAP = 62;
export const ROW_GAP = 24;

export type Direction = "RIGHT" | "DOWN";

export interface Placed {
  positions: Record<string, { x: number; y: number }>;
}

const COLUMN: Record<NodeKind, number> = { stimulus: 0, reflex: 1, instinct: 1, response: 2 };

/**
 * The instant first layout: three columns, reflexes in order, each stimulus and response next to
 * the first reflex it serves (pushed down past the one above). ELK refines it in a worker.
 */
export function columnLayout(map: { nodes: MapNode[]; edges: MapEdge[] }, dir: Direction = "RIGHT"): Placed {
  const reflexes = map.nodes.filter((n) => COLUMN[n.kind] === 1);
  const row = new Map<string, number>();
  reflexes.forEach((n, i) => row.set(n.routes[0] ?? n.id, i));
  const positions: Record<string, { x: number; y: number }> = {};
  const pitch = NODE_H + ROW_GAP;
  const lanePitch = NODE_W + COL_GAP;
  const next = [0, 0, 0];
  const place = (n: MapNode, want: number) => {
    const c = COLUMN[n.kind];
    const y = Math.max(want, next[c] ?? 0);
    next[c] = y + pitch;
    positions[n.id] = dir === "RIGHT" ? { x: c * lanePitch, y } : { x: y * (NODE_W / NODE_H) * 0.9, y: c * (NODE_H + 70) };
  };
  const byRow = (n: MapNode) => Math.min(...n.routes.map((k) => row.get(k) ?? 0));
  const ordered = [...map.nodes].sort((a, b) => byRow(a) - byRow(b) || COLUMN[a.kind] - COLUMN[b.kind]);
  for (const n of ordered) place(n, byRow(n) * pitch);
  return { positions };
}

/** Where to put the Stimulus / Reflex / Response labels above the columns. */
export function columnHeads(
  map: { nodes: MapNode[] },
  placed: Placed,
): { label: string; x: number; y: number }[] {
  const heads: { label: string; x: number; y: number }[] = [];
  const labels = ["Stimulus", "Reflex", "Response"];
  for (let c = 0; c < 3; c++) {
    const ps = map.nodes.filter((n) => COLUMN[n.kind] === c).map((n) => placed.positions[n.id]).filter((p) => !!p);
    if (!ps.length) continue;
    heads.push({
      label: labels[c] ?? "",
      x: Math.min(...ps.map((p) => p.x)),
      y: Math.min(...ps.map((p) => p.y)),
    });
  }
  return heads;
}
