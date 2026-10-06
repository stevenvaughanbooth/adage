import type {
  Graph,
  Measurement,
  Route,
  TraceEvent,
  TraceSession,
} from "../api/types";

export const traceKey = (s: Pick<TraceSession, "repo" | "session">) =>
  JSON.stringify([s.repo, s.session]);
export const exactRouteKey = (r: Route) =>
  JSON.stringify([r.repo, r.reference, r.id]);
export function measurement(r: Route, version: string | null): Measurement {
  return r.measurement_history?.find((m) => m.version === version) ?? r;
}
/** No id-only attribution: identical route ids can belong to different installed reflexes. */
export function relates(
  e: TraceEvent,
  r: Route,
  version?: string | null,
): boolean {
  const key = exactRouteKey(r);
  return [
    ...(e.directives ?? []),
    ...(e.would_have ?? []),
    ...(e.judgements ?? []),
  ].some((d) => d.route_key === key && (!version || d.version === version));
}
export function traceInventory(g: Graph): TraceSession[] {
  if (g.trace_sessions) return g.trace_sessions;
  const sessions = new Map<string, TraceSession>();
  for (const e of g.trace_events ?? g.events) {
    const key = traceKey(e),
      s = sessions.get(key);
    if (s) {
      s.events_total++;
      s.end_ts = Math.max(s.end_ts ?? 0, e.ts);
    } else
      sessions.set(key, {
        session: e.session,
        repo: e.repo,
        arm: e.arm,
        events_total: 1,
        start_ts: e.ts,
        end_ts: e.ts,
      });
  }
  return [...sessions.values()];
}
