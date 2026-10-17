// Tolerant reading of the graph JSON. The binary now writes the lexicon's words in its data
// (strength fire/prime/inhibit, states nascent/forming/firm/dormant/extinguished, the control arm,
// ignored/misfired judgements, learn.log kind `condition` and changes strength/nascent/…). Older
// binaries and older spools wrote aspect green/yellow/red, draft/probation/active/stale/retired/
// demoted, holdout, derailed/followed_failed, `learn` and aspect/draft/retired/stale. A newer app may
// read older data, so every reader goes through here once, and the rest of the app sees only the
// new words.

import type { ArmStats, Graph, LearningHistory, LearnChange, LearnRun, Live, Route, Savings, TraceEvent } from "./types";

type Loose = Record<string, unknown>;

const obj = (v: unknown): Loose => (v && typeof v === "object" ? (v as Loose) : {});
const num = (v: unknown): number => (typeof v === "number" && Number.isFinite(v) ? v : 0);

const STRENGTH: Record<string, string> = { green: "fire", yellow: "prime", red: "inhibit" };

/** fire / prime / inhibit from either spelling (other values pass through, lowercased). */
export function strengthWord(v: unknown): string {
  if (typeof v !== "string") return "";
  const s = v.toLowerCase();
  return STRENGTH[s] ?? s;
}

const STATE: Record<string, string> = {
  draft: "nascent",
  probation: "forming",
  active: "firm",
  stale: "dormant",
  retired: "extinguished",
  // There is no weakened state any more: a demoted route was still delivered, at its lower strength.
  demoted: "firm",
  weakened: "firm",
};

export function stateWord(v: unknown): string {
  if (typeof v !== "string") return "firm";
  const s = v.toLowerCase();
  return STATE[s] ?? s;
}

/** `holdout` was the control arm's old name. */
export function armWord(v: unknown): string | null {
  if (typeof v !== "string") return null;
  return v === "holdout" ? "control" : v;
}

const JUDGEMENT: Record<string, string> = { derailed: "ignored", followed_failed: "misfired" };

export function judgementWord(v: string): string {
  return JUDGEMENT[v] ?? v;
}

const CHANGE: Record<string, string> = {
  aspect: "strength",
  draft: "nascent",
  retired: "extinguished",
  stale: "dormant",
  added: "new",
};

const RUN_KIND: Record<string, string> = { learn: "condition", learn_error: "condition_error" };

function armStats(v: unknown): ArmStats | undefined {
  if (!v || typeof v !== "object") return undefined;
  const a = obj(v);
  return { sessions: num(a.sessions), applied: num(a.applied), followed: num(a.followed), met: num(a.met), hit_avoided: a.hit_avoided === null ? null : num(a.hit_avoided) };
}

function live(v: unknown): Live {
  const l = obj(v);
  const c = obj(l.control ?? l.holdout);
  return {
    emitted: num(l.emitted),
    would_have: num(l.would_have),
    followed: num(l.followed),
    ignored: num(l.ignored ?? l.derailed),
    misfired: num(l.misfired ?? l.followed_failed),
    obligation_met: num(l.obligation_met),
    control: { unaided: num(c.unaided), not_reached: num(c.not_reached), obligation_met: num(c.obligation_met) },
    sessions: num(l.sessions),
  };
}

/** Where a reflex came from. Older binaries sent a label string (or nothing) instead of the object. */
function normalizeOrigin(o: unknown, imported: boolean, instinct: boolean): Route["origin"] {
  if (o && typeof o === "object") return o as Route["origin"];
  const kind = instinct ? "instinct" : imported ? "imported" : "learned";
  return { kind, label: typeof o === "string" ? o : null };
}

export function normalizeRoute(raw: unknown): Route {
  const r = obj(raw) as Loose & Partial<Route>;
  const ev = obj(r.evidence);
  const arms = obj(r.arms);
  const review = r.review
    ? { ...r.review, reasons: (r.review.reasons ?? []).map((x) => ({ ...x, kind: x.kind === "derailing" ? "ignoring" : x.kind === "draft" ? "nascent" : x.kind })) }
    : undefined;
  const state = stateWord(r.state);
  const uplift = (ev.uplift ?? null) as Route["evidence"]["uplift"];
  // Older binaries sent no `quiet`: a nascent reflex whose uplift was gated, or whose review
  // reason says so, is quiet.
  const quiet =
    typeof r.quiet === "boolean"
      ? r.quiet
      : state === "nascent" && (Boolean(uplift?.gated) || Boolean(review?.reasons.some((x) => x.kind === "quiet")));
  return {
    ...(r as Route),
    origin: normalizeOrigin(r.origin, Boolean(r.imported), Boolean(r.instinct)),
    directive: (r.directive ?? null) as string | null,
    review,
    last_review: r.last_review ?? null,
    quiet,
    obligation: (r.obligation ?? null) as string | null,
    strength: strengthWord(r.strength ?? r.aspect),
    ceiling: strengthWord(r.ceiling),
    state,
    evidence: {
      ...(ev as unknown as Route["evidence"]),
      support: num(ev.support),
      opportunities: num(ev.opportunities),
      rate: num(ev.rate),
      lower_bound: num(ev.lower_bound),
      turns_saved: num(ev.turns_saved),
      dormant_since: (ev.dormant_since ?? ev.stale_since ?? null) as string | null,
      uplift,
    },
    live: live(r.live),
    arms: { treated: armStats(arms.treated), control: armStats(arms.control ?? arms.holdout) },
  };
}

