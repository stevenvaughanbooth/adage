// Chapter: measure against control (evals/live, demo/report.py on the rebrand branch; the corpus
// in evals/corpus). Eight sample tasks run in both arms; the difference is measured within each
// task; arms are shuffled within tasks to build the null; Holm adjusts across metrics; a task
// bootstrap and a sign-flip test ask how far the effect generalises. Every statistic on the sample
// is computed here, seeded. Then the live corpus's published results, and in "This repository" the
// map's own treated and control counts, with their denominators.

import type { Graph, Route } from "../../api/types";
import { kpis, pct, share } from "../../lib/evidence";
import type { CamSpec, DotLook, Palette, SceneState, Stage, V3 } from "../engine";
import type { Built, Chapter, Step } from "../types";

// ------------------------------------------------------------------ constants from the code

// evals/live/README.md#statistics (demo/report.py): permutations, bootstrap resamples, seeds.
const N_PERM = 10_000;
const N_BOOT = 2_000;
const PERM_SEED = 7;
const BOOT_SEED = 11;
const ALPHA = 0.05;
// src/views/Evidence.tsx: traces before a share is stated.
const MIN_TREATED = 10;
const MIN_CONTROL = 5;

// ------------------------------------------------------------------ sample tasks (illustrative)

interface Run {
  turns: number;
  cost: number;
  passed: number;
}
interface Task {
  control: Run[];
  treated: Run[];
}

// Eight tasks, two runs per arm, turns and pass. Cost follows turns, as tokens do.
const RAW: [number[], number[], number[], number[]][] = [
  // control turns, treated turns, control passed, treated passed
  [[24, 31], [12, 14], [1, 0], [1, 1]],
  [[28, 22], [15, 19], [1, 1], [1, 1]],
  [[19, 25], [18, 13], [0, 1], [1, 0]],
  [[30, 34], [11, 16], [0, 0], [1, 0]],
  [[21, 17], [20, 22], [1, 1], [1, 1]],
  [[26, 29], [14, 12], [1, 0], [0, 1]],
  [[23, 27], [17, 25], [1, 1], [1, 1]],
  [[33, 28], [13, 18], [0, 1], [1, 1]],
];
const cost = (turns: number, t: number, r: number) => Math.round((0.0042 * turns + 0.004 * ((t * 3 + r * 5) % 4)) * 1000) / 1000;
export const TASKS: Task[] = RAW.map(([ct, tt, cp, tp], t) => ({
  control: ct.map((turns, r) => ({ turns, cost: cost(turns, t, r), passed: cp[r]! })),
  treated: tt.map((turns, r) => ({ turns, cost: cost(turns, t, 2 + r), passed: tp[r]! })),
}));

type Metric = "turns" | "cost" | "passed";
const METRICS: { key: Metric; label: string; agg: "median" | "mean" }[] = [
  { key: "turns", label: "turns", agg: "median" },
  { key: "cost", label: "cost", agg: "mean" },
  { key: "passed", label: "pass rate", agg: "mean" },
];

// ------------------------------------------------------------------ statistics (demo/report.py)

/** A seeded uniform generator on [0, 1) (mulberry32). */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;
export function median(xs: number[]): number {
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length % 2 ? s[m]! : (s[m - 1]! + s[m]!) / 2;
}
const agg = (k: "median" | "mean") => (k === "median" ? median : mean);

type Groups = [number[], number[]][];
const groupsOf = (m: Metric): Groups => TASKS.map((t) => [t.treated.map((r) => r[m]), t.control.map((r) => r[m])]);

/** The test statistic: the mean over tasks of (task mean treated − task mean control). */
export function taskMeanDiff(g: Groups): number {
  return mean(g.map(([a, b]) => mean(a) - mean(b)));
}

function shuffle<T>(xs: T[], r: () => number): void {
  for (let i = xs.length - 1; i > 0; i--) {
    const j = Math.floor(r() * (i + 1));
    [xs[i], xs[j]] = [xs[j]!, xs[i]!];
  }
}

/** Two-sided permutation test, arms shuffled within each task; also every permuted statistic. */
export function permTest(g: Groups, n = N_PERM, seed = PERM_SEED): { p: number; obs: number; null: number[]; first: Groups } {
  const obs = taskMeanDiff(g);
  const r = rng(seed);
  const work = g.map(([a, b]) => ({ k: a.length, vals: [...a, ...b] }));
  const nul: number[] = [];
  let hits = 0;
  let first: Groups = [];
  for (let i = 0; i < n; i++) {
    for (const w of work) shuffle(w.vals, r);
    const perm: Groups = work.map((w) => [w.vals.slice(0, w.k), w.vals.slice(w.k)]);
    if (i === 0) first = perm.map(([a, b]) => [[...a], [...b]]);
    const d = taskMeanDiff(perm);
    nul.push(d);
    if (Math.abs(d) >= Math.abs(obs) - 1e-12) hits++;
  }
  return { p: (hits + 1) / (n + 1), obs, null: nul, first };
}

