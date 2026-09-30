// The JSON the adage binary serves (crates/adage-cli/src/graph.rs, map/http.rs), in the lexicon's
// stored words: strength fire/prime/inhibit, state nascent/forming/firm/dormant/extinguished, arms
// treated/control, judgements followed/ignored/misfired/obligation_met. Older binaries wrote
// aspect green/yellow/red, draft/probation/…, holdout and derailed; src/api/normalize.ts reads
// those once, so these types hold only the new words.

export type Arm = "treated" | "control" | string;

export interface Trigger {
  kind: "prompt" | "after_command" | "after_failure" | "after_edit" | "session_start" | "intent" | "context" | string;
  label: string;
  /** Intent: the family, and how close (cosine) a prompt must be to it. */
  family?: string;
  threshold?: number;
  /** Context: the events in time order, each within `within` events of the one before. */
  within?: number;
  items?: { kind: string; label: string }[];
}

export interface CardCommand {
  command: string;
  sessions: number;
  runs: number;
  failed: number;
  /** For a command to avoid: the first line of its error. */
  error?: string | null;
  /** Replayed on a clean checkout (`adage train cards`): worked, failed, or not replayed (null). */
  verified?: boolean | null;
}

/** A procedure card's contents: what earlier traces of a family of similar tasks had in common. */
export interface Card {
  /** Traces of the family it was learned from, and how many of them passed. */
  sessions: number;
  passed: number;
  files: { path: string; sessions: number }[];
  exemplars?: { path: string; lines: string[] }[];
  commands: CardCommand[];
  avoid: CardCommand[];
  finish?: string | null;
  /** Delivered at a prompt at least this close to the family; from `grey_from`, at its first edit of a card file. */
  threshold?: number | null;
  grey_from?: number | null;
  /** Always false: a card is first-hand only and never travels in a pack. */
  shared?: boolean;
}

export interface Step {
  kind: "run" | "read_batch" | "avoid" | "advise" | "sequence" | "card" | string;
  label: string;
  avoid?: string;
  signature?: string;
  slot?: string;
  template?: string;
  /** Sequence: the commands, in order. */
  commands?: string[];
  card?: Card;
}

/** What delivering a reflex changes: how much more often agents take the step when told than unaided. */
export interface Uplift {
  kind: string;
  p_follow: number;
  p_self: number;
  mean: number;
  lower: number;
  prior_self?: number;
  prior_fitted?: boolean;
  traces?: number;
  traces_took?: number;
  control?: number;
  control_took?: number;
  delivered?: number;
  followed?: number;
  /** Below the bar: kept quiet until a person approves it. */
  gated?: boolean;
  /** The binary's own sentence for it. */
  line?: string;
}

/** The value estimate a reflex formed on (`.why.json`): calls saved, P(saves) and its lower bound. */
export interface Why {
  shape: string;
  pattern: string;
  gain: number;
  lift: number;
  conf: number;
  conf_lb: number;
  support: number;
  days: number;
  backoff?: number;
}

export interface ArmStats {
  sessions: number;
  applied: number;
  followed: number;
  met: number;
  hit_avoided: number;
}

export interface Evidence {
  /** Recounted routine evidence, separate from execution authority and measured savings. */
  execution?: { opportunities: number; immediate: number; sessions: number } | null;
  support: number;
  opportunities: number;
  rate: number;
  lower_bound: number;
  turns_saved: number;
  days?: number;
  exposed?: number;
  /** Since when the reflex has starved its own evidence (older binaries: `stale_since`). */
  dormant_since?: string | null;
  provenance?: string;
  /** The evidence the strength was decided on, in one line (`adage status` prints the same). */
  describe?: string;
  /** What that was: a follow count, a value estimate, a family of traces (cards), or shipped facts. */
  basis?: "count" | "value" | "family" | "instinct" | string;
  why?: Why | null;
  uplift?: Uplift | null;
  outcome_lift?: { with: number; with_passed: number; without: number; without_passed: number; diff: number } | null;
  effect_evidence?: string | null;
}

