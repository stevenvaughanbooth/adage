// The lexicon (docs/lexicon.md) at display time. The binary stores the lexicon's words now; the
// older spellings are still read here (and once in src/api/normalize.ts) for older data.

import type { Pretool, Route, Savings, Step, Trigger } from "../api/types";

export type Strength = "fire" | "prime" | "inhibit";
export type Lifecycle = "nascent" | "forming" | "firm" | "dormant" | "extinguished";

export const STRENGTHS: Strength[] = ["fire", "prime", "inhibit"];
export const LIFECYCLE: Lifecycle[] = ["nascent", "forming", "firm", "dormant", "extinguished"];
/** What the map's filters can show or hide: the lifecycles, and quiet (nascent on its uplift). */
export type Showable = Lifecycle | "quiet";
export const SHOWABLE: Showable[] = [...LIFECYCLE, "quiet"];

const STRENGTH: Record<string, Strength> = {
  green: "fire",
  yellow: "prime",
  red: "inhibit",
  fire: "fire",
  prime: "prime",
  inhibit: "inhibit",
};

/** fire / prime / inhibit, from the stored strength (or an older stored aspect green/yellow/red). */
export function strength(stored: string | null | undefined): Strength | null {
  return (stored && STRENGTH[stored.toLowerCase()]) || null;
}

const STATE: Record<string, Lifecycle> = {
  draft: "nascent",
  probation: "forming",
  active: "firm",
  stale: "dormant",
  retired: "extinguished",
  // No weakened state: a demoted route was still delivered, at its lower strength.
  demoted: "firm",
  nascent: "nascent",
  forming: "forming",
  firm: "firm",
  dormant: "dormant",
  extinguished: "extinguished",
  weakened: "firm",
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

/** Kept quiet on its uplift (agents already do this): waits for a person, it is not an install step. */
export function isQuiet(r: Route): boolean {
  return Boolean(r.quiet);
}

/** A procedure card: files, commands and a finishing check for a family of similar tasks. */
export function isCard(r: Route): boolean {
  return r.step?.kind === "card";
}

/** The lifecycle as a chip says it: a quiet nascent reflex is "quiet". */
export function lifecycleWord(r: Route): string {
  return isQuiet(r) ? "quiet" : lifecycle(r.state);
}

/** The uplift bar a reflex must clear to be delivered (`learn.uplift_min`, an engine parameter). */
export const UPLIFT_MIN = 0.1;

/** A signed two-decimal number: +0.05, −0.02 (never −0.00). */
export function signed(x: number): string {
  const v = Math.abs(x) < 0.005 ? 0 : x;
  return `${v < 0 ? "−" : "+"}${Math.abs(v).toFixed(2)}`;
}

/**
 * A reflex's evidence in a few characters, by what its strength was decided on: `23/33` traces
 * took it; `~1.25 calls` saved (value); `3/4 passed` (a card's family); `instinct`.
 */
export function evidenceShort(r: Route): string {
  const e = r.evidence;
  if (isCard(r) && r.step.card) return `${r.step.card.passed}/${r.step.card.sessions} passed`;
  if (e.basis === "value" && e.why) return `~${e.why.gain.toFixed(2)} calls`;
  if (isInstinct(r) && !e.opportunities) return "instinct";
  return `${e.support}/${e.opportunities}`;
}

/** What that short form means, for a title or an accessible name. */
export function evidenceShortWhy(r: Route): string {
  const e = r.evidence;
  if (isCard(r) && r.step.card) return `from ${plural(r.step.card.sessions, "trace")} of its family, ${r.step.card.passed} passed`;
  if (e.basis === "value" && e.why) return `saves ~${e.why.gain.toFixed(2)} calls when followed (P(saves) ${e.why.conf.toFixed(2)})`;
  return `${e.support}/${e.opportunities} traces took it`;
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
    case "context":
      return "context";
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
    case "sequence":
      return "steps in order";
    case "card":
      return "procedure card";
    default:
      return s.kind.replace(/_/g, " ");
  }
}

/** A judgement's word: obligation_met is "met"; older spools' derailed / followed_failed are ignored / misfired. */
export function judgement(kind: string): string {
  switch (kind) {
    case "followed":
      return "followed";
    case "ignored":
    case "derailed":
      return "not observed within the window";
    case "obligation_met":
      return "met";
    case "misfired":
    case "followed_failed":
      return "misfired";
    default:
      return kind.replace(/_/g, " ");
  }
}

/** The arm's word (older spools called the control arm "holdout"). */
export function arm(a: string | null | undefined): string {
  if (a === "control" || a === "holdout") return "control";
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
  if (isCard(r)) return "Learned from earlier traces of a family of similar tasks in this repository.";
  if (r.evidence.basis === "value")
    return "Formed on value: the calls saved when the step is taken, estimated from traces that took it and those that did not.";
  return "Counted in past traces when Adage conditioned this reflex.";
}

// Savings (crates/adage-cli/src/savings.rs): the verdict's words and mark. Amounts stay in the
// default ink; only the verdict carries a mark, with the existing roles (style guide, Colour).

export function verdictWord(v: Savings["verdict"]): string {
  switch (v) {
    case "saved":
      return "saved";
    case "adds_cost":
      return "adds cost";
    case "no_difference":
      return "no measurable difference yet";
    default:
      return "measuring";
  }
}

/** Dollars as the CLI prints them: $0.034, $1.20, $140. */
export function usd(x: number): string {
  const a = Math.abs(x);
  if (a >= 100) return `$${a.toFixed(0)}`;
  if (a >= 1) return `$${a.toFixed(2)}`;
  return `$${a.toFixed(3)}`;
}

/** A PreToolUse record in words: what was changed (or advised) before the command ran. */
export function pretoolWords(p: Pretool | undefined, withheld: boolean): string {
  if (!p) return "";
  const at = p.interpreter ? ` at ${p.interpreter}` : "";
  if (p.rewrite) {
    const what = p.rewrite === "test" ? "test run" : p.rewrite === "repro" ? "reproduction run" : `${p.rewrite} run`;
    return withheld ? `would have pointed this ${what}${at} (control: left as it was)` : `pointed this ${what}${at}, the project's environment`;
  }
  if (p.advise) {
    const what = p.advise === "pip-install" ? "pip install into the system Python" : p.advise === "permissions" ? "a command your permission rules would not allow rewritten" : p.advise;
    return withheld ? `would have advised on ${what} (control)` : `advised on ${what}`;
  }
  return "";
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
