// Chapter: judge the evidence (crates/adage-learn/src/{engine,evidence,value}.rs,
// crates/adage-tracks/src/model.rs). The graph learner is the only learner: what it mines and
// compiles is counted again on the traces, exactly as each reflex's own trigger and step read them.
// Evidence is counted in distinct traces, a Wilson lower bound meets the fire bar, the step's effect
// caps the strength, and uplift decides whether telling the agent changes anything. The arc follows
// one finish line through the sample traces; the last step shows a recorded index, or this
// repository's own reflexes.

import type { Route } from "../../api/types";
import { strength, type Strength } from "../../lib/words";
import type { CamSpec, DotLook, SceneState, Stage, V3 } from "../engine";
import { PROTOS, TRACES, f2, laneX, laneZ, wilson, type Event } from "../sample";
import type { Built, Chapter, Context, Step } from "../types";

// ---------------------------------------------------------------------------------------- rules

// The learner's thresholds (adage-tracks params.rs `LearnParams::default` and `::demo`).
export const BARS = {
  default: { min_support: 5, fire_lower_bound: 0.8 },
  demo: { min_support: 3, fire_lower_bound: 0.5 },
};
const PRIME_RATE = 0.5; // yellow_rate: a new reflex primes at this rate
const KEEP_RATE = 0.3; // retain_rate: an existing one keeps its place down to this rate
const CONTRAST = 0.3; // finish_line_contrast
const DEAD_END = [9, 10] as const; // dead_end_consistency
const DEAD_END_WINDOW = 8; // mine/mod.rs `gap`: pattern items at most eight events apart
const LINKED_WINDOW = 16; // engine.rs `judge_read_batch`: reads within 16 steps of the lookup
const MIN_TASKS = 2; // engine.rs: support must span two distinct tasks
const LIFT_LB = 0.2; // params.rs OUTCOME_LIFT_LOWER_BOUND: the outcome-lift path, prime at most

// Uplift (adage-learn value.rs): p_follow − p_self, each a Beta posterior from its kind's prior.
export const UPLIFT_MIN = 0.1; // params.rs UPLIFT_MIN (learn.uplift_min; demo: 0)
export const UPLIFT_MIN_EVIDENCE = 3; // situations of the repository's own before a gate
const SELF_KAPPA = 4.0;
const FOLLOW_KAPPA = 11.8;
/** value.rs SELF_PRIORS and FOLLOW_PRIORS, by directive kind. */
export const PRIORS: Record<string, { self: number; follow: number }> = {
  avoid: { self: 0.29, follow: 0.944 },
  "run-after-edit": { self: 0.32, follow: 0.881 },
  "read-batch": { self: 0.93, follow: 0.986 },
  prompt: { self: 0.95, follow: 0.987 },
  obligation: { self: 0.88, follow: 0.975 },
  sequence: { self: 0.88, follow: 0.975 },
  advise: { self: 0.5, follow: 0.99 },
};

/** value.rs `uplift`: the posterior means of p_follow and p_self, their difference, and whether
 * the gate keeps the reflex quiet. `traces`/`took`: unexposed training traces in the situation;
 * `control`/`controlTook`: control-arm would-have judgements; `delivered`/`followed`: treated. */
export function uplift(kind: string, u: { traces: number; took: number; control?: number; controlTook?: number; delivered?: number; followed?: number }, bar = UPLIFT_MIN) {
  const pr = PRIORS[kind] ?? { self: 0.5, follow: 0.958 };
  const control = u.control ?? 0;
  const took = Math.min(u.took, u.traces) + Math.min(u.controlTook ?? 0, control);
  const n = u.traces + control;
  const delivered = u.delivered ?? 0;
  const followed = Math.min(u.followed ?? 0, delivered);
  const pSelf = (pr.self * SELF_KAPPA + took) / (SELF_KAPPA + n);
  const pFollow = (pr.follow * FOLLOW_KAPPA + followed) / (FOLLOW_KAPPA + delivered);
  const mean = pFollow - pSelf;
  return { pFollow, pSelf, mean, gated: bar > 0 && mean < bar && n >= UPLIFT_MIN_EVIDENCE };
}
const TICKET = /[A-Z][A-Z0-9]{0,9}-\d{1,7}/; // spans.rs ticket shape

type Effect = "observe" | "verify" | "regenerate" | "environment" | "author" | "publish";

/** The strongest a reflex with this effect may be (model.rs `ceiling`); null: never directed. */
export function ceiling(effect: Effect, byEdit: boolean): "fire" | "prime" | null {
  if (effect === "observe" || effect === "verify") return "fire";
  if (effect === "regenerate") return byEdit ? "fire" : "prime";
  if (effect === "environment") return "prime";
  return null;
}

