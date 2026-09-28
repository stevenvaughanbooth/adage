// The lexicon (docs/lexicon.md) at display time. Stored values stay as the binary writes them.

import type { Route, Step, Trigger } from "../api/types";

export type Strength = "fire" | "prime" | "inhibit";
export type Lifecycle = "nascent" | "forming" | "firm" | "dormant" | "extinguished" | "weakened";

export const STRENGTHS: Strength[] = ["fire", "prime", "inhibit"];
export const LIFECYCLE: Lifecycle[] = ["nascent", "forming", "firm", "dormant", "extinguished", "weakened"];

const STRENGTH: Record<string, Strength> = {
  green: "fire",
  yellow: "prime",
  red: "inhibit",
  fire: "fire",
  prime: "prime",
  inhibit: "inhibit",
};

/** fire / prime / inhibit, from a stored aspect (or the newer stored word). */
export function strength(aspect: string | null | undefined): Strength | null {
  return (aspect && STRENGTH[aspect.toLowerCase()]) || null;
}

const STATE: Record<string, Lifecycle> = {
  draft: "nascent",
  probation: "forming",
  active: "firm",
  stale: "dormant",
  retired: "extinguished",
  demoted: "weakened",
  nascent: "nascent",
  forming: "forming",
  firm: "firm",
  dormant: "dormant",
  extinguished: "extinguished",
  weakened: "weakened",
};

export function lifecycle(state: string | null | undefined): Lifecycle {
  return (state && STATE[state.toLowerCase()]) || "firm";
}

/** Delivered at all: nascent and extinguished reflexes say nothing. */
export function live(r: Route): boolean {
  const l = lifecycle(r.state);
  return l !== "nascent" && l !== "extinguished";
}

export function isInstinct(r: Route): boolean {
  return Boolean(r.instinct);
}

/** The stimulus kind, as the node's label says it. */
export function stimulusKind(t: Trigger): string {
  switch (t.kind) {
    case "prompt":
      return "prompt";
    case "after_command":
      return "after command";
    case "after_failure":
      return "after failure";
    case "after_edit":
      return "after edit";
    case "session_start":
      return "session start";
    case "intent":
      return "intent";
    default:
      return t.kind.replace(/_/g, " ");
  }
}

/** The response kind, as the node's label says it. */
export function responseKind(s: Step): string {
  switch (s.kind) {
    case "run":
      return "run";
    case "read_batch":
      return "read together";
    case "avoid":
      return "run instead";
    case "advise":
      return "advise";
    default:
      return s.kind.replace(/_/g, " ");
  }
}

/** A judgement's word: derailed is "ignored" (lexicon), obligation_met is "met". */
export function judgement(kind: string): string {
  switch (kind) {
    case "followed":
      return "followed";
    case "derailed":
      return "ignored";
    case "obligation_met":
      return "met";
    case "misfired":
      return "misfired";
    default:
      return kind.replace(/_/g, " ");
  }
}

/** The arm's word: the holdout arm is "control". */
export function arm(a: string | null | undefined): string {
  if (a === "holdout") return "control";
  if (a === "treated") return "with reflexes";
  return a ?? "unknown";
}

// Where each kind of number comes from. Support, lower bound and turns saved were counted once,
// when the reflex was conditioned; with-vs-control is counted live, in traces since it was
// installed. The two are never mixed, so each is labelled where it appears.

/** The source of a reflex's `evidence` numbers. */
export function evidenceSource(r: Route): string {
  if (isInstinct(r)) return "Shipped with Adage";
  if (r.imported) return "From the pack";
  return "From conditioning";
}

/** One line on what that source means. */
export function evidenceNote(r: Route): string {
  if (isInstinct(r)) return "Instincts carry no traces of their own: bound from what this repository declares.";
  if (r.imported) return "Counted in the traces the pack was conditioned on, not in yours.";
  return "Counted in past traces when Adage conditioned this reflex.";
}

/** The source of the per-arm numbers. */
export const LIVE_SOURCE = "Live since installed";

/** The control arm, in one line. */
export const CONTROL_NOTE = "Control: traces where Adage withheld the reflex on purpose, so the agent worked unaided: the baseline.";

/** Review verbs as the CLI names them (lexicon: promote → strengthen, and so on). */
const VERB: Record<string, string> = {
  promote: "strengthen",
  demote: "weaken",
  reject: "extinguish",
  retire: "extinguish",
  approve: "approve",
  pin: "pin",
  dismiss: "dismiss",
  strengthen: "strengthen",
  weaken: "weaken",
  extinguish: "extinguish",
};

export function verb(v: string): string {
  return VERB[v] ?? v;
}

const VERB_WHY: Record<string, string> = {
  strengthen: "prime → fire, within its ceiling",
  weaken: "fire → prime, held across conditioning",
  extinguish: "stop delivering it",
  approve: "accept it as it is",
  pin: "lock it against re-learning",
  dismiss: "keep it; silence the proposed change",
};

export function verbWhy(v: string): string {
  return VERB_WHY[verb(v)] ?? "";
}

/** `adage review <verb> <track> <reflex>`: what a person runs to decide. */
export function reviewCommand(v: string, r: Pick<Route, "reference" | "id">): string {
  return `adage review ${verb(v)} ${shellWord(r.reference)} ${shellWord(r.id)}`;
}

/** A word safe to paste into a POSIX shell. */
export function shellWord(s: string): string {
  return /^[A-Za-z0-9@%_+=:,./-]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`;
}

/** Decisions that apply, most useful first; approve leads for a nascent reflex. */
export function orderedVerbs(r: Route): string[] {
  const verbs = (r.review?.verbs ?? []).map(verb);
  const order =
    lifecycle(r.state) === "nascent"
      ? ["approve", "extinguish", "strengthen", "weaken", "pin", "dismiss"]
      : ["strengthen", "weaken", "extinguish", "approve", "pin", "dismiss"];
  return [...new Set(verbs)].sort((a, b) => order.indexOf(a) - order.indexOf(b));
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

const PROSE: [RegExp, string][] = [
  [/\broute\(s\)/g, "reflex(es)"],
  [/\bsession\(s\)/g, "trace(s)"],
  [/\bgreen\b/g, "fire"],
  [/\byellow\b/g, "prime"],
  [/\bred\b/g, "inhibit"],
  [/\broutes\b/g, "reflexes"],
  [/\broute\b/g, "reflex"],
  [/\bsessions\b/g, "traces"],
  [/\bholdout\b/g, "control"],
  [/\bretired\b/g, "extinguished"],
  [/\bdrafted\b/g, "nascent"],
  [/\bprobation\b/g, "forming"],
  [/\blearned\b/g, "conditioned"],
];

/**
 * Stored prose (change summaries, review reasons written by the learner) in the lexicon's words,
 * as `words::prose` in the binary does it. Commands (between backticks) and paths are untouched.
 */
export function prose(s: string): string {
  const words = (t: string) =>
    t
      .split(" ")
      .map((w) => (w.includes("/") || w.includes("\\") ? w : PROSE.reduce((acc, [re, rep]) => acc.replace(re, rep), w)))
      .join(" ");
  return s
    .split("`")
    .map((part, i) => (i % 2 === 1 ? part : words(part)))
    .join("`");
}

/** The trace id as people read it: the first 8 characters. */
export function shortSession(s: string): string {
  return s.slice(0, 8);
}
