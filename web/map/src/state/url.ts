// All view state lives in the URL, so every view is shareable and the back button works.

import { useCallback, useSyncExternalStore } from "react";
import type { Direction, Mode } from "../lib/graph";
import { HIDDEN_BY_DEFAULT } from "../lib/graph";
import { SHOWABLE, type Showable, STRENGTHS, type Strength } from "../lib/words";

export type View = "map" | "traces" | "evidence" | "health" | "how";
export const VIEWS: View[] = ["map", "traces", "evidence", "health", "how"];
export type Window = "24h" | "7d" | "30d" | "all";
export const WINDOWS: Window[] = ["24h", "7d", "30d", "all"];

export interface UrlState {
  view: View;
  repo: string | null;
  reflex: string | null;
  mode: Mode;
  focus: number;
  dir: Direction;
  q: string;
  strengths: Set<Strength>;
  show: Set<Showable>;
  /** Procedure cards on the map; `cards=hide` turns them off. */
  cards: boolean;
  window: Window;
  /** Replay: the scrubbed time, or live. */
  at: number | null;
  /** How it works: the chapter and the step within it. */
  ch: string | null;
  step: number;
}

function list<T extends string>(v: string | null, all: readonly T[], dflt: readonly T[]): Set<T> {
  if (v === null) return new Set(dflt);
  return new Set(v.split(",").filter((x): x is T => (all as readonly string[]).includes(x)));
}

export function parse(search: string): UrlState {
  const p = new URLSearchParams(search);
  const view = p.get("view");
  const mode = p.get("mode");
  const win = p.get("window");
  const at = Number(p.get("at"));
  return {
    view: VIEWS.includes(view as View) ? (view as View) : "map",
    repo: p.get("repo") || null,
    reflex: p.get("reflex") || null,
    mode: mode === "expanded" ? "expanded" : "aggregated",
    focus: Math.max(0, Math.min(3, Number(p.get("focus")) || 0)),
    dir: p.get("dir") === "down" ? "DOWN" : "RIGHT",
    q: p.get("q") ?? "",
    strengths: list(p.get("strength"), STRENGTHS, STRENGTHS),
    show: list(p.get("show"), SHOWABLE, []),
    cards: p.get("cards") !== "hide",
    window: WINDOWS.includes(win as Window) ? (win as Window) : "all",
    at: p.has("at") && Number.isFinite(at) && at > 0 ? at : null,
    ch: p.get("ch") || null,
    step: Math.max(0, Math.floor(Number(p.get("step")) || 0)),
  };
}

export function serialize(s: UrlState): string {
  const p = new URLSearchParams();
  if (s.view !== "map") p.set("view", s.view);
  if (s.repo) p.set("repo", s.repo);
  if (s.reflex) p.set("reflex", s.reflex);
  if (s.mode !== "aggregated") p.set("mode", s.mode);
  if (s.focus) p.set("focus", String(s.focus));
  if (s.dir === "DOWN") p.set("dir", "down");
  if (s.q) p.set("q", s.q);
  if (s.strengths.size !== STRENGTHS.length) p.set("strength", STRENGTHS.filter((x) => s.strengths.has(x)).join(","));
  const shown = SHOWABLE.filter((x) => s.show.has(x) && HIDDEN_BY_DEFAULT.includes(x));
  if (shown.length) p.set("show", shown.join(","));
  if (!s.cards) p.set("cards", "hide");
  if (s.window !== "all") p.set("window", s.window);
  if (s.at) p.set("at", String(s.at));
  if (s.view === "how" && s.ch) p.set("ch", s.ch);
  if (s.view === "how" && s.step) p.set("step", String(s.step));
  const q = p.toString();
  return q ? `?${q}` : "";
}

const listeners = new Set<() => void>();
let cachedSearch: string | null = null;
let cached: UrlState | null = null;

function snapshot(): UrlState {
  const search = typeof location === "undefined" ? "" : location.search;
  if (search !== cachedSearch || !cached) {
    cachedSearch = search;
    cached = parse(search);
  }
  return cached;
}

function subscribe(fn: () => void) {
  listeners.add(fn);
  const pop = () => fn();
  window.addEventListener("popstate", pop);
  return () => {
    listeners.delete(fn);
    window.removeEventListener("popstate", pop);
  };
}

/** Change the URL state. `push` makes a history entry (views and selection); else it replaces. */
export function setUrl(patch: Partial<UrlState>, push = false) {
  const next = { ...snapshot(), ...patch };
  const url = `${location.pathname}${serialize(next)}${location.hash}`;
  if (url === `${location.pathname}${location.search}${location.hash}`) return;
  if (push) history.pushState(null, "", url);
  else history.replaceState(null, "", url);
  for (const l of listeners) l();
}

export function useUrl(): [UrlState, (patch: Partial<UrlState>, push?: boolean) => void] {
  const s = useSyncExternalStore(subscribe, snapshot, snapshot);
  const set = useCallback((patch: Partial<UrlState>, push?: boolean) => setUrl(patch, push), []);
  return [s, set];
}

export function windowStart(w: Window, now: number): number {
  switch (w) {
    case "24h":
      return now - 24 * 3600e3;
    case "7d":
      return now - 7 * 24 * 3600e3;
    case "30d":
      return now - 30 * 24 * 3600e3;
    default:
      return 0;
  }
}