export function normalizeEvent(raw: unknown): TraceEvent {
  const e = obj(raw) as Loose & Partial<TraceEvent>;
  return {
    ...(e as TraceEvent),
    arm: armWord(e.arm),
    directed: e.directed ?? [],
    would: e.would ?? [],
    judged: (e.judged ?? []).map(([k, id]) => [judgementWord(k), id] as [string, string]),
    judgements: e.judgements?.map((j) => ({ ...j, kind: judgementWord(j.kind) })),
    observed: e.observed ?? [],
    continue: Boolean(e.continue),
  };
}

function change(raw: LearnChange): LearnChange {
  const w = (v: string | null | undefined) => (v == null ? v : strengthWord(v));
  return { ...raw, change: CHANGE[raw.change] ?? raw.change, from: w(raw.from), to: w(raw.to) };
}

export function normalizeLearnRun(raw: unknown): LearnRun {
  const l = obj(raw) as Loose & Partial<LearnRun>;
  return {
    ...(l as LearnRun),
    kind: l.kind ? (RUN_KIND[l.kind] ?? l.kind) : l.kind,
    changes: l.changes?.map(change),
  };
}

function savings(raw: unknown): Savings {
  const s = obj(raw) as Loose & Partial<Savings>;
  const c = obj(s.counts);
  const need = obj(s.need);
  return {
    ...(s as Savings),
    repo: String(s.repo ?? ""),
    n_treated: num(s.n_treated),
    n_control: num(s.n_control),
    gate_met: Boolean(s.gate_met),
    verdict: typeof s.verdict === "string" ? s.verdict : "measuring",
    counts: {
      days: num(c.days) || 30,
      sessions: num(c.sessions),
      directives: num(c.directives),
      followed: num(c.followed),
      dead_ends_headed_off: num(c.dead_ends_headed_off),
      finish_lines_met: num(c.finish_lines_met),
      rewritten: num(c.rewritten),
    },
    need: { control: num(need.control) || 10, treated: num(need.treated) || 30 },
    lines: Array.isArray(s.lines) ? s.lines : [],
  };
}

function learningHistory(raw: unknown): LearningHistory | undefined {
  if (!raw || typeof raw !== "object") return undefined;
  const h = obj(raw);
  const count = (v: unknown) => typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : null;
  return {
    source: "recent_conditioning_runs",
    description: typeof h.description === "string" ? h.description : "Latest retained conditioning run per repository; overlapping runs are not added together.",
    repositories: (Array.isArray(h.repositories) ? h.repositories : []).map(value => {
      const r = obj(value);
      return { repo: String(r.repo ?? ""), last_conditioned_at: num(r.last_conditioned_at),
        sessions_used: count(r.sessions_used), files_read: count(r.files_read),
        sessions_steered: count(r.sessions_steered), sessions_duplicates: count(r.sessions_duplicates) };
    }),
  };
}

export function normalizeGraph(raw: unknown): Graph {
  const g = obj(raw) as Loose & Partial<Graph>;
  const s = obj(g.sessions);
  return {
    ...(g as Graph),
    sessions: { total: num(s.total), treated: num(s.treated), control: num(s.control ?? s.holdout) },
    stop_requests: num(g.stop_requests),
    routes: (g.routes ?? []).map(normalizeRoute),
    observed: g.observed ?? [],
    events: (g.events ?? []).map(normalizeEvent),
    trace_events: g.trace_events?.map(normalizeEvent),
    learning: g.learning?.map(normalizeLearnRun),
    learning_history: learningHistory(g.learning_history),
    // Older binaries sent neither.
    savings: Array.isArray(g.savings) ? g.savings.map(savings) : [],
    pretool: Array.isArray(g.pretool) ? g.pretool : [],
  };
}