/** evidence.rs `strength_for`: fire above the bar (if the ceiling allows), else prime above the rate bar. */
export function strengthFor(effect: Effect, byEdit: boolean, lb: number, rate: number, bar: number, existing = false): "fire" | "prime" | null {
  const max = ceiling(effect, byEdit);
  if (!max) return null;
  if (lb >= bar && max === "fire") return "fire";
  if (rate >= (existing ? KEEP_RATE : PRIME_RATE)) return "prime";
  return null;
}

/** The Wilson score upper bound, the other end of `wilson()`'s interval. */
function wilsonUpper(k: number, n: number, z = 1.959964): number {
  if (!n) return 1;
  const p = k / n;
  const d = 1 + (z * z) / n;
  const c = p + (z * z) / (2 * n);
  const r = z * Math.sqrt((p * (1 - p)) / n + (z * z) / (4 * n * n));
  return (c + r) / d;
}

/** An after-edit reflex judged on sample traces (engine.rs `count_after_edit`): traces that edited
 * the glob are the opportunities; support is those that ran exactly `command`, and succeeded, after
 * their last edit to it. The rate without the edit is its runs in the other traces. */
export function finishLine(traces: Event[][], edit: Event[0], command: string) {
  const editors: number[] = [];
  const support: number[] = [];
  const near: number[] = [];
  let ranWithout = 0;
  traces.forEach((tr, i) => {
    const last = tr.map((e) => e[0]).lastIndexOf(edit);
    if (last < 0) {
      if (tr.some((e) => e[1] === command)) ranWithout++;
      return;
    }
    editors.push(i);
    const after = tr.slice(last + 1);
    if (after.some((e) => e[1] === command)) support.push(i);
    else if (after.some((e) => e[0] === "G")) near.push(i);
  });
  const opp = editors.length;
  const without = traces.length - opp;
  const rate = opp ? support.length / opp : 0;
  const rateWithout = without ? ranWithout / without : 0;
  return { editors, support, near, opp, without, ranWithout, rate, rateWithout, lb: wilson(support.length, opp) };
}

/** A dead end's first gates on sample traces: a failed verify run followed within the mining gap by
 * a different verify command that works, and how consistently the runner failed. */
export function deadEnd(traces: Event[][]) {
  const pairs: [number, number, number][] = [];
  const ran = new Set<number>();
  const passed = new Set<number>();
  traces.forEach((tr, i) =>
    tr.forEach(([code], j) => {
      if (code !== "F") return;
      ran.add(i);
      const k = tr.slice(j + 1, j + 1 + DEAD_END_WINDOW).findIndex((e) => e[0] === "C");
      if (k >= 0) pairs.push([i, j, j + 1 + k]);
    }),
  );
  // A sample `npm test` never passes; `passed` stays empty, as the rule counts it.
  const n = ran.size;
  const failedIn = n - passed.size;
  const consistent = n > 0 && failedIn * DEAD_END[1] >= n * DEAD_END[0];
  const support = new Set(pairs.map((p) => p[0])).size;
  return { pairs, ran: n, failedIn, consistent, support };
}

// ---------------------------------------------------------------------------------------- data

// A recorded index: the demo dataset's reflexes after its first round, with the demo thresholds
// (evals/datasets/demo-haiku45/recorded/index-round2.json).
interface Formed {
  id: string;
  strength: string;
  effect: Effect;
  support: number;
  opportunities: number;
  lower_bound: number;
  what: string;
}
const RECORDED: Formed[] = [
  { id: "lookup-work-item", strength: "fire", effect: "observe", support: 23, opportunities: 33, lower_bound: 0.5266186471733927, what: "python3 scripts/wi-lookup {item}" },
  { id: "read-linked-files", strength: "prime", effect: "observe", support: 15, opportunities: 16, lower_bound: 0.7167126220185179, what: "read the linked files together" },
  { id: "dead-end-d41b1e", strength: "inhibit", effect: "verify", support: 11, opportunities: 43, lower_bound: 0.14929517058883945, what: "avoid pytest / unittest → make test" },
  { id: "finish-line-e7e809", strength: "fire", effect: "regenerate", support: 7, opportunities: 7, lower_bound: 0.6456695613241317, what: "after schema/*.json → make gen" },
];

const CAM: Record<"lanes" | "gauge" | "lid" | "board", CamSpec> = {
  lanes: { target: [-0.6, -1, 0.6], yaw: -0.2, pitch: 0.8, dist: 25 },
  gauge: { target: [0, 0.6, 0], yaw: -0.06, pitch: 0.1, dist: 19 },
  lid: { target: [0, 0.2, 0], yaw: 0.1, pitch: 0.14, dist: 18.5 },
  board: { target: [0, 0.8, 0], yaw: 0.08, pitch: 0.12, dist: 21 },
};

const LAMP: Record<Strength, "fire" | "prime" | "inhibit"> = { fire: "fire", prime: "prime", inhibit: "inhibit" };

// ---------------------------------------------------------------------------------------- build

