// The map's data: the graph snapshot, kept live by the server's event stream.
//
// A decision is appended to the snapshot's events at once (and pulses its reflex); the snapshot
// itself is fetched again shortly after, and at once when learning changed the graph.

import { useSyncExternalStore } from "react";
import { normalizeEvent, normalizeGraph, normalizeLearnRun } from "./normalize";
import type { Graph, Health, LearnRun, TraceEvent } from "./types";

export type Connection = "connecting" | "live" | "offline";

interface Store {
  graph: Graph | null;
  error: string | null;
  connection: Connection;
  /** Decisions received over the stream since the page loaded. */
  streamed: number;
}

const MAX_EVENTS = 1000;
let state: Store = { graph: null, error: null, connection: "connecting", streamed: 0 };
const listeners = new Set<() => void>();
const decisionListeners = new Set<(e: TraceEvent) => void>();

function set(patch: Partial<Store>) {
  state = { ...state, ...patch };
  for (const l of listeners) l();
}

export async function fetchJson<T>(url: string): Promise<T> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 10_000);
  try {
    const r = await fetch(url, { cache: "no-store", headers: { Accept: "application/json" }, signal: controller.signal });
    if (!r.ok) throw new Error(`${r.status} ${(await r.text()).trim()}`);
    return (await r.json()) as T;
  } catch (e) {
    if (controller.signal.aborted) throw new Error("The local server took too long to respond. Try again.");
    throw e;
  } finally {
    clearTimeout(timeout);
  }
}

let inflight: Promise<void> | null = null;
let retryDelay = 1000;
export function refresh(): Promise<void> {
  if (inflight) return inflight;
  if (later) clearTimeout(later);
  later = null;
  inflight = fetchJson<unknown>("/api/v1/graph")
    .then((raw) => {
      retryDelay = 1000;
      set({ graph: normalizeGraph(raw), error: null });
    })
    .catch((e: unknown) => {
      set({ error: e instanceof Error ? e.message : String(e) });
      refreshSoon(retryDelay);
      retryDelay = Math.min(retryDelay * 2, 30_000);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

let later: ReturnType<typeof setTimeout> | null = null;
let due = 0;
function refreshSoon(ms = 1500) {
  // A busy stream must not postpone the snapshot indefinitely.
  const next = Date.now() + ms;
  if (later && due <= next) return;
  if (later) clearTimeout(later);
  due = next;
  later = setTimeout(() => {
    later = null;
    void refresh();
  }, ms);
}

function onDecision(e: TraceEvent) {
  const g = state.graph;
  if (g) {
    const events = [...g.events, e].slice(-MAX_EVENTS);
    set({ graph: { ...g, events }, streamed: state.streamed + 1 });
  } else {
    set({ streamed: state.streamed + 1 });
  }
  for (const l of decisionListeners) l(e);
  refreshSoon();
}

function onLearn(run: LearnRun) {
  const g = state.graph;
  if (g) set({ graph: { ...g, learning: [...(g.learning ?? []), run].slice(-100) } });
  refreshSoon(300);
}

let source: EventSource | null = null;
let started = false;

/** Load the graph and follow the stream (once per page). */
export function start() {
  if (started) return;
  started = true;
  void refresh();
  if (typeof EventSource === "undefined") return;
  source = new EventSource("/api/v1/events");
  source.addEventListener("open", () => {
    // Back after a drop: what changed meanwhile never streamed, so load it now.
    if (state.connection === "offline" || state.error) void refresh();
    set({ connection: "live" });
  });
  // The browser retries on its own (readyState CONNECTING), but once the stream has dropped the
  // page says so: stale data must not look live.
  source.addEventListener("error", () =>
    set({ connection: source?.readyState === 2 || state.connection !== "connecting" || state.graph ? "offline" : "connecting" }),
  );
  source.addEventListener("hello", () => set({ connection: "live" }));
  source.addEventListener("decision", (m) => {
    try {
      onDecision(normalizeEvent(JSON.parse((m as MessageEvent<string>).data)));
    } catch {
      /* a malformed event is skipped */
    }
  });
  source.addEventListener("learn", (m) => {
    try {
      onLearn(normalizeLearnRun(JSON.parse((m as MessageEvent<string>).data)));
    } catch {
      /* skipped */
    }
  });
  source.addEventListener("graph", () => refreshSoon(100));
  source.addEventListener("reset", () => void refresh());
}

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function useStore(): Store {
  return useSyncExternalStore(subscribe, () => state, () => state);
}

/** Called for each decision as it arrives (for pulses). */
export function subscribeDecisions(fn: (e: TraceEvent) => void): () => void {
  decisionListeners.add(fn);
  return () => decisionListeners.delete(fn);
}

let health: Promise<Health> | null = null;
export function loadHealth(force = false): Promise<Health> {
  if (!health || force) {
    const request = fetchJson<Health>("/api/v1/health").catch((e: unknown) => {
      if (health === request) health = null;
      throw e;
    });
    health = request;
  }
  return health;
}
