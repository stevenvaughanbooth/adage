// The JSON the adage binary serves (crates/adage-cli/src/graph.rs, map/http.rs). Stored values
// (aspect green/yellow/red, state draft/probation/…) are kept as they are and translated for
// people at display time (src/lib/words.ts); the newer stored words (fire/prime/inhibit,
// nascent/forming/…) are accepted too.

export type Arm = "treated" | "holdout" | string;

export interface Trigger {
  kind: "prompt" | "after_command" | "after_failure" | "after_edit" | "session_start" | "intent" | string;
  label: string;
}

export interface Step {
  kind: "run" | "read_batch" | "avoid" | "advise" | string;
  label: string;
  avoid?: string;
  signature?: string;
  slot?: string;
  template?: string;
}

export interface ArmStats {
  sessions: number;
  applied: number;
  followed: number;
  met: number;
  hit_avoided: number;
}

export interface Evidence {
  support: number;
  opportunities: number;
  rate: number;
  lower_bound: number;
  turns_saved: number;
  days?: number;
  exposed?: number;
  stale_since?: string | null;
  provenance?: string;
}

export interface Live {
  emitted: number;
  would_have: number;
  followed: number;
  derailed: number;
  obligation_met: number;
  holdout: { unaided: number; not_reached: number; obligation_met: number };
  sessions: number;
}

export interface Route {
  id: string;
  summary?: string;
  track: string;
  version: string;
  /** The track's directory under the tracks directory: what `adage review` takes. */
  reference: string;
  repo: string;
  imported: boolean;
  in_index: boolean;
  instinct?: boolean;
  origin?: string | null;
  review?: { verbs: string[]; reasons: { kind: string; why: string }[] };
  aspect: string;
  ceiling: string;
  effect: string;
  state: string;
  authored: boolean;
  trigger: Trigger;
  step: Step;
  directive: string;
  evidence: Evidence;
  live: Live;
  arms: { treated?: ArmStats; holdout?: ArmStats };
}

/** One hook decision: routes directed, would-have (control), judgements `[kind, route]`. */
export interface TraceEvent {
  ts: number;
  session: string;
  arm: Arm | null;
  event: string;
  repo: string;
  directed: string[];
  would: string[];
  judged: [string, string][];
  observed: string[];
  continue: boolean;
}

export interface LearnChange {
  route: string;
  track?: string;
  change: string;
  from?: string | null;
  to?: string | null;
  summary?: string;
}

export interface LearnRun {
  ts: number;
  kind?: string;
  trigger?: string;
  repo?: string | null;
  sessions_read?: number;
  sessions_used?: number;
  duration_ms?: number;
  index_generation?: string;
  changes?: LearnChange[];
}

export interface Observed {
  key: string;
  count: number;
  sessions: number;
  route: string | null;
  how: "directs" | "avoids" | null;
}

export interface Graph {
  generation: string;
  home: string;
  sessions: { total: number; treated: number; holdout: number };
  stop_requests: number;
  routes: Route[];
  observed: Observed[];
  events: TraceEvent[];
  learning?: LearnRun[];
  bars?: { min_support: number; fire_lower_bound: number };
}

export interface HealthCheck {
  check: string;
  /** The check's name as people read it (the lexicon's words); `check` is its id. */
  label?: string;
  status: "pass" | "warn" | "fail";
  detail: string;
  fix?: string;
}

export interface Health {
  checks: HealthCheck[];
  summary: { pass: number; warn: number; fail: number };
}
