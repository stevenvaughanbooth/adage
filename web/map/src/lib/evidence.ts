// The Evidence view's numbers, from the per-arm counts the binary reports. Nothing is estimated:
// every figure is k of n distinct traces, with its denominator.

import type { Graph, Route, TraceEvent } from "../api/types";

export interface Ratio {
  k: number;
  n: number;
}

export interface Kpi {
  label: string;
  treated: Ratio;
  control: Ratio;
  /** Whether a lower share is the better outcome (dead ends). */
  lowerIsBetter: boolean;
  note: string;
}

const sum = (rs: Route[], f: (r: Route) => number) => rs.reduce((a, r) => a + f(r), 0);

export function share(r: Ratio): number | null {
  return r.n ? r.k / r.n : null;
}

export function pct(r: Ratio): string {
  const s = share(r);
  return s === null ? "—" : `${Math.round(s * 100)}%`;
}

/** Headline comparisons across the reflexes in view. */
export function kpis(routes: Route[]): Kpi[] {
  const paths = routes.filter((r) => r.step.kind !== "avoid");
  const dead = routes.filter((r) => r.step.kind === "avoid");
  const lines = routes.filter((r) => (r.arms.treated?.met ?? 0) + (r.arms.control?.met ?? 0) > 0);
  const t = (f: (a: NonNullable<Route["arms"]["treated"]>) => number) => (r: Route) => (r.arms.treated ? f(r.arms.treated) : 0);
  const c = (f: (a: NonNullable<Route["arms"]["control"]>) => number) => (r: Route) => (r.arms.control ? f(r.arms.control) : 0);
  return [
    {
      label: "Took the known path",
      treated: { k: sum(paths, t((a) => a.followed)), n: sum(paths, t((a) => a.applied)) },
      control: { k: sum(paths, c((a) => a.followed)), n: sum(paths, c((a) => a.applied)) },
      lowerIsBetter: false,
      note: "reflex–trace pairs · current versions",
    },
    {
      label: "Ran into a known dead end",
      treated: { k: sum(dead, t((a) => (a.hit_avoided ?? 0))), n: sum(dead, t((a) => a.applied)) },
      control: { k: sum(dead, c((a) => (a.hit_avoided ?? 0))), n: sum(dead, c((a) => a.applied)) },
      lowerIsBetter: true,
      note: "inhibit reflexes · lower is better",
    },
    {
      label: "Met the finish line",
      treated: { k: sum(lines, t((a) => a.met)), n: sum(lines, t((a) => a.applied)) },
      control: { k: sum(lines, c((a) => a.met)), n: sum(lines, c((a) => a.applied)) },
      lowerIsBetter: false,
      note: "obligations such as regenerating code",
    },
  ];
}

/** Distinct traces per day, by arm, for the chart. */
export function perDay(events: Pick<TraceEvent, "ts" | "repo" | "session" | "arm">[]): { day: number; treated: number; control: number }[] {
  const days = new Map<number, { t: Set<string>; c: Set<string> }>();
  for (const e of events) {
    const d = new Date(e.ts);
    d.setHours(0, 0, 0, 0);
    const k = d.getTime();
    const b = days.get(k) ?? { t: new Set<string>(), c: new Set<string>() };
    (e.arm === "control" ? b.c : b.t).add(JSON.stringify([e.repo, e.session]));
    days.set(k, b);
  }
  return [...days.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([day, b]) => ({ day, treated: b.t.size, control: b.c.size }));
}

/** The traces in each arm, from the events in view. */
export function armTraces(g: Graph, events: Pick<TraceEvent, "repo" | "session" | "arm">[]): { treated: number; control: number } {
  const t = new Set<string>();
  const c = new Set<string>();
  for (const e of events) (e.arm === "control" ? c : t).add(JSON.stringify([e.repo, e.session]));
  void g;
  return { treated: t.size, control: c.size };
}