/** Exact two-sided sign-flip test over the per-task differences. */
export function signFlip(g: Groups): number {
  const d = g.map(([a, b]) => mean(a) - mean(b));
  const obs = Math.abs(d.reduce((x, y) => x + y, 0)) - 1e-12;
  let hits = 0;
  for (let mask = 0; mask < 1 << d.length; mask++) {
    const s = d.reduce((acc, v, i) => acc + ((mask >> i) & 1 ? -v : v), 0);
    if (Math.abs(s) >= obs) hits++;
  }
  return hits / (1 << d.length);
}

const quantile = (s: number[], q: number) => {
  const i = q * (s.length - 1);
  const lo = Math.floor(i);
  const hi = Math.min(lo + 1, s.length - 1);
  return s[lo]! + (s[hi]! - s[lo]!) * (i - lo);
};

/** Percentile bootstrap CI of agg(treated) − agg(control), resampling whole tasks. */
export function bootCI(g: Groups, f: (xs: number[]) => number, n = N_BOOT, seed = BOOT_SEED): { lo: number; hi: number; draws: number[] } {
  const r = rng(seed);
  const draws: number[] = [];
  for (let i = 0; i < n; i++) {
    const xa: number[] = [];
    const xb: number[] = [];
    for (let k = 0; k < g.length; k++) {
      const [a, b] = g[Math.floor(r() * g.length)]!;
      xa.push(...a);
      xb.push(...b);
    }
    draws.push(f(xa) - f(xb));
  }
  const s = [...draws].sort((a, b) => a - b);
  return { lo: quantile(s, 0.025), hi: quantile(s, 0.975), draws };
}

/** Holm's step-down adjustment. */
export function holm(ps: number[]): number[] {
  const order = ps.map((p, i) => [p, i] as const).sort((a, b) => a[0] - b[0]);
  const out = new Array<number>(ps.length);
  let running = 0;
  order.forEach(([p, i], r) => {
    running = Math.max(running, Math.min(1, (ps.length - r) * p));
    out[i] = running;
  });
  return out;
}

// ------------------------------------------------------------------ the live corpus (published)

// evals/corpus/README.md: sessions (train + eval) per sandbox run, and whether a reflex was learned.
export const CORPUS: { name: string; sessions: number; learned: boolean; sonnet?: boolean }[] = [
  { name: "acme-billing", sessions: 12 + 24, learned: false },
  { name: "harbor-dispatch", sessions: 12 + 36, learned: true },
  { name: "py-billing-s11", sessions: 9 + 12, learned: false },
  { name: "py-clinic-s12", sessions: 9 + 18, learned: true },
  { name: "py-logistics-s13", sessions: 9 + 12, learned: false },
  { name: "node-logistics-s21", sessions: 9 + 18, learned: true },
  { name: "node-billing-s22", sessions: 9 + 12, learned: false },
  { name: "node-clinic-s23", sessions: 9 + 18, learned: true },
  { name: "py-billing-hash-s32", sessions: 9 + 12, learned: false },
  { name: "node-billing-uuid-s42", sessions: 9 + 12, learned: false },
  { name: "acme-billing", sessions: 6 + 36, learned: true, sonnet: true },
  { name: "harbor-dispatch", sessions: 6 + 36, learned: true, sonnet: true },
];
// Pooled Haiku 4.5 comparisons, turns (median), adage − control, with the task-bootstrap CI.
const HEADLINE = [
  { label: "4 learned · adage − control", diff: -15.5, lo: -18.5, hi: -9, holm: "0.001" },
  { label: "all 10 · adage − control", diff: -8, lo: -13, hi: -1.5, holm: "0.029" },
  { label: "all 10 · instincts − control", diff: -1, lo: -4, hi: 3, holm: "1.0" },
];
// L0 bench (docs/evals.md, docs/tracks/track-g-graph-engine.md on the rebrand branch).
const L0 = { miners: 0.6307, graphG3: 0.9759, graphG5: 0.6148 };

// ------------------------------------------------------------------ the scene

const TX = (turns: number) => -6.2 + (turns - 10) * 0.46;
const ROWZ = (t: number) => -5.6 + t * 1.6;
const ARM_DZ = 0.36;
const HX = (d: number) => d * 0.55;
const HIST_BIN = 0.5;
const HIST_DOTS = 200;

const CAM: Record<"rows" | "hist" | "holm" | "boot" | "grid" | "mine", CamSpec> = {
  rows: { target: [-0.2, 0, 0.6], yaw: 0, pitch: 1.2, dist: 20 },
  hist: { target: [-2.2, 2, 0], yaw: -0.12, pitch: 0.28, dist: 20 },
  holm: { target: [0, 2.6, 0], yaw: 0, pitch: 0.12, dist: 16 },
  boot: { target: [-3, 0.8, 0], yaw: 0, pitch: 0.3, dist: 16 },
  grid: { target: [0, 0, 0.4], yaw: 0, pitch: 1.3, dist: 21 },
  mine: { target: [-1.5, 0, 0.4], yaw: 0, pitch: 1.2, dist: 19 },
};