export interface Live {
  emitted: number;
  would_have: number;
  followed: number;
  ignored: number;
  misfired?: number;
  obligation_met: number;
  control: { unaided: number; not_reached: number; obligation_met: number };
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
  review?: { verbs: string[]; reasons: { kind: string; why: string }[]; execution_command?: string | null };
  /** Silent unless an exact local execution grant is valid; never ordinary advice. */
  execution_only?: boolean;
  /** fire / prime / inhibit (older binaries: `aspect` green/yellow/red). */
  strength: string;
  ceiling: string;
  effect: string;
  state: string;
  authored: boolean;
  /** The delivery shape: run, avoid, read-batch, obligation, sequence, advise, card. */
  shape?: string;
  /** Kept quiet on its uplift: agents already take the step unaided, so it waits for a person. */
  quiet?: boolean;
  /** What it does beyond advising: a learned finish line is asked for once at Stop. */
  obligation?: string | null;
  trigger: Trigger;
  step: Step;
  directive: string;
  evidence: Evidence;
  live: Live;
  arms: { treated?: ArmStats; control?: ArmStats };
}

/** What the PreToolUse hook did before a command ran (only the kind and the interpreter are kept). */
export interface Pretool {
  rewrite?: "test" | "repro" | string;
  advise?: "pip-install" | "permissions" | string;
  interpreter?: string;
}

/** One hook decision: routes directed, would-have (control), judgements `[kind, route]`. */
export interface TraceEvent {
  /** `pretool`: a rewrite or advice before a command ran (`withheld` in the control arm). */
  kind?: "pretool" | string;
  pretool?: Pretool;
  withheld?: boolean;
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
  /** new, nascent, extinguished, dormant, strength, evidence or proposal. */
  change: string;
  from?: string | null;
  to?: string | null;
  summary?: string;
}

export interface LearnRun {
  ts: number;
  /** `condition` or `condition_error` (older binaries: `learn`, `learn_error`). */
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

/** One outcome's estimate: Δ = control − treated per session (positive saves), with its 95% interval. */
export interface Estimate {
  delta: number;
  lo: number;
  hi: number;
  mean_control: number;
  mean_treated: number;
  n_control: number;
  n_treated: number;
  winsorized_at?: number;
}

/** A repository's savings estimate (`adage savings --json`), with the lines the CLI prints. */
export interface Savings {
  repo: string;
  computed_at?: number;
  pricing_as_of?: string;
  n_treated: number;
  n_control: number;
  gate_met: boolean;
  verdict: "measuring" | "saved" | "no_difference" | "adds_cost" | string;
  cost_usd?: Estimate | null;
  model_turns?: Estimate | null;
  turns_to_first_edit?: Estimate | null;
  unpriced?: number;
  asked_for_help_treated?: number;
  asked_for_help_control?: number;
  counts: {
    days: number;
    sessions: number;
    directives: number;
    followed: number;
    dead_ends_headed_off: number;
    finish_lines_met: number;
    rewritten: number;
  };
  need: { control: number; treated: number };
  lines: string[];
  guardrail?: string | null;
}

export interface PretoolCounts {
  repo: string;
  rewritten: number;
  advised: number;
  withheld: number;
}

export interface RewriteState {
  on: boolean;
  /** `runtime.rewrite`, and whether the older `exec/rewrite-off` marker turns them off. */
  config: boolean;
  marker: boolean;
}

export interface Graph {
  demo?: boolean;
  generation: string;
  home: string;
  sessions: { total: number; treated: number; control: number };
  stop_requests: number;
  routes: Route[];
  observed: Observed[];
  events: TraceEvent[];
  learning?: LearnRun[];
  bars?: { min_support: number; fire_lower_bound: number; uplift_min?: number };
  savings: Savings[];
  pretool: PretoolCounts[];
  runtime?: { rewrite: RewriteState; control?: number; card_threshold?: number };
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
  /** Settings, not problems: the rewrite switch and what intent reflexes embed with. */
  runtime?: { rewrite: RewriteState; embeddings: { model: string; bundled: boolean; fallback: boolean } };
}