function build(stage: Stage, ctx: Context): Built {
  const T = TRACES;
  const CMD = "make gen";
  const fl = finishLine(T, "E", CMD);
  const k = fl.support.length;
  const bar = BARS.default.fire_lower_bound;
  const verdict = strengthFor("regenerate", true, fl.lb, fl.rate, bar);
  const verdictDemo = strengthFor("regenerate", true, fl.lb, fl.rate, BARS.demo.fire_lower_bound);
  // What telling changes: the finish line is an obligation; its unexposed traces are the editors.
  const up = uplift("obligation", { traces: fl.opp, took: k });
  const de = deadEnd(T);
  const tickets = T.map((tr) => tr[0]![1].match(TICKET)?.[0]).filter(Boolean) as string[];
  const ticketUsed = T.some((tr) => tr.slice(1).some((e) => tickets.some((t) => e[1].includes(t))));

  // The interval as traces add up, in trace order.
  const cum = fl.editors.map((_, n) => {
    const kk = fl.editors.slice(0, n + 1).filter((i) => fl.support.includes(i)).length;
    return { k: kk, n: n + 1, lb: wilson(kk, n + 1), ub: wilsonUpper(kk, n + 1) };
  });

  // Rows.
  T.forEach((tr, i) => tr.forEach((_, j) => stage.dot(`n${i}.${j}`)));
  const lanePairs: [string, string][] = [];
  T.forEach((tr, i) => tr.forEach((_, j) => j && lanePairs.push([`n${i}.${j - 1}`, `n${i}.${j}`])));
  stage.link("lane", lanePairs, "ink3");
  const lastE = (i: number) => T[i]!.map((e) => e[0]).lastIndexOf("E");
  const genAt = (i: number) => T[i]!.findIndex((e, j) => j > lastE(i) && e[1] === CMD);
  stage.link(
    "eg",
    fl.support.map((i) => [`n${i}.${lastE(i)}`, `n${i}.${genAt(i)}`] as [string, string]),
    "accent",
  );
  stage.link(
    "de",
    de.pairs.map(([i, a, b]) => [`n${i}.${a}`, `n${i}.${b}`] as [string, string]),
    "accent",
  );

  // The gauge: one column per opportunity counted so far.
  const gx = (n: number) => -5.6 + ((n - 1) * 10.8) / Math.max(1, cum.length - 1);
  const gy = (v: number) => -3 + v * 7;
  cum.forEach((_, n) => {
    stage.dot(`lb${n}`, 0.16);
    stage.dot(`ub${n}`, 0.1);
    stage.dot(`rt${n}`, 0.12);
  });
  stage.link(
    "iv",
    cum.map((_, n) => [`lb${n}`, `ub${n}`] as [string, string]),
    "ink2",
  );
  stage.link("fireBar", [[[-7.4, gy(bar), 0], [7.6, gy(bar), 0]]], "fire");
  stage.link("primeLine", [[[-7.4, gy(PRIME_RATE), 0], [7.6, gy(PRIME_RATE), 0]]], "prime");
  stage.link("axis", [[[-7.4, gy(0), 0], [7.6, gy(0), 0]]], "ink3");
  stage.dot("lamp", 0.34);
  const LAMP_AT: V3 = [gx(cum.length) + 0.1, gy(fl.lb), 0];

  // Effect ceilings: one column per effect (regenerate twice: after an edit, and otherwise).
  const COLS: { effect: Effect; byEdit: boolean; name: string }[] = [
    { effect: "observe", byEdit: false, name: "observe" },
    { effect: "verify", byEdit: false, name: "verify" },
    { effect: "regenerate", byEdit: true, name: "regenerate<br>after an edit" },
    { effect: "regenerate", byEdit: false, name: "regenerate<br>otherwise" },
    { effect: "environment", byEdit: false, name: "environment" },
    { effect: "author", byEdit: false, name: "author" },
    { effect: "publish", byEdit: false, name: "publish" },
  ];
  const LEVEL = { none: -2.4, prime: 0, fire: 2.4 };
  const cx = (c: number) => -6.3 + (c * 12.6) / (COLS.length - 1);
  COLS.forEach((_, c) => stage.dot(`cap${c}`, 0.24));
  stage.link(
    "lid",
    COLS.map((col, c) => {
      const y = LEVEL[ceiling(col.effect, col.byEdit) ?? "none"] + 0.42;
      return [[cx(c) - 0.7, y, 0], [cx(c) + 0.7, y, 0]] as [V3, V3];
    }),
    "ink1",
  );
  stage.link("floor", [[[-7.2, LEVEL.none - 0.3, 0], [7.2, LEVEL.none - 0.3, 0]]], "ink3");

  // The board: recorded reflexes, or this repository's.
  const repoRoutes: Route[] = ctx.repo && ctx.graph ? ctx.graph.routes.filter((r) => strength(r.strength)) : [];
  const useRepo = repoRoutes.length > 0;
  const boardBar = useRepo ? (ctx.graph?.bars?.fire_lower_bound ?? bar) : BARS.demo.fire_lower_bound;
  const boardMin = useRepo ? (ctx.graph?.bars?.min_support ?? BARS.default.min_support) : BARS.demo.min_support;
  type Board = { id: string; lamp: Strength; lb: number; rate: number; support: number; opp: number; what: string };
  const board: Board[] = useRepo
    ? repoRoutes
        .map((r) => ({
          id: r.id,
          lamp: strength(r.strength)!,
          lb: r.evidence.lower_bound,
          rate: r.evidence.rate ?? (r.evidence.opportunities ? r.evidence.support / r.evidence.opportunities : 0),
          support: r.evidence.support,
          opp: r.evidence.opportunities,
          what: r.step.label,
        }))
        .sort((a, b) => b.lb - a.lb)
        .slice(0, 24)
    : RECORDED.map((r) => ({
        id: r.id,
        lamp: strength(r.strength)!,
        lb: wilson(r.support, r.opportunities),
        rate: r.support / r.opportunities,
        support: r.support,
        opp: r.opportunities,
        what: r.what,
      }));
  const bx = (i: number) => (board.length === 1 ? 0 : -6.6 + (i * 13.2) / (board.length - 1));
  const by = (v: number) => -3 + v * 6.4;
  board.forEach((_, i) => {
    stage.dot(`bl${i}`, 0.3);
    stage.dot(`br${i}`, 0.1);
  });
  stage.link(
    "bstem",
    board.map((_, i) => [`bl${i}`, `br${i}`] as [string, string]),
    "ink3",
  );
  stage.link("bbar", [[[-7.6, by(boardBar), 0], [7.8, by(boardBar), 0]]], "fire");
  stage.link("baxis", [[[-7.6, by(0), 0], [7.8, by(0), 0]]], "ink3");
  const labelled = board.length <= 8 ? board.map((_, i) => i) : board.map((_, i) => i).filter((i) => i < 4 || board[i]!.lamp === "inhibit").slice(0, 6);

  // Labels.
  T.forEach((_, i) => stage.label(`t${i}`, `T${i + 1}`, `n${i}.0`, { dx: -34, dy: -7 }));
  const firstG = fl.support[0]!;
  const firstF = de.pairs[0]!;
  const firstR = T.findIndex((tr) => tr.some((e) => e[0] === "R"));
  const firstD = T.findIndex((tr) => tr[0]![0] === "D");
  stage.label("mFinish", "finish line", `n${firstG}.${genAt(firstG)}`, { cls: "chip", dy: -30, center: true });
  stage.label("mDead", "dead ends", `n${firstF[0]}.${firstF[1]}`, { cls: "chip", dy: -30, center: true });
  stage.label("mLinked", "linked files", `n${firstR}.${T[firstR]!.findIndex((e) => e[0] === "R")}`, { cls: "chip", dy: -30, center: true });
  stage.label("mLookup", "lookup", `n0.0`, { cls: "chip", dy: -30, center: true });
  stage.label("mIntent", "intent", `n${firstD}.0`, { cls: "chip", dy: -30, center: true });
  const counter: V3 = [laneX(2), 0, laneZ(0) - 1.4];
  stage.label("count", `<small>${CMD.toUpperCase()} AFTER THE LAST registry/*.csv EDIT</small>${k} / ${fl.opp} traces`, counter, { cls: "chip big", dy: -44, center: true });
  for (const i of fl.near) stage.label(`near${i}`, `${T[i]!.find((e, j) => j > lastE(i) && e[0] === "G")![1]}: another command`, `n${i}.${T[i]!.findIndex((e, j) => j > lastE(i) && e[0] === "G")}`, { cls: "note", dx: 12, dy: 6 });
  const noGen = fl.editors.filter((i) => !fl.support.includes(i) && !fl.near.includes(i));
  for (const i of noGen) stage.label(`nogen${i}`, "edited, no regenerate step", `n${i}.${lastE(i)}`, { cls: "note", dx: 12, dy: 6 });
  stage.label(
    "without",
    `<small>WITHOUT THE EDIT</small>${CMD} in ${fl.ranWithout} / ${fl.without} traces`,
    [laneX(2), 0, laneZ(T.length - 1) + 1.2],
    { cls: "chip big", dy: 8, center: true },
  );
  stage.label("fireBar", `fire bar ${f2(bar)}`, [-7.4, gy(bar), 0], { dy: -18 });
  stage.label("primeLine", `prime rate ${f2(PRIME_RATE)}`, [-7.4, gy(PRIME_RATE), 0], { dy: -18 });
  stage.label("n1", "1 trace", [gx(1), gy(0), 0], { dy: 8, center: true });
  stage.label("nN", `${cum.length} traces`, [gx(cum.length), gy(0), 0], { dy: 8, center: true });
  stage.label("lbN", `lower bound ${f2(fl.lb)}`, `lb${cum.length - 1}`, { cls: "note", dx: 14, dy: -7 });
  stage.label("rtN", `rate ${f2(fl.rate)}`, `rt${cum.length - 1}`, { cls: "note", dx: 14, dy: -7 });
  stage.label(
    "reflex",
    `<small>AFTER EDIT registry/*.csv · RUN ${CMD}</small><span class="how-lamp ${verdict ?? ""}">●</span> ${verdict ?? "no reflex"}`,
    "lamp",
    { cls: "chip big", dx: -190, dy: 28, center: true },
  );
  COLS.forEach((col, c) => {
    const cap = ceiling(col.effect, col.byEdit);
    const note = col.effect === "environment" ? "starts nascent" : cap ? "" : "never";
    stage.label(`cn${c}`, `${col.name}${note ? `<br><span class="how-dim">${note}</span>` : ""}`, [cx(c), LEVEL.none - 0.3, 0], { dy: 10, center: true });
  });
  stage.label("lvFire", "fire", [7.2, LEVEL.fire, 0], { dx: 8, dy: -7 });
  stage.label("lvPrime", "prime", [7.2, LEVEL.prime, 0], { dx: 8, dy: -7 });
  const fAt = de.pairs.map(([i, a]) => `n${i}.${a}`);
  stage.label("deNote", `npm test ✗ → make check within ${DEAD_END_WINDOW} events`, fAt[0]!, { cls: "note", dx: 14, dy: 6 });
  stage.label(
    "deGate",
    `<small>DEAD END · WOULD INHIBIT</small>failed in ${de.failedIn} / ${de.ran} traces that ran it · support ${de.support}`,
    [laneX(2), 0, laneZ(T.length - 1) + 1.2],
    { cls: "chip big", dy: 8, center: true },
  );
  board.forEach((b, i) => {
    stage.label(`bl${i}`, `<span class="how-lamp ${LAMP[b.lamp]}">●</span> ${b.id}<br><span class="how-dim">${b.support}/${b.opp} · LB ${f2(b.lb)}</span>`, `bl${i}`, {
      cls: "chip",
      dy: i % 2 ? 18 : -54,
      center: true,
    });
  });
  stage.label("bbar", `fire bar ${f2(boardBar)}${useRepo ? "" : " (demo)"}`, [-7.6, by(boardBar), 0], { dy: -18 });

  // ------------------------------------------------------------------ states

  function lanes(s: number): Record<string, DotLook> {
    const dots: Record<string, DotLook> = {};
    T.forEach((tr, i) =>
      tr.forEach(([code, exact], j) => {
        const id = `n${i}.${j}`;
        const base: DotLook = { pos: [laneX(j), 0, laneZ(i)], color: { hue: PROTOS[code].hue }, alpha: 0.85, glow: 0.14 };
        let look = base;
        if (s === 0) {
          const candidate =
            (j === genAt(i) && fl.support.includes(i)) || code === "F" || code === "R" || (j === 0 && (tickets.some((t) => exact.includes(t)) || code === "D"));
          look = candidate ? { ...base, scale: 1.25, glow: 0.45 } : { ...base, alpha: 0.4, glow: 0 };
        } else if (s === 1 || s === 2) {
          const editor = fl.editors.includes(i);
          const sup = fl.support.includes(i);
          const isE = j === lastE(i);
          const isG = sup && j === genAt(i);
          if (s === 1 && editor && (isE || isG)) look = { ...base, color: isG ? "soft" : "accent", scale: 1.45, glow: 0.8 };
          else if (s === 1 && editor && fl.near.includes(i) && code === "G") look = { ...base, color: "ink2", scale: 1.2, glow: 0.3 };
          else if (s === 2 && !editor) look = { ...base, color: "ink2", scale: 1.15, glow: 0.3 };
          else if (s === 2 && (isE || isG)) look = { ...base, color: isG ? "soft" : "accent", alpha: 0.5, glow: 0.1 };
          else look = { ...base, alpha: 0.2, glow: 0 };
        } else if (s === 6) {
          const pair = de.pairs.find(([i2, a, b]) => i2 === i && (j === a || j === b));
          if (pair) look = { ...base, color: j === pair[1] ? "ink1" : "soft", scale: 1.5, glow: 0.8 };
          else look = { ...base, alpha: 0.2, glow: 0 };
        }
        dots[id] = look;
      }),
    );
    return dots;
  }

  function state(s: number): SceneState {
    let dots: Record<string, DotLook> = {};
    const labels: string[] = [];
    const links: Record<string, number> = {};
    let cam = CAM.lanes;
    if (s <= 2 || s === 6) {
      dots = lanes(s);
      labels.push(...T.map((_, i) => `t${i}`));
      links.lane = s === 0 ? 0.45 : 0.25;
      if (s === 0) labels.push("mFinish", "mDead", "mLinked", "mLookup", "mIntent");
      if (s === 1) labels.push("count", ...fl.near.map((i) => `near${i}`), ...noGen.map((i) => `nogen${i}`));
      if (s === 2) labels.push("count", "without");
      if (s === 1 || s === 2) links.eg = s === 1 ? 0.95 : 0.35;
      if (s === 6) {
        labels.push("deNote", "deGate");
        links.de = 0.95;
      }
    } else if (s === 3 || s === 4) {
      cum.forEach((c, n) => {
        const last = n === cum.length - 1;
        const dim = s === 4 && !last;
        const a = dim ? 0.3 : 1;
        dots[`lb${n}`] = { pos: [gx(n + 1), gy(c.lb), 0], color: "soft", alpha: a, glow: dim ? 0 : 0.5, scale: last ? 1.3 : 1 };
        dots[`ub${n}`] = { pos: [gx(n + 1), gy(c.ub), 0], color: "ink3", alpha: a, glow: 0 };
        dots[`rt${n}`] = { pos: [gx(n + 1), gy(c.k / c.n), 0], color: "ink1", alpha: a, glow: dim ? 0 : 0.2 };
      });
      if (s === 4 && verdict) dots.lamp = { pos: LAMP_AT, color: verdict, scale: 1, glow: 1 };
      links.iv = s === 3 ? 0.8 : 0.4;
      links.fireBar = 0.9;
      links.primeLine = 0.9;
      links.axis = 0.5;
      labels.push("fireBar", "primeLine", "n1", "nN");
      if (s === 3) labels.push("lbN", "rtN");
      if (s === 4) labels.push("reflex");
      cam = CAM.gauge;
    } else if (s === 5) {
      COLS.forEach((col, c) => {
        const cap = ceiling(col.effect, col.byEdit);
        dots[`cap${c}`] = { pos: [cx(c), LEVEL[cap ?? "none"], 0], color: cap ?? "ink3", glow: cap ? 0.8 : 0.1, alpha: cap ? 1 : 0.6 };
      });
      links.lid = 0.9;
      links.floor = 0.5;
      labels.push(...COLS.map((_, c) => `cn${c}`), "lvFire", "lvPrime");
      cam = CAM.lid;
    } else {
      board.forEach((b, i) => {
        dots[`bl${i}`] = { pos: [bx(i), by(b.lb), 0], color: b.lamp, glow: 0.9, scale: board.length > 12 ? 0.7 : 1 };
        dots[`br${i}`] = { pos: [bx(i), by(b.rate), 0], color: "ink3", glow: 0, alpha: 0.8 };
      });
      links.bstem = 0.6;
      links.bbar = 0.9;
      links.baxis = 0.5;
      labels.push(...labelled.map((i) => `bl${i}`), "bbar");
      cam = CAM.board;
    }
    return { dots, links, labels, cam };
  }

  const ivFigure: Step["figure"] = (g, w, h, pal) => {
    const padB = 14;
    const padL = 34;
    const X = (n: number) => padL + ((n - 1) / Math.max(1, cum.length - 1)) * (w - padL - 8);
    const Y = (v: number) => h - padB - v * (h - padB - 6);
    g.font = "10px 'Red Hat Mono', ui-monospace, monospace";
    // The interval band.
    g.beginPath();
    cum.forEach((c, i) => (i ? g.lineTo(X(c.n), Y(c.ub)) : g.moveTo(X(c.n), Y(c.ub))));
    [...cum].reverse().forEach((c) => g.lineTo(X(c.n), Y(c.lb)));
    g.closePath();
    g.fillStyle = pal.css("soft") + "33";
    g.fill();
    const line = (vals: number[], color: string, width: number) => {
      g.beginPath();
      vals.forEach((v, i) => (i ? g.lineTo(X(i + 1), Y(v)) : g.moveTo(X(i + 1), Y(v))));
      g.strokeStyle = color;
      g.lineWidth = width;
      g.stroke();
    };
    line(
      cum.map((c) => c.lb),
      pal.css("soft"),
      1.5,
    );
    line(
      cum.map((c) => c.k / c.n),
      pal.css("ink2"),
      1,
    );
    const dash = (v: number, color: string, label: string) => {
      g.setLineDash([3, 3]);
      g.strokeStyle = color;
      g.lineWidth = 1;
      g.beginPath();
      g.moveTo(padL, Y(v));
      g.lineTo(w - 4, Y(v));
      g.stroke();
      g.setLineDash([]);
      g.fillStyle = color;
      g.textAlign = "right";
      g.textBaseline = "middle";
      g.fillText(label, padL - 4, Y(v));
    };
    dash(bar, pal.css("fire"), f2(bar));
    dash(PRIME_RATE, pal.css("prime"), f2(PRIME_RATE));
    g.fillStyle = pal.css("ink3");
    g.textBaseline = "top";
    g.textAlign = "left";
    g.fillText("1 trace", X(1), h - padB + 2);
    g.textAlign = "right";
    g.fillText(`${cum.length} traces`, X(cum.length), h - padB + 2);
  };

  const upFigure: Step["figure"] = (g, w, h, pal) => {
    const rows: [string, number, string][] = [
      ["told (p_follow)", up.pFollow, pal.css("fire")],
      ["unaided (p_self)", up.pSelf, pal.css("ink2")],
    ];
    const left = 104;
    const X = (v: number) => left + v * (w - left - 44);
    const rowH = (h - 16) / rows.length;
    g.font = "10px 'Red Hat Mono', ui-monospace, monospace";
    g.textBaseline = "middle";
    rows.forEach(([label, v, color], i) => {
      const y = 4 + i * rowH + rowH / 2;
      g.fillStyle = pal.css("ink2");
      g.textAlign = "left";
      g.fillText(label, 0, y);
      g.fillStyle = color;
      g.fillRect(X(0), y - 4, X(v) - X(0), 8);
      g.fillStyle = pal.css("ink1");
      g.fillText(f2(v), X(v) + 5, y);
    });
    g.fillStyle = pal.css("ink3");
    g.textAlign = "left";
    g.textBaseline = "bottom";
    g.fillText(`uplift ${up.mean >= 0 ? "+" : ""}${f2(up.mean)} · bar +${f2(UPLIFT_MIN)}`, left, h);
  };

  const gate = (ok: boolean, text: string) => ({ text, ok });
  const text: Omit<Step, "state">[] = [
    {
      title: "Every reflex is judged again",
      body: `Mining and compiling propose; before a reflex is written, its evidence is counted again on the traces, exactly as its own trigger and step read them: a lookup through the value type of the ticket it names, linked files over the traces that ran the lookup, a finish line over the traces that edited its files, a dead end over the traces that ran the failing runner. A trace a directive already steered toward the step is left out. ${tickets.length ? `Only ${tickets.length} sample prompt names a ticket-shaped token (${tickets.join(", ")}), and ${ticketUsed ? "a command uses it" : "no command uses it"}, so no lookup forms here.` : "No sample prompt names a ticket, so no lookup forms here."}`,
      chips: [{ text: "one learner: the graph engine" }, { text: "evidence in distinct traces" }, { text: `linked files: reads within ${LINKED_WINDOW} steps` }],
    },
    {
      title: "Finish line: count the traces",
      body: `A reflex after an edit asks: after the last edit to registry/*.csv, did the trace run the same regenerate command, and succeed? ${k} of the ${fl.opp} traces that edited those files ran ${CMD}. A different command, even a variant, does not count, and the supporting traces must span at least ${MIN_TASKS} distinct tasks.`,
      chips: [{ text: `support ${k} ≥ ${BARS.default.min_support}`, ok: k >= BARS.default.min_support }, { text: `opportunities ${fl.opp}` }, { text: `rate ${f2(fl.rate)}` }, { text: `≥ ${MIN_TASKS} tasks` }],
    },
    {
      title: "Would it happen anyway?",
      body: `A finish line must belong to the edit. In the ${fl.without} traces without it, ${CMD} ran ${fl.ranWithout} times, so the rate rises by ${f2(fl.rate - fl.rateWithout)} with the edit. It must rise by at least ${f2(CONTRAST)}.`,
      chips: [gate(fl.rate - fl.rateWithout >= CONTRAST, `contrast ${f2(fl.rate - fl.rateWithout)} ≥ ${f2(CONTRAST)}`), { text: `${f2(fl.rate)} with · ${f2(fl.rateWithout)} without` }],
    },
    {
      title: "The lower bound grows slowly",
      body: `A rate from few traces is uncertain, so strength is judged by the Wilson lower bound at 95%, not the rate. Each column adds one trace: the interval narrows, but after ${fl.opp} traces the lower bound is ${f2(fl.lb)}, well under the fire bar of ${f2(bar)}.`,
      chips: [{ text: "Wilson, z = 1.96" }, { text: `${k}/${fl.opp} → LB ${f2(fl.lb)}` }, { text: `fire bar ${f2(bar)}` }],
      figure: ivFigure,
    },
    {
      title: verdict ? `It ${verdict}s` : "No reflex yet",
      body: `A reflex fires when its lower bound reaches the fire bar and its effect allows it. Otherwise it primes if its rate reaches ${f2(PRIME_RATE)}, or ${f2(KEEP_RATE)} for a reflex that already exists, so reflexes do not flicker. A step too few traces took can still prime when the traces that took it succeeded more often than those that did not (the 95% lower bound on the difference at least ${f2(LIFT_LB)}). Here the lower bound is ${f2(fl.lb)} and the rate ${f2(fl.rate)}: ${verdict ?? "no reflex"}. With the demo thresholds (fire bar ${f2(BARS.demo.fire_lower_bound)}), it would ${verdictDemo ?? "not form"} too.`,
      chips: [
        gate(fl.lb >= bar, `LB ${f2(fl.lb)} ≥ ${f2(bar)}`),
        gate(fl.rate >= PRIME_RATE, `rate ${f2(fl.rate)} ≥ ${f2(PRIME_RATE)}`),
        { text: `keep ≥ ${f2(KEEP_RATE)}` },
        { text: `or outcome lift ≥ ${f2(LIFT_LB)} → prime` },
      ],
    },
    {
      title: "Does telling change anything?",
      body: `Agents taking a step is not evidence that telling them helps. Uplift is how much more often agents take the step when told (p_follow) than unaided (p_self), each a Beta posterior that starts from its kind's prior and is updated by this repository's traces: unexposed traces and control-arm traces for p_self, treated judgements for p_follow. A finish line starts at p_self ${f2(PRIORS.obligation!.self)}; here ${k} of ${fl.opp} editing traces ran ${CMD} unaided, so p_self is ${f2(up.pSelf)} and uplift ${up.mean >= 0 ? "+" : ""}${f2(up.mean)}: ${up.gated ? "under the bar, so it stays quiet" : "above the bar, so it is delivered"}. A reflex under +${f2(UPLIFT_MIN)}, with at least ${UPLIFT_MIN_EVIDENCE} situations of the repository's own evidence, stays quiet: nascent, listed in adage review as quiet, until a person approves it.`,
      chips: [
        gate(!up.gated, `uplift ${up.mean >= 0 ? "+" : ""}${f2(up.mean)} ≥ +${f2(UPLIFT_MIN)}`),
        { text: `p_follow ${f2(up.pFollow)} · p_self ${f2(up.pSelf)}` },
        { text: "quiet: nascent until approved" },
        { text: "demo thresholds and procedure cards: not gated" },
      ],
      figure: upFigure,
    },
    {
      title: "The effect sets a ceiling",
      body: "What a step does limits how strongly Adage may direct it. Reads and checks can fire. A regenerate step fires only after an edit. An install primes at most, and starts nascent until a person reviews it. Edits and publishing are never directed.",
      chips: [{ text: "observe, verify: fire" }, { text: "regenerate: fire after an edit" }, { text: "environment: prime" }, { text: "author, publish: none" }],
    },
    {
      title: "Dead ends: fail, then recover",
      body: `A failed check followed within ${DEAD_END_WINDOW} events by a different check that works is a dead-end pair. The runner must fail in at least ${DEAD_END[0]} of ${DEAD_END[1]} traces that ran it; npm test failed in ${de.failedIn} of ${de.ran}. But only ${de.support} traces show the pair, under the minimum of ${BARS.default.min_support}, so no reflex forms. A dead end inhibits: announced at session start when most traces run the failing runner, otherwise said right after the failure.`,
      chips: [
        gate(de.consistent, `failed ${de.failedIn}/${de.ran} ≥ ${DEAD_END[0]}/${DEAD_END[1]}`),
        gate(de.support >= BARS.default.min_support, `support ${de.support} ≥ ${BARS.default.min_support}`),
      ],
    },
    useRepo
      ? {
          title: "Your reflexes against the bar",
          body: `Each column is one of this repository's reflexes: its lower bound (the lamp) below its rate. A lamp above the fire bar of ${f2(boardBar)} may fire; the ceiling or the rules explain the rest. Inhibit comes from the dead-end rule, not the bar.`,
          chips: [{ text: `${board.length} reflexes${repoRoutes.length > board.length ? ` of ${repoRoutes.length}` : ""}` }, { text: `min support ${boardMin}` }, { text: `fire bar ${f2(boardBar)}` }],
        }
      : {
          title: "A recorded index",
          body: `The demo dataset's recorded index, conditioned with the demo thresholds (fire bar ${f2(boardBar)}, min support ${boardMin}). The lookup fires at ${f2(board[0]!.lb)}. Linked files sit higher, at ${f2(board[1]!.lb)}, yet prime: a read is only ever suggested. The dead end inhibits whatever its bound.`,
          chips: board.map((b) => ({ text: `${b.lamp} ${b.support}/${b.opp}` })),
        },
  ];

  // The uplift step keeps the gauge's scene (step 4); the rest follow in order.
  const SCENE = [0, 1, 2, 3, 4, 4, 5, 6, 7];
  return {
    source: useRepo ? `${T.length} sample traces · your ${board.length} reflexes` : `${T.length} sample traces · 1 recorded index`,
    legend: [
      { label: "fire", role: "fire" },
      { label: "prime", role: "prime" },
      { label: "inhibit", role: "inhibit" },
      { label: "edit", role: "accent" },
      { label: "counted step", role: "soft" },
    ],
    steps: text.map((t, i) => ({ ...t, state: state(SCENE[i]!) })),
  };
}

export const judge: Chapter = {
  id: "judge",
  title: "Judge the evidence",
  status: "Built · default",
  kind: "built",
  summary:
    "Every reflex the graph learner compiles is counted again on the traces, in distinct traces, with a Wilson lower bound against the fire bar and its effect as a ceiling. Reflexes that telling the agent would not change (uplift under +0.1) stay quiet until a person approves them.",
  source: "crates/adage-learn/src/{engine,evidence,value}.rs · crates/adage-tracks/src/model.rs",
  usesRepo: true,
  build,
};

export const _recorded = RECORDED;