function build(stage: Stage, ctx: { graph: Graph | null; repo: boolean }): Built {
  const mine = ctx.repo && ctx.graph ? ctx.graph.routes.filter((r) => r.in_index).slice(0, 8) : [];
  const useRepo = mine.length > 0;

  // Every statistic the captions state, computed from the sample tasks.
  const tests = METRICS.map((m) => {
    const g = groupsOf(m.key);
    const f = agg(m.agg);
    const all = (arm: 0 | 1) => g.flatMap((x) => x[arm]);
    return { m, g, diff: f(all(0)) - f(all(1)), treated: f(all(0)), control: f(all(1)), perm: permTest(g), boot: bootCI(g, f), flip: signFlip(g) };
  });
  const adj = holm(tests.map((t) => t.perm.p));
  const turns = tests[0]!;
  const obs = turns.perm.obs;
  const nul = turns.perm.null;
  const minFlip = 2 / 2 ** TASKS.length;
  const f2 = (x: number) => x.toFixed(2);
  const fp = (p: number) => (p < 0.001 ? p.toExponential(1) : p < 0.01 ? p.toFixed(4) : p.toFixed(3));
  const sd = (x: number) => (x > 0 ? `+${x.toFixed(1)}` : x < 0 ? `−${(-x).toFixed(1)}` : "0.0");

  // ---- rows: each run a dot, its arm a sub-lane.
  const runs: { id: string; t: number; arm: "control" | "treated"; r: number; turns: number }[] = [];
  TASKS.forEach((task, t) =>
    (["control", "treated"] as const).forEach((arm) =>
      task[arm].forEach((run, r) => {
        const id = `r${t}.${arm[0]}${r}`;
        runs.push({ id, t, arm, r, turns: run.turns });
        stage.dot(id, 0.19);
      }),
    ),
  );
  // Where each run lands after the first permutation: the arm whose half it was shuffled into.
  const permArm = new Map<string, "control" | "treated">();
  TASKS.forEach((_, t) => {
    const [a] = turns.perm.first[t]!;
    const pool = [...runs.filter((x) => x.t === t)];
    const want = [...a];
    for (const x of pool) {
      const i = want.indexOf(x.turns);
      if (i >= 0) {
        permArm.set(x.id, "treated");
        want.splice(i, 1);
      } else permArm.set(x.id, "control");
    }
  });
  const permDiff = taskMeanDiff(turns.perm.first);
  const means = (armOf: (id: string) => "control" | "treated") =>
    TASKS.map((_, t) => {
      const m = (arm: string) => mean(runs.filter((x) => x.t === t && armOf(x.id) === arm).map((x) => x.turns));
      return { c: m("control"), a: m("treated") };
    });
  const meanObs = means((id) => runs.find((x) => x.id === id)!.arm);
  const meanPerm = means((id) => permArm.get(id)!);
  TASKS.forEach((_, t) => {
    stage.dot(`mc${t}`, 0.12);
    stage.dot(`ma${t}`, 0.12);
  });
  stage.link("diffs", TASKS.map((_, t) => [`mc${t}`, `ma${t}`] as [string, string]), "accent");
  stage.link(
    "rowlines",
    TASKS.flatMap((_, t) => [
      [
        [TX(10), 0, ROWZ(t) - ARM_DZ],
        [TX(36), 0, ROWZ(t) - ARM_DZ],
      ],
      [
        [TX(10), 0, ROWZ(t) + ARM_DZ],
        [TX(36), 0, ROWZ(t) + ARM_DZ],
      ],
    ]) as [V3, V3][],
    "ink3",
  );
  TASKS.forEach((_, t) => stage.label(`task${t}`, `task ${t + 1}`, [TX(10), 0, ROWZ(t)], { dx: -54, dy: -7 }));
  TASKS.forEach((_, t) =>
    stage.label(`d${t}`, sd(meanObs[t]!.a - meanObs[t]!.c), [TX(36), 0, ROWZ(t)], { dx: 8, dy: -7 }),
  );
  TASKS.forEach((_, t) =>
    stage.label(`dp${t}`, sd(meanPerm[t]!.a - meanPerm[t]!.c), [TX(36), 0, ROWZ(t)], { dx: 8, dy: -7 }),
  );
  stage.label("armC", "control", [TX(10), 0, ROWZ(0) - ARM_DZ], { cls: "note", dx: 4, dy: -22 });
  stage.label("armT", "treated", [TX(10), 0, ROWZ(0) + ARM_DZ], { cls: "note", dx: 4, dy: 6 });
  stage.label("axisT", "turns →", [TX(23), 0, ROWZ(TASKS.length - 1) + 0.8], { dy: 4, center: true });
  stage.label("ax10", "10", [TX(10), 0, ROWZ(TASKS.length - 1) + 0.8], { dy: 4, center: true });
  stage.label("ax36", "36", [TX(36), 0, ROWZ(TASKS.length - 1) + 0.8], { dy: 4, center: true });
  stage.label("stat", `<small>MEAN OVER TASKS</small>${sd(obs)} turns`, [TX(23), 0, ROWZ(TASKS.length - 1) + 1], { cls: "chip big", dy: 8, center: true });
  stage.label("statP", `<small>SHUFFLE 1 · MEAN OVER TASKS</small>${sd(permDiff)} turns`, [TX(23), 0, ROWZ(TASKS.length - 1) + 1], { cls: "chip big", dy: 8, center: true });

  // ---- the null histogram: the first permutations, one dot each, stacked in bins.
  const shown = nul.slice(0, HIST_DOTS);
  const bins = new Map<number, number>();
  const histPos: V3[] = shown.map((d) => {
    const b = Math.round(d / HIST_BIN);
    const h = bins.get(b) ?? 0;
    bins.set(b, h + 1);
    return [HX(b * HIST_BIN), 0.2 + h * 0.24, 0];
  });
  shown.forEach((_, i) => stage.dot(`h${i}`, 0.1));
  stage.dot("obs", 0.26);
  const extreme = nul.filter((d) => Math.abs(d) >= Math.abs(obs) - 1e-12).length;
  const nullSd = Math.sqrt(mean(nul.map((d) => d * d)) - mean(nul) ** 2);
  stage.link(
    "hAxis",
    [
      [
        [HX(-16), 0, 0],
        [HX(10), 0, 0],
      ],
      [
        [HX(0), -0.2, 0],
        [HX(0), 0.2, 0],
      ],
    ],
    "ink3",
  );
  stage.link(
    "obsLine",
    [
      [
        [HX(obs), 0, 0],
        [HX(obs), 3.4, 0],
      ],
    ],
    "accent",
  );
  stage.label("obs", `<small>OBSERVED</small>${sd(obs)} turns`, [HX(obs), 3.4, 0], { cls: "chip big", dy: -50, center: true });
  stage.label("nullL", `${HIST_DOTS} of ${N_PERM.toLocaleString("en")} shuffles`, [HX(0), 0, 0], { cls: "note", dy: 10, center: true });
  stage.label("zero", "0", [HX(0), 0, 0], { dy: 26, center: true });

  // ---- Holm: raw and adjusted p per metric, on a −log10 p scale.
  const LY = (p: number) => Math.min(4.2, -Math.log10(p)) * 1.25;
  const HM = (i: number) => -4.5 + i * 4.5;
  tests.forEach((_, i) => {
    stage.dot(`praw${i}`, 0.16);
    stage.dot(`padj${i}`, 0.24);
  });
  stage.link("holm", tests.map((_, i) => [`praw${i}`, `padj${i}`] as [string, string]), "ink3");
  stage.link(
    "alpha",
    [
      [
        [-6.4, LY(ALPHA), 0],
        [6.4, LY(ALPHA), 0],
      ],
    ],
    "accent",
  );
  stage.link(
    "pBase",
    [
      [
        [-6.4, 0, 0],
        [6.4, 0, 0],
      ],
    ],
    "ink3",
  );
  tests.forEach((t, i) =>
    stage.label(`hl${i}`, `<small>${t.m.label.toUpperCase()}</small>p ${fp(t.perm.p)} → Holm ${fp(adj[i]!)}`, [HM(i), 0, 0], { cls: "chip", dy: 12, center: true }),
  );
  stage.label("alphaL", `α ${ALPHA}`, [-6.4, LY(ALPHA), 0], { cls: "note", dx: -46, dy: -7 });
  stage.label("pAxis", "−log10 p ↑", [-6.4, LY(1e-4) + 0.6, 0], { dx: -28, dy: -7 });
  stage.link("pStems", tests.map((_, i) => [[HM(i), 0, 0], `padj${i}`] as [V3, string]), "ink3");

  // ---- bootstrap: resampled medians as a strip, the interval, and zero.
  const BX = (d: number) => d * 0.5;
  const BOOT_DOTS = 120;
  const bshown = turns.boot.draws.slice(0, BOOT_DOTS);
  const bcount = new Map<number, number>();
  const bootPos: V3[] = bshown.map((d) => {
    const k = Math.round(d * 2);
    const h = bcount.get(k) ?? 0;
    bcount.set(k, h + 1);
    return [BX(d), 0.25 + h * 0.2, 0];
  });
  bshown.forEach((_, i) => stage.dot(`b${i}`, 0.09));
  stage.link(
    "ci",
    [
      [
        [BX(turns.boot.lo), -0.3, 0],
        [BX(turns.boot.hi), -0.3, 0],
      ],
      [
        [BX(turns.boot.lo), -0.55, 0],
        [BX(turns.boot.lo), -0.05, 0],
      ],
      [
        [BX(turns.boot.hi), -0.55, 0],
        [BX(turns.boot.hi), -0.05, 0],
      ],
    ],
    "accent",
  );
  stage.link(
    "zeroB",
    [
      [
        [0, -0.8, 0],
        [0, 3, 0],
      ],
    ],
    "ink3",
  );
  stage.label("ciL", `95% CI [${sd(turns.boot.lo)}, ${sd(turns.boot.hi)}]`, [BX((turns.boot.lo + turns.boot.hi) / 2), -0.3, 0], { cls: "chip", dy: 10, center: true });
  stage.label("zeroBL", "no effect", [0, 3, 0], { cls: "note", dy: -18, center: true });

  // ---- the corpus: 354 sessions, clustered by sandbox run.
  const total = CORPUS.reduce((a, c) => a + c.sessions, 0);
  const cells: { id: string; c: number; pos: V3 }[] = [];
  CORPUS.forEach((c, k) => {
    const cx = -5.7 + (k % 4) * 3.8;
    const cz = -3.9 + Math.floor(k / 4) * 3.9;
    const cols = Math.ceil(Math.sqrt(c.sessions));
    for (let i = 0; i < c.sessions; i++) {
      const id = `s${k}.${i}`;
      cells.push({ id, c: k, pos: [cx + ((i % cols) - (cols - 1) / 2) * 0.34, 0, cz + (Math.floor(i / cols) - (cols - 1) / 2) * 0.34] });
      stage.dot(id, 0.09);
    }
    stage.label(`sb${k}`, `${c.name}<br><span class="how-dim">${c.sonnet ? "Sonnet 5" : "Haiku 4.5"}<br>${c.sessions} traces</span>`, [cx, 0, cz + 1.3], { dy: 4, center: true });
  });
  const learnedHaiku = CORPUS.filter((c) => c.learned && !c.sonnet).length;
  const haiku = CORPUS.filter((c) => !c.sonnet).length;

  // ---- this repository: each reflex's two arms, one dot per trace where it applied.
  const rows = mine.map((r) => repoRow(r));
  const MAXD = 24;
  rows.forEach((row, i) => {
    (["t", "c"] as const).forEach((a) => {
      const n = Math.min(MAXD, a === "t" ? row.t.n : row.c.n);
      for (let j = 0; j < n; j++) stage.dot(`y${i}${a}${j}`, 0.12);
    });
    const verdict = row.enough ? `${pct(row.t)} vs ${pct(row.c)}` : "too few to conclude";
    stage.label(
      `y${i}`,
      `${row.r.id}<br><span class="how-dim">${row.word} · treated ${row.t.k}/${row.t.n} · control ${row.c.k}/${row.c.n} · ${verdict}</span>`,
      [-7.4, 0, YZ(i, rows.length)],
      { dy: -30 },
    );
  });
  const k0 = kpis(mine)[0];

  // ---------------------------------------------------------------- states

  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    const labels: string[] = [];
    const links: Record<string, number> = {};
    if (s <= 2) {
      for (const x of runs) {
        const arm = s === 2 ? permArm.get(x.id)! : x.arm;
        dots[x.id] = { pos: [TX(x.turns), 0, ROWZ(x.t) + (arm === "control" ? -ARM_DZ : ARM_DZ)], color: arm === "control" ? "ink2" : "accent", glow: arm === "control" ? 0.12 : 0.4 };
      }
      if (s >= 1) {
        const ms = s === 2 ? meanPerm : meanObs;
        ms.forEach((m, t) => {
          dots[`mc${t}`] = { pos: [TX(m.c), 0.02, ROWZ(t)], color: "ink2", scale: 1.1, glow: 0 };
          dots[`ma${t}`] = { pos: [TX(m.a), 0.02, ROWZ(t)], color: "accent", scale: 1.1, glow: 0 };
        });
        links.diffs = 0.9;
        labels.push(...TASKS.map((_, t) => (s === 2 ? `dp${t}` : `d${t}`)), s === 2 ? "statP" : "stat");
      }
      links.rowlines = 0.18;
      labels.push(...TASKS.map((_, t) => `task${t}`), "armC", "armT");
      if (s === 0) labels.push("axisT", "ax10", "ax36");
      return { dots, labels, links, cam: CAM.rows };
    }
    if (s === 3) {
      shown.forEach((_, i) => (dots[`h${i}`] = { pos: histPos[i]!, color: "ink2", alpha: 0.85, glow: 0.05 }));
      dots.obs = { pos: [HX(obs), 0.2, 0], color: "accent", scale: 1.2, glow: 0.9 };
      return { dots, labels: ["obs", "nullL", "zero"], links: { hAxis: 0.6, obsLine: 0.9 }, cam: CAM.hist };
    }
    if (s === 4) {
      tests.forEach((t, i) => {
        const sig = adj[i]! < ALPHA;
        dots[`praw${i}`] = { pos: [HM(i), LY(t.perm.p), 0], color: "ink3", alpha: 0.6, glow: 0 };
        dots[`padj${i}`] = { pos: [HM(i), LY(adj[i]!), 0], color: sig ? "accent" : "ink2", glow: sig ? 0.8 : 0.1 };
      });
      labels.push(...tests.map((_, i) => `hl${i}`), "alphaL", "pAxis");
      return { dots, labels, links: { holm: 0.7, alpha: 0.8, pBase: 0.5, pStems: 0.35 }, cam: CAM.holm };
    }
    if (s === 5) {
      bshown.forEach((_, i) => (dots[`b${i}`] = { pos: bootPos[i]!, color: "soft", alpha: 0.85, glow: 0.1 }));
      return { dots, labels: ["ciL", "zeroBL"], links: { ci: 0.95, zeroB: 0.6 }, cam: CAM.boot };
    }
    if (s === 6 || (s === 7 && !useRepo)) {
      for (const cell of cells) {
        const c = CORPUS[cell.c]!;
        const lit = s === 6 ? c.learned : !c.learned;
        dots[cell.id] = { pos: cell.pos, color: lit ? (s === 6 ? (c.sonnet ? "soft" : "accent") : "ink1") : "ink3", alpha: lit ? 1 : 0.28, glow: lit ? 0.3 : 0 };
      }
      labels.push(...CORPUS.map((_, k) => `sb${k}`));
      return { dots, labels, links: {}, cam: CAM.grid };
    }
    // This repository.
    rows.forEach((row, i) => {
      const z = YZ(i, rows.length);
      (["t", "c"] as const).forEach((a) => {
        const ratio = a === "t" ? row.t : row.c;
        const n = Math.min(MAXD, ratio.n);
        const k = ratio.n > MAXD ? Math.round((ratio.k / ratio.n) * MAXD) : ratio.k;
        for (let j = 0; j < n; j++) {
          const on = j < k;
          dots[`y${i}${a}${j}`] = {
            pos: [-7.2 + j * 0.42, 0, z + (a === "t" ? -0.3 : 0.3) + 0.5],
            color: on ? (a === "t" ? "accent" : "ink1") : "ink3",
            alpha: on ? 1 : 0.35,
            glow: on && a === "t" ? 0.4 : 0,
          };
        }
      });
      labels.push(`y${i}`);
    });
    return { dots, labels, links: {}, cam: CAM.mine };
  }

  // ---------------------------------------------------------------- words

  const text: Omit<Step, "state">[] = [
    {
      title: "Two arms, task by task",
      body: `Each row is one of ${TASKS.length} sample tasks; each dot is a run, placed by its turns. Every task runs in both arms, treated and control, twice each. Runs of one task are not independent, so the task, not the run, is the unit.`,
      chips: [
        { text: `${TASKS.length} tasks × 2 arms × 2 runs` },
        { text: `median turns ${turns.treated} vs ${turns.control}` },
        { text: "arm order shuffled per (task, repeat)" },
      ],
    },
    {
      title: "Measure the difference within each task",
      body: `Within each task, the treated runs' mean is compared with the control runs' mean. The test statistic is the mean of those differences over the ${TASKS.length} tasks: ${sd(obs)} turns.`,
      chips: [{ text: `task mean diff ${sd(obs)}` }, { text: `${turns.g.filter(([a, b]) => mean(a) < mean(b)).length} of ${TASKS.length} tasks fewer turns` }],
    },
    {
      title: "Shuffle the arms within each task",
      body: `If the arm made no difference, the labels could be swapped within a task and nothing would change. One shuffle, seeded: runs trade arms inside their own row, never across rows, and the statistic becomes ${sd(permDiff)}.`,
      chips: [{ text: "labels shuffled within tasks" }, { text: `seed ${PERM_SEED}` }, { text: `shuffle 1: ${sd(permDiff)}` }],
    },
    {
      title: "Chance, ten thousand times",
      body: `Repeat the shuffle ${N_PERM.toLocaleString("en")} times and the statistics pile up around zero, with a spread of ±${f2(nullSd)}. ${extreme ? `Only ${extreme}` : "None"} land as far out as the observed ${sd(obs)}, so p = (${extreme} + 1) / (${N_PERM.toLocaleString("en")} + 1) = ${fp(turns.perm.p)}.`,
      chips: [{ text: `${N_PERM.toLocaleString("en")} permutations` }, { text: "two-sided" }, { text: `p ${fp(turns.perm.p)}`, ok: turns.perm.p < ALPHA }],
      figure: histFigure(nul, obs),
    },
    {
      title: "Correct for looking at several metrics",
      body: `Turns, cost and pass rate are tested together, so a chance hit somewhere is likelier. Holm's step-down multiplies the smallest p by ${tests.length}, the next by ${tests.length - 1}, and so on, never letting a later p fall below an earlier one. ${tests
        .map((t, i) => `${t.m.label} ${fp(adj[i]!)}`)
        .join(", ")}.`,
      chips: tests.map((t, i) => ({ text: `${t.m.label}: Holm ${fp(adj[i]!)}`, ok: adj[i]! < ALPHA })),
    },
    {
      title: "Ask how far it generalises",
      body: `Resampling whole tasks ${N_BOOT.toLocaleString("en")} times gives a 95% interval for the median difference: [${sd(turns.boot.lo)}, ${sd(turns.boot.hi)}] turns${turns.boot.hi < 0 ? ", clear of zero" : ""}. A sign-flip test over the task differences asks whether it would hold for new tasks; it is exact, and with ${TASKS.length} tasks its smallest possible p is 2/2^${TASKS.length} = ${fp(minFlip)}.`,
      chips: [{ text: `bootstrap ${N_BOOT.toLocaleString("en")} · seed ${BOOT_SEED}` }, { text: `sign-flip p ${fp(turns.flip)}`, ok: turns.flip < ALPHA }, { text: `${TASKS.length} tasks` }],
      figure: ciFigure([{ label: "sample · turns", diff: turns.diff, lo: turns.boot.lo, hi: turns.boot.hi }], -16, 6),
    },
    {
      title: "What 354 real traces showed",
      body: `The live corpus: ${total} headless Claude Code sessions over ${CORPUS.length} runs of ${haiku} sandboxes, confined with bubblewrap, arms control, adage and instincts. With Haiku 4.5, in the ${learnedHaiku} sandboxes where a reflex was learned: median turns 12.5 vs 28 (−15.5, CI [−18.5, −9], Holm p 0.001, sign-flip p 0.0002), cost $0.063 vs $0.120 (−48%). Across all ${haiku}: 18 vs 26 turns (Holm p 0.029); pass rate 0.62 vs 0.51, Holm p 0.76, not significant.${useRepo ? ` In ${haiku - learnedHaiku} of ${haiku} sandboxes nothing was learned, and instincts alone showed no measurable effect.` : ""}`,
      chips: [
        { text: "turns −15.5 · Holm 0.001", ok: true },
        { text: "cost −48%", ok: true },
        { text: "pass 0.62 vs 0.51 · not significant" },
        ...(useRepo ? [{ text: "0 false learned reflexes" }] : []),
      ],
      figure: ciFigure(HEADLINE, -20, 5),
    },
    useRepo
      ? {
          title: "Your reflexes against control",
          body: `Each row is one of your reflexes: a dot per trace where it applied, lit where the agent took its path (or, for an inhibit, hit the dead end). Treated above, control below. A share is stated only with at least ${MIN_TREATED} treated and ${MIN_CONTROL} control traces; ${
            rows.filter((r) => r.enough).length ? `${rows.filter((r) => r.enough).length} of ${rows.length} have that many.` : `none of your ${rows.length} has that many yet, so these numbers are too small to conclude anything.`
          }${k0 ? ` In all: took the known path ${k0.treated.k}/${k0.treated.n} treated, ${k0.control.k}/${k0.control.n} control.` : ""}`,
          chips: [{ text: `≥ ${MIN_TREATED} treated · ≥ ${MIN_CONTROL} control` }, { text: "counts are distinct traces" }],
        }
      : {
          title: "Where nothing was learned, nothing changed",
          body: `In ${haiku - learnedHaiku} of ${haiku} Haiku sandboxes the agents found the lookup in too few training traces, so no reflex was learned; there, instincts alone showed no measurable effect (turns −1, CI [−4, 3]). No learned reflex was false. Offline, before any agent runs, the L0 bench scores a learner by J = turns saved − 2.0 × false directives + 1.0 × route F1, per session.`,
          chips: [
            { text: `nothing learned: ${haiku - learnedHaiku}/${haiku}` },
            { text: "0 false learned reflexes" },
            { text: `L0 J: miners ${L0.miners} · graph ${L0.graphG3} (G3), ${L0.graphG5} (G5)` },
          ],
        },
  ];

  const legend = [
    { label: "control", role: "ink2" as const },
    { label: "treated", role: "accent" as const },
  ];
  return {
    source: useRepo ? `your ${rows.length} reflexes · ${TASKS.length} sample tasks` : `${TASKS.length} sample tasks · the live corpus`,
    legend,
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

const YZ = (i: number, n: number) => (i - (n - 1) / 2) * Math.min(2.6, 16 / Math.max(1, n));

interface RepoRow {
  r: Route;
  word: string;
  t: { k: number; n: number };
  c: { k: number; n: number };
  enough: boolean;
}

/** One reflex's treated and control counts, as the Evidence view reads them. */
function repoRow(r: Route): RepoRow {
  const avoid = r.step.kind === "avoid";
  const ratio = (a: Route["arms"]["treated"]) => ({ k: a ? (avoid ? a.hit_avoided : a.followed) : 0, n: a?.applied ?? 0 });
  const t = ratio(r.arms.treated);
  const c = ratio(r.arms.holdout);
  return { r, word: avoid ? "hit the dead end" : "took the path", t, c, enough: t.n >= MIN_TREATED && c.n >= MIN_CONTROL && share(t) !== null };
}

// ------------------------------------------------------------------ figures

const MONO = "10px 'Red Hat Mono', ui-monospace, monospace";

function histFigure(nul: number[], obs: number) {
  return (g: CanvasRenderingContext2D, w: number, h: number, pal: Palette) => {
    const lo = Math.min(obs, ...nul) - 1;
    const hi = Math.max(-obs, ...nul) + 1;
    const nb = 48;
    const counts = new Array<number>(nb).fill(0);
    for (const d of nul) counts[Math.min(nb - 1, Math.floor(((d - lo) / (hi - lo)) * nb))]!++;
    const padB = 14;
    const X = (v: number) => 4 + ((v - lo) / (hi - lo)) * (w - 8);
    const top = Math.max(...counts);
    const bw = (w - 8) / nb;
    g.fillStyle = pal.css("ink3");
    counts.forEach((c, i) => {
      const bh = (c / top) * (h - padB - 16);
      g.fillRect(4 + i * bw + 0.5, h - padB - bh, Math.max(1, bw - 1), bh);
    });
    g.font = MONO;
    g.textBaseline = "top";
    g.textAlign = "center";
    g.fillText("0", X(0), h - padB + 2);
    g.strokeStyle = pal.css("accent");
    g.fillStyle = pal.css("accent");
    g.lineWidth = 1.5;
    g.beginPath();
    g.moveTo(X(obs), 12);
    g.lineTo(X(obs), h - padB);
    g.stroke();
    g.textAlign = "left";
    g.fillText(`observed ${obs < 0 ? "−" : ""}${Math.abs(obs).toFixed(1)}`, X(obs) + 4, 2);
    g.textAlign = "right";
    g.fillStyle = pal.css("ink3");
    g.fillText(`${nul.length.toLocaleString("en")} shuffles`, w - 4, 2);
  };
}

function ciFigure(rows: { label: string; diff: number; lo: number; hi: number; holm?: string }[], lo: number, hi: number) {
  return (g: CanvasRenderingContext2D, w: number, h: number, pal: Palette) => {
    const left = rows.length > 1 ? 176 : 96;
    const X = (v: number) => left + ((v - lo) / (hi - lo)) * (w - left - 8);
    const padB = 14;
    const rowH = (h - padB - 4) / rows.length;
    g.font = MONO;
    g.strokeStyle = pal.css("ink3");
    g.globalAlpha = 0.6;
    g.setLineDash([3, 3]);
    g.beginPath();
    g.moveTo(X(0) + 0.5, 2);
    g.lineTo(X(0) + 0.5, h - padB);
    g.stroke();
    g.setLineDash([]);
    g.globalAlpha = 1;
    g.fillStyle = pal.css("ink3");
    g.textBaseline = "top";
    g.textAlign = "center";
    for (let v = Math.ceil(lo / 5) * 5; v <= hi; v += 5) g.fillText(v < 0 ? `−${-v}` : String(v), X(v), h - padB + 2);
    rows.forEach((r, i) => {
      const y = 4 + i * rowH + rowH / 2;
      const clear = r.hi < 0 || r.lo > 0;
      const color = pal.css(clear ? "accent" : "ink2");
      g.textBaseline = "middle";
      g.textAlign = "left";
      g.fillStyle = pal.css("ink2");
      g.fillText(r.label, 0, y);
      g.strokeStyle = color;
      g.lineWidth = 2;
      g.beginPath();
      g.moveTo(X(r.lo), y);
      g.lineTo(X(r.hi), y);
      g.moveTo(X(r.lo), y - 4);
      g.lineTo(X(r.lo), y + 4);
      g.moveTo(X(r.hi), y - 4);
      g.lineTo(X(r.hi), y + 4);
      g.stroke();
      g.fillStyle = color;
      g.beginPath();
      g.arc(X(r.diff), y, 3.5, 0, Math.PI * 2);
      g.fill();
      if (r.holm) {
        g.fillStyle = pal.css("ink3");
        const t = `Holm ${r.holm}`;
        const right = X(r.hi) + 8 + g.measureText(t).width <= w;
        g.textAlign = right ? "left" : "right";
        g.fillText(t, right ? X(r.hi) + 6 : X(r.lo) - 6, y);
      }
    });
    g.lineWidth = 1;
  };
}

export const measure: Chapter = {
  id: "measure",
  title: "Measure against control",
  status: "Built",
  kind: "built",
  summary:
    "Effects are measured treated against control on the same tasks. The task is the unit, and the tests shuffle arms rather than assume a distribution.",
  source: "crates/adage-eval · evals/live · evals/corpus",
  usesRepo: true,
  build,
};


