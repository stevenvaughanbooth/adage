// Chapter: choose and compile (Track G, crates/adage-graph: score.rs, select.rs, compile/).
// Candidate patterns are scored as predictions, calibrated, selected by compression with one code
// table per delivery shape, and compiled to reflexes whose strength is decided at compile time; an
// avoid interlocks the runs it contradicts, and the survivors are written to one track,
// `<repo>/learned`, with a `.why.json` beside it.
//
// The traces are sample data in the registry-driven service of "Find repeated subgraphs"; the
// candidates are the patterns mining would hand the scorer. Everything else is the real arithmetic, ported: the
// discounted Beta posterior with its backoff prior, the anchor-level contrast, p_soon and Δturns,
// the value, the gates, the isotonic calibration over 200 bins on 3 task-grouped folds, and the
// SQS-style MDL gain of select.rs with its lazy greedy order. Every number a caption states is
// computed here or is a constant cited from the code.

import type { CamSpec, DotLook, SceneState, Stage, V3 } from "../engine";
import { PROTOS, betaGrid, f2, type Code } from "../sample";
import type { Built, Chapter, Step } from "../types";

// ------------------------------------------------------------------ constants (cited)

// score.rs ScoreParams::default
const MIN_SUPPORT = 5;
const MIN_DAYS = 2;
const KAPPA = 2;
const LAMBDA_FP = 2;
const HALF_LIFE = 60;
const CONTRAST = 0.3;
const CONSISTENCY = 0.9;
const DOMINANT = 0.8;
const GUARD_LB = 0.5;
// mine: the gap between items (alphabet events), the context trigger's `within`
const GAP = 8;
// select.rs
const ITEM_BITS = 4;
const MAX_REFLEXES = 256;
const MAX_CLOSED = 20_000;
const MAX_UNSELECTED = 2_000;
const CAL_FOLDS = 3;
const CAL_BINS = 200;
const CAL_REMINE_MAX = 400;
// compile/mod.rs CompileParams::default
const GREEN_LB = 0.8;
const YELLOW_RATE = 0.5;
const ANNOUNCE_AT_START = 0.5;
const PRIOR_KAPPA = 3;
const PRIOR_MEAN = 0.7;

// ------------------------------------------------------------------ sample traces

type Ty = "prompt" | "read" | "edit" | "act";
const TYPE: Record<Code, Ty> = { P: "prompt", D: "prompt", R: "read", E: "edit", S: "edit", G: "act", C: "act", F: "act", W: "act", h: "act" };

interface Ev {
  code: Code;
  exact: string;
  fail: boolean;
}

interface Trace {
  id: string;
  task: string;
  day: number;
  ev: Ev[];
}

const EXACT: Record<Code, string[]> = {
  P: ["add carrier HX-7", "fix the Oslo port code", "lane rate wrong NYC→BOS", "rename carrier Northwind", "remove port Tacoma", "update lane LA→SF", "add port Gdańsk"],
  D: ["dispatch picks the slow lane", "retry dispatch on 503", "router ignores holidays", "log dispatch latency", "cap dispatch retries"],
  R: ["read src/registry/load.ts", "read src/registry/ports.ts", "read src/dispatch.ts", "read src/router.ts"],
  E: ["edit registry/carriers.csv", "edit registry/ports.csv", "edit registry/lanes.csv"],
  S: ["edit src/dispatch.ts", "edit src/router.ts"],
  G: ["make gen", "make gen -B"],
  C: ["make check", "make check -j4"],
  F: ["npm test"],
  W: ["git status"],
  h: ["curl localhost:8080/health"],
};

/** Trace shapes and how many traces follow each (codes; F is a failing npm test). */
const SHAPES: [string, number][] = [
  ["P R E E W G W C W", 4],
  ["P R E W G R C W", 4],
  ["P E R G W C", 2],
  ["P R E G W C W", 2],
  ["P R E G F W C", 3],
  ["P R E W G W", 1],
  ["P R E W C W", 1],
  ["D R S W F C W", 7],
  ["D R S W F W C", 2],
  ["D S F C W", 2],
  ["D R S W C", 1],
  ["D R S h C", 1],
  ["D W S F C", 2],
];

export const TRACES_05: Trace[] = (() => {
  const out: Trace[] = [];
  let n = 0;
  SHAPES.forEach(([shape, count], si) => {
    for (let c = 0; c < count; c++, n++) {
      const codes = shape.split(" ") as Code[];
      const ev = codes.map((code, j) => {
        const opts = EXACT[code];
        // Options vary in one trace in nine, so each command keeps a dominant spelling.
        const k = code === "G" || code === "C" ? (n % 9 === (code === "G" ? 4 : 7) ? 1 : 0) : n * 3 + j * 5 + si;
        return { code, exact: opts[k % opts.length]!, fail: code === "F" };
      });
      out.push({ id: `t${n + 1}`, task: `task-${String(si).padStart(2, "0")}-${Math.floor(c / 2)}`, day: 1 + (n % 6), ev });
    }
  });
  return out;
})();

// ------------------------------------------------------------------ patterns

interface Item {
  code: Code;
  /** A failed act (polarity fail). */
  fail?: boolean;
  /** The last edit of its kind in the trace. */
  last?: boolean;
}

export interface Cand {
  key: string;
  /** How the card reads. */
  text: string;
  items: Item[];
  cut: number;
}

const it = (code: Code, o: Partial<Item> = {}): Item => ({ code, ...o });

/** The candidates the chapter follows, as mining hands them to the scorer. */
export const CANDS: Cand[] = [
  { key: "seq", text: "edit registry/*.csv → make gen → make check", items: [it("E"), it("G"), it("C")], cut: 1 },
  { key: "obl", text: "last edit of registry/*.csv → make gen", items: [it("E", { last: true }), it("G")], cut: 1 },
  { key: "ctx", text: "[edit registry/*.csv, make gen] → make check", items: [it("E"), it("G"), it("C")], cut: 2 },
  { key: "seq2", text: "[registry prompt, edit registry/*.csv] → make gen → make check", items: [it("P"), it("E"), it("G"), it("C")], cut: 2 },
  { key: "avoid", text: "npm test ✗ → make check", items: [it("F", { fail: true }), it("C")], cut: 1 },
  { key: "npm", text: "[read src/*.ts, edit src/*.ts] → npm test", items: [it("R"), it("S"), it("F")], cut: 2 },
  { key: "status", text: "git status → make check", items: [it("W"), it("C")], cut: 1 },
  { key: "curl", text: "curl localhost:8080/health → make check", items: [it("h"), it("C")], cut: 1 },
];

function nodeOk(t: Trace, q: number, item: Item): boolean {
  const e = t.ev[q]!;
  if (e.code !== item.code || (item.fail && !e.fail)) return false;
  return !item.last || !t.ev.some((x, j) => j > q && x.code === item.code);
}

/** End positions of `items` in `t`: each item within GAP events after the one before it. */
function ends(t: Trace, items: Item[], avail?: boolean[]): number[] {
  let cur = t.ev.map((_, q) => q).filter((q) => nodeOk(t, q, items[0]!) && (!avail || avail[q]));
  for (const item of items.slice(1)) {
    cur = t.ev.map((_, q) => q).filter((q) => nodeOk(t, q, item) && (!avail || avail[q]) && cur.some((p) => p < q && q <= p + GAP));
  }
  return cur;
}

const occurs = (t: Trace, items: Item[]) => ends(t, items).length > 0;

/** The earliest continuation chain after anchor `p` over available events. */
function chainFrom(t: Trace, p: number, ys: Item[], avail?: boolean[]): number[] | null {
  if (!ys.length) return [];
  for (let q = p + 1; q <= Math.min(p + GAP, t.ev.length - 1); q++) {
    if ((avail && !avail[q]) || !nodeOk(t, q, ys[0]!)) continue;
    const rest = chainFrom(t, q, ys.slice(1), avail);
    if (rest) return [q, ...rest];
  }
  return null;
}

// ------------------------------------------------------------------ scoring (score.rs)

export interface Scored {
  cand: Cand;
  support: number;
  opp: number;
  days: number;
  prior: number;
  alpha: number;
  beta: number;
  conf: number;
  confLb: number;
  confCal: number;
  confLbCal: number;
  contrast: number;
  lift: number;
  consistency: number | null;
  pSoon: number;
  dturns: number;
  value: number;
  variant: string | null;
  variantShare: number;
  guard: boolean;
  gate: string;
  passes: boolean;
  oppList: number[];
}

const median = (v: number[]) => (v.length ? v.slice().sort((a, b) => a - b)[Math.floor((v.length - 1) / 2)]! : 0);

function posterior(s: number, n: number, m: number, kappa: number): [number, number] {
  const mm = Math.min(1 - 1e-6, Math.max(1e-6, m));
  return [s + kappa * mm, Math.max(0, n - s) + kappa * (1 - mm)];
}

function score(traces: Trace[], cand: Cand): Scored {
  const { items, cut } = cand;
  const xs = items.slice(0, cut);
  const ys = items.slice(cut);
  const maxday = Math.max(...traces.map((t) => t.day));
  const w = traces.map((t) => Math.pow(0.5, (maxday - t.day) / HALF_LIFE));
  const total = w.reduce((a, b) => a + b, 0);
  const occW = (its: Item[]) => traces.reduce((s, t, i) => s + (occurs(t, its) ? w[i]! : 0), 0);
  // The base rate of Y, then the backoff chain over context suffixes, shortest first.
  let prior = total > 0 ? occW(ys) / total : 0;
  let last = [0, 0];
  for (let k = 0; k < cut; k++) {
    const xk = xs.slice(cut - 1 - k);
    const n = occW(xk);
    const s = occW([...xk, ...ys]);
    last = [n, s];
    if (k + 1 < cut) prior = (s + KAPPA * prior) / (n + KAPPA);
  }
  const [n, s] = last as [number, number];
  const [alpha, beta] = posterior(s, n, prior, KAPPA);
  const conf = alpha / (alpha + beta);
  const confLb = betaGrid(alpha, beta).q05;

  // Anchor-level statistics.
  const xLast = xs[cut - 1]!;
  let xEnds = 0;
  let xFollow = 0;
  let soon = 0;
  const gaps: number[] = [];
  const variants = new Map<string, number>();
  const oppList: number[] = [];
  traces.forEach((t, i) => {
    const e = ends(t, xs);
    if (e.length) oppList.push(i);
    for (const q of e) {
      xEnds++;
      const ch = chainFrom(t, q, ys);
      if (!ch) continue;
      xFollow++;
      gaps.push(ch[0]! - q);
      if (ch[0]! - q <= 1) soon++;
      variants.set(t.ev[ch[0]!]!.exact, (variants.get(t.ev[ch[0]!]!.exact) ?? 0) + 1);
    }
  });
  // Background: events of the type X ends with (every event, when X ends at all of them).
  let bgSame = 0;
  let sameN = 0;
  let bgAny = 0;
  let positions = 0;
  for (const t of traces) {
    t.ev.forEach((e, q) => {
      positions++;
      const f = chainFrom(t, q, ys) ? 1 : 0;
      bgAny += f;
      if (TYPE[e.code] === TYPE[xLast.code]) {
        sameN++;
        bgSame += f;
      }
    });
  }
  const confA = xEnds ? xFollow / xEnds : 0;
  const [bg, bgN] = sameN > xEnds ? [bgSame, sameN] : [bgAny, positions];
  const bgRate = bgN ? bg / bgN : 0;
  const others = bgN - xEnds;
  const bgOther = others > 0 ? Math.max(0, bg - xFollow) / others : 0;
  const lift = bgRate > 0 ? confA / bgRate : 0;
  const contrast = confA - bgOther;
  const hasFail = !!xLast.fail;
  const pSoon = xEnds && !hasFail ? soon / xEnds : 0;
  const dturns = median(gaps);
  const failItems = items.filter((x) => x.fail);
  const consistency = failItems.length
    ? Math.min(
        ...failItems.map((x) => {
          const runs = traces.flatMap((t) => t.ev.filter((e) => e.code === x.code));
          return runs.length ? runs.filter((e) => e.fail).length / runs.length : 0;
        }),
      )
    : null;
  const value = conf * (1 - pSoon) * dturns - LAMBDA_FP * (1 - conf);
  const tot = [...variants.values()].reduce((a, b) => a + b, 0);
  const top = [...variants.entries()].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))[0];
  const variantShare = top && tot ? top[1] / tot : 0;
  const variant = top && variantShare >= DOMINANT ? top[0] : null;

  const full = traces.filter((t) => occurs(t, items));
  const support = full.length;
  const days = new Set(full.map((t) => t.day)).size;
  const minDays = Math.min(MIN_DAYS, Math.max(1, new Set(traces.map((t) => t.day)).size));
  let gate =
    support < MIN_SUPPORT
      ? `support ${support} < ${MIN_SUPPORT}`
      : days < minDays
        ? `days ${days} < ${minDays}`
        : consistency !== null
          ? consistency < CONSISTENCY
            ? `consistency ${f2(consistency)} < ${CONSISTENCY}`
            : ""
          : contrast < CONTRAST
            ? `contrast ${f2(contrast)} < ${f2(CONTRAST)}`
            : "";
  const guard = hasFail || (!!xLast.last && TYPE[xLast.code] === "edit");
  if (!gate && !guard && value <= 0) gate = `value ${f2(value)} ≤ 0`;
  else if (!gate && guard && confLb < GUARD_LB) gate = `guard lower bound ${f2(confLb)} < ${f2(GUARD_LB)}`;
  return {
    cand,
    support,
    opp: oppList.length,
    days,
    prior,
    alpha,
    beta,
    conf,
    confLb,
    confCal: conf,
    confLbCal: confLb,
    contrast,
    lift,
    consistency,
    pSoon,
    dturns,
    value,
    variant,
    variantShare,
    guard,
    gate,
    passes: !gate,
    oppList,
  };
}

// ------------------------------------------------------------------ calibration (score.rs Calibration)

export interface Calibration {
  /** (mean predicted, mean observed) per isotonic block. */
  points: [number, number][];
  /** The raw bins, (predicted sum, outcome sum, count). */
  bins: [number, number, number][];
  pairs: number;
}

function fromBins(bins: [number, number, number][]): Calibration {
  const blocks: [number, number, number][] = [];
  for (const b of bins.filter((x) => x[2] > 0)) {
    blocks.push([...b]);
    while (blocks.length >= 2) {
      const x = blocks[blocks.length - 2]!;
      const y = blocks[blocks.length - 1]!;
      if (x[1] / x[2] <= y[1] / y[2]) break;
      blocks.pop();
      blocks[blocks.length - 1] = [x[0] + y[0], x[1] + y[1], x[2] + y[2]];
    }
  }
  return { points: blocks.map((b) => [b[0] / b[2], b[1] / b[2]]), bins, pairs: bins.reduce((s, b) => s + b[2], 0) };
}

export function apply(cal: Calibration, x: number): number {
  const p = cal.points;
  if (!p.length) return x;
  if (p.length === 1 || x <= p[0]![0]) return p[0]![1];
  for (let i = 1; i < p.length; i++) {
    const [a, b] = [p[i - 1]!, p[i]!];
    if (x <= b[0]) {
      const t = b[0] > a[0] ? (x - a[0]) / (b[0] - a[0]) : 1;
      return a[1] + t * (b[1] - a[1]);
    }
  }
  return p[p.length - 1]![1];
}

/** Task-grouped inner folds: a task's rank among the sorted task names, mod k. */
function innerFolds(traces: Trace[], k: number): number[] {
  const tasks = [...new Set(traces.map((t) => t.task))].sort();
  return traces.map((t) => tasks.indexOf(t.task) % k);
}

/** Every one- and two-item context with a one-item continuation that the traces show at least
 * `min` times: the pool of calibration pairs (the real fit uses every scored candidate). */
function pool(traces: Trace[], min: number): Cand[] {
  const codes = Object.keys(TYPE) as Code[];
  const ctxs: Item[][] = [];
  for (const a of codes) {
    ctxs.push([it(a, { fail: a === "F" })]);
    for (const b of codes) ctxs.push([it(a), it(b, { fail: b === "F" })]);
  }
  const out: Cand[] = [];
  for (const x of ctxs)
    for (const y of codes.filter((c) => TYPE[c] === "act" || TYPE[c] === "read")) {
      const items = [...x, it(y)];
      if (traces.filter((t) => occurs(t, items)).length >= min) out.push({ key: "", text: "", items, cut: x.length });
    }
  return out;
}

function calibrate(traces: Trace[]): Calibration {
  const fold = innerFolds(traces, CAL_FOLDS);
  const bins: [number, number, number][] = Array.from({ length: CAL_BINS }, () => [0, 0, 0]);
  for (let f = 0; f < CAL_FOLDS; f++) {
    const train = traces.filter((_, i) => fold[i] !== f);
    const test = traces.filter((_, i) => fold[i] === f);
    if (!train.length || !test.length) continue;
    for (const c of pool(train, 3)) {
      const sc = score(train, c);
      const xs = c.items.slice(0, c.cut);
      for (const t of test) {
        if (!occurs(t, xs)) continue;
        const i = Math.min(CAL_BINS - 1, Math.floor(sc.conf * CAL_BINS));
        bins[i]![0] += sc.conf;
        bins[i]![1] += occurs(t, c.items) ? 1 : 0;
        bins[i]![2] += 1;
      }
    }
  }
  return fromBins(bins);
}

// ------------------------------------------------------------------ selection (select.rs)

const lg = (x: number) => (x > 0 ? Math.log2(x) : 0);
const ulogu = (u: number) => (u > 0 ? u * Math.log2(u) : 0);
const entropyBits = (h: number, m: number) => {
  const n = h + m;
  const part = (x: number) => (x > 0 ? -x * lg(x / n) : 0);
  return part(h) + part(m);
};

type Shape = "run" | "read-batch" | "avoid" | "obligation" | "sequence";

export function shapeOf(c: Cand): Shape {
  const ctx = c.items.slice(0, c.cut);
  const cont = c.items.slice(c.cut);
  const lastX = ctx[ctx.length - 1]!;
  if (lastX.fail) return "avoid";
  if (TYPE[cont[0]!.code] === "read") return "read-batch";
  if (lastX.last && TYPE[lastX.code] === "edit") return "obligation";
  if (cont.filter((x) => TYPE[x.code] === "act").length >= 2) return "sequence";
  return "run";
}

class Mdl {
  freq = new Map<string, number>();
  usage = new Map<string, number>();
  n = 0;
  uTotal = 0;
  sSum = 0;
  streams = 0;
  model = 0;
  avail: boolean[][];

  constructor(private traces: Trace[]) {
    for (const t of traces)
      for (const e of t.ev) {
        this.n++;
        this.freq.set(`x:${e.exact}`, (this.freq.get(`x:${e.exact}`) ?? 0) + 1);
        this.freq.set(`c:${e.code}`, (this.freq.get(`c:${e.code}`) ?? 0) + 1);
        this.usage.set(e.exact, (this.usage.get(e.exact) ?? 0) + 1);
      }
    this.uTotal = this.n;
    this.sSum = [...this.usage.values()].reduce((s, u) => s + ulogu(u), 0);
    this.model = [...this.usage.keys()].reduce((s, e) => s - lg(this.freq.get(`x:${e}`)! / this.n), 0);
    this.avail = traces.map((t) => t.ev.map(() => true));
  }

  total() {
    return ulogu(this.uTotal) - this.sSum + this.streams + this.model;
  }

  patternBits(items: Item[]) {
    return items.reduce((s, i) => s - lg(this.freq.get(`c:${i.code}`)! / this.n) + ITEM_BITS, 0) + lg(6);
  }

  /** The gain of adding `sc` in bits; commits it when positive and `commit`. */
  tryAdd(sc: Scored, commit: boolean): { gain: number; hits: number; anchors: number } {
    const { items, cut } = sc.cand;
    const offsets = new Map<string, number>();
    const local: [number, boolean[]][] = [];
    const dec = new Map<string, number>();
    let anchors = 0;
    let hits = 0;
    let refine = 0;
    for (const si of sc.oppList) {
      const t = this.traces[si]!;
      const av = this.avail[si]!.slice();
      const e = ends(t, items.slice(0, cut));
      anchors += e.length;
      let any = false;
      for (const p of e) {
        const chain = chainFrom(t, p, items.slice(cut), av);
        if (!chain) continue;
        any = true;
        hits++;
        let prev = p;
        chain.forEach((q, j) => {
          av[q] = false;
          const k = `${j}:${Math.abs(q - prev)}`;
          offsets.set(k, (offsets.get(k) ?? 0) + 1);
          prev = q;
          const exact = t.ev[q]!.exact;
          dec.set(exact, (dec.get(exact) ?? 0) + 1);
          // Every item here is a prototype label: refining it to the exact label costs bits.
          refine -= lg(this.freq.get(`x:${exact}`)! / this.freq.get(`c:${items[cut + j]!.code}`)!);
        });
      }
      if (any) local.push([si, av]);
    }
    if (hits === 0) return { gain: 0, hits, anchors };
    const before = this.total();
    let sSum = this.sSum;
    let model = this.model + this.patternBits(items);
    let covered = 0;
    for (const [e, c] of dec) {
      covered += c;
      const u = this.usage.get(e)!;
      sSum += ulogu(u - c) - ulogu(u);
      if (u - c <= 0) model -= -lg(this.freq.get(`x:${e}`)! / this.n);
    }
    const uTotal = this.uTotal - covered;
    const offsetBits = [...offsets.values()].reduce((s, c) => s - c * lg(c / hits), 0);
    const stream = entropyBits(hits, anchors - hits) + offsetBits + refine;
    const after = ulogu(uTotal) - sSum + this.streams + stream + model;
    const gain = before - after;
    if (gain > 0 && commit) {
      for (const [e, c] of dec) this.usage.set(e, this.usage.get(e)! - c);
      this.sSum = sSum;
      this.model = model;
      this.uTotal = uTotal;
      this.streams += stream;
      for (const [si, av] of local) this.avail[si] = av;
    }
    return { gain, hits, anchors };
  }
}

export interface Table {
  shape: Shape;
  runner: string | null;
  /** In the order they were tried: first bound, and the gain when accepted (0 when not). */
  entries: { sc: Scored; bound: number; gain: number; kept: boolean }[];
}

function select(traces: Trace[], scored: Scored[]): Table[] {
  const order = scored
    .filter((s) => s.passes)
    .sort((a, b) => b.value - a.value || b.support - a.support || (a.cand.text < b.cand.text ? -1 : 1));
  const tables = new Map<string, Table>();
  for (const sc of order) {
    const shape = shapeOf(sc.cand);
    const runner = shape === "avoid" ? PROTOS[sc.cand.items.slice(0, sc.cand.cut).reverse().find((x) => x.fail)!.code].label : null;
    const k = `${shape}|${runner ?? ""}`;
    if (!tables.has(k)) tables.set(k, { shape, runner, entries: [] });
    tables.get(k)!.entries.push({ sc, bound: 0, gain: 0, kept: false });
  }
  for (const table of tables.values()) {
    const mdl = new Mdl(traces);
    table.entries.forEach((e) => (e.bound = mdl.tryAdd(e.sc, false).gain));
    let bound = table.entries.map((e, pos) => ({ g: e.bound, pos, e })).filter((x) => x.g > 0);
    while (bound.length) {
      let best = 0;
      bound.forEach((x, i) => {
        const b = bound[best]!;
        if (x.g > b.g || (x.g === b.g && x.pos < b.pos)) best = i;
      });
      const cur = bound.splice(best, 1)[0]!;
      const g = mdl.tryAdd(cur.e.sc, false).gain;
      if (g <= 0) continue;
      if (bound.every((x) => g > x.g || (g === x.g && cur.pos < x.pos))) {
        mdl.tryAdd(cur.e.sc, true);
        cur.e.gain = g;
        cur.e.kept = true;
      } else bound.push({ ...cur, g });
    }
  }
  return [...tables.values()];
}

// ------------------------------------------------------------------ the pipeline

export function run() {
  const traces = TRACES_05;
  const scored = CANDS.map((c) => score(traces, c));
  const cal = calibrate(traces);
  for (const sc of scored) {
    const m = Math.min(1 - 1e-6, Math.max(1e-6, apply(cal, sc.conf)));
    const c = sc.alpha + sc.beta;
    sc.confCal = m;
    sc.confLbCal = betaGrid(m * c, (1 - m) * c).q05;
  }
  const tables = select(traces, scored);
  const runners = new Map<string, number>();
  for (const t of traces) for (const r of new Set(t.ev.filter((e) => e.code === "F").map((e) => e.exact))) runners.set(r, (runners.get(r) ?? 0) + 1);
  const compiled = compile(scored, tables, runners, traces.length);
  return { traces, scored, cal, tables, runners, compiled, days: new Set(traces.map((t) => t.day)).size };
}

// ------------------------------------------------------------------ compile (compile/shapes.rs)

type Lamp = "fire" | "prime" | "inhibit";

export interface Compiled {
  sc: Scored;
  shape: string;
  lamp: Lamp | null;
  /** Why it is not delivered, if it is not. */
  dropped: string | null;
  /** The runner of an avoid, and whether it is announced at session start. */
  atStart?: boolean;
  share?: number;
}

/** The first candidates' continuation commands (the dominant spellings). */
const DIRECTS: Record<string, Code[]> = { seq: ["G", "C"], obl: ["G"], ctx: ["C"], npm: ["F"], avoid: ["C"] };

function compile(scored: Scored[], tables: Table[], runners: Map<string, number>, traces: number): Compiled[] {
  const selected = new Set(tables.flatMap((t) => t.entries.filter((e) => e.kept).map((e) => e.sc.cand.key)));
  const out: Compiled[] = [];
  for (const sc of scored) {
    if (!sc.passes) continue;
    const c = sc.cand;
    const shape = shapeOf(c);
    const context = c.cut >= 2;
    const base = { sc, shape: context ? `${shape} · context` : shape };
    // Contexts compile from selected reflexes only; every other shape also from the gated rest.
    if (context && !selected.has(c.key)) {
      out.push({ ...base, lamp: null, dropped: "a context that saves no bits is not compiled" });
      continue;
    }
    if (sc.variantShare < DOMINANT) {
      out.push({ ...base, lamp: null, dropped: `no spelling holds ${DOMINANT} of its runs` });
      continue;
    }
    if (shape === "avoid") {
      const runner = PROTOS[c.items[0]!.code].label.replace(" ✗", "");
      const share = (runners.get(runner) ?? 0) / traces;
      const atStart = share >= ANNOUNCE_AT_START;
      const lamp: Lamp | null = atStart ? "inhibit" : sc.confLb >= GREEN_LB ? "fire" : sc.conf >= YELLOW_RATE ? "prime" : null;
      out.push({ ...base, lamp, dropped: null, atStart, share });
      continue;
    }
    // A sequence or an after-edit route is provisional: prime until its evidence is counted again
    // on its own trigger. A context keeps the reflex's own posterior.
    const provisional = !context;
    const lamp: Lamp | null = provisional ? "prime" : sc.confLb >= GREEN_LB ? "fire" : sc.conf >= YELLOW_RATE ? "prime" : null;
    out.push({ ...base, lamp, dropped: lamp ? null : "below the prime rate" });
  }
  // Interlocking before emission: a run or sequence directing a runner an avoid names is dropped.
  const avoided = out.filter((x) => x.shape === "avoid" && x.lamp).map((x) => x.sc.cand.items[0]!.code);
  for (const x of out)
    if (x.shape !== "avoid" && !x.dropped && (DIRECTS[x.sc.cand.key] ?? []).some((code) => avoided.includes(code)))
      x.dropped = `directs ${PROTOS.F.label.replace(" ✗", "")}, which an avoid names`;
  return out;
}

// ------------------------------------------------------------------ the scene

const CAM: Record<"cards" | "axis" | "tables" | "reflexes" | "track", CamSpec> = {
  cards: { target: [-0.6, 0, 0], yaw: -0.08, pitch: 0.1, dist: 21 },
  axis: { target: [0, 0, 0], yaw: 0, pitch: 0.08, dist: 21 },
  tables: { target: [0, 0.4, 0], yaw: 0, pitch: 0.12, dist: 21 },
  reflexes: { target: [-0.6, 0, 0], yaw: -0.06, pitch: 0.1, dist: 21 },
  track: { target: [0, 0, 0], yaw: 0, pitch: 0.1, dist: 22 },
};

/** Short names, and how each compiled reflex reads. */
const NAME: Record<string, string> = {
  seq: "gen → check after an edit",
  obl: "gen after the last edit",
  ctx: "[edit, gen] → check",
  seq2: "[prompt, edit] → gen → check",
  avoid: "npm test ✗ → make check",
  npm: "[read, edit src] → npm test",
  status: "git status → make check",
  curl: "curl → make check",
};
const READS: Record<string, { trigger: string; step: string }> = {
  seq: { trigger: "after an edit to registry/*.csv", step: "make gen, then make check" },
  obl: { trigger: "the last edit to registry/*.csv", step: "make gen before finishing" },
  ctx: { trigger: "after [edit registry/*.csv, make gen]", step: "make check" },
  avoid: { trigger: "at session start", step: "npm test fails here · make check works" },
  npm: { trigger: "after [read src/*.ts, edit src/*.ts]", step: "npm test" },
};
const SHAPE_COL: Record<string, number> = { sequence: -6.4, obligation: -2.1, run: 2.1, avoid: 6.4 };
const TABLE_NAME: Record<string, string> = {
  seq: "edit → gen → check",
  obl: "last edit → gen",
  ctx: "[edit, gen] → check",
  seq2: "[prompt, edit] → gen → check",
  avoid: "npm test ✗ → check",
  npm: "[read, edit] → npm test",
};

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function build(stage: Stage): Built {
  const r = run();
  const S = r.scored;
  const keys = S.map((s) => s.cand.key);
  const by = new Map(S.map((s) => [s.cand.key, s]));
  const comp = new Map(r.compiled.map((c) => [c.sc.cand.key, c]));
  const entry = new Map(r.tables.flatMap((t) => t.entries.map((e) => [e.sc.cand.key, { ...e, table: t }] as const)));
  const passing = S.filter((s) => s.passes);
  const failing = S.filter((s) => !s.passes);
  const kept = [...entry.values()].filter((e) => e.kept);
  const fell = [...entry.values()].filter((e) => !e.kept);
  const delivered = r.compiled.filter((c) => c.lamp && !c.dropped);
  const interlocked = r.compiled.filter((c) => c.lamp && c.dropped);
  const lead = by.get("seq")!;

  // Dots: one per candidate, a ghost for its raw confidence, and three per compiled reflex.
  for (const k of keys) {
    stage.dot(k, 0.2);
    stage.dot(`${k}.raw`, 0.12);
    stage.dot(`${k}.a`, 0.16);
    stage.dot(`${k}.b`, 0.26);
    stage.dot(`${k}.c`, 0.16);
  }
  stage.dot("why.a", 0.2);
  stage.dot("why.b", 0.2);

  // Positions.
  const row = (i: number, n = keys.length, top = 4.3) => top - (i * (2 * top)) / (n - 1);
  const cardPos = (k: string): V3 => [-6.6, row(keys.indexOf(k)), 0];
  const gatePos = (k: string): V3 => (by.get(k)!.passes ? cardPos(k) : [-6.6 + 0.8, row(keys.indexOf(k)) - 0.4, -1.5]);
  const ax = (x: number) => -6.5 + ((x - 0.5) / 0.5) * 10.5;
  const axisRow = (k: string) => row(passing.findIndex((s) => s.cand.key === k), passing.length, 3.2);
  const tablePos = new Map<string, V3>();
  const tableKept = new Map<string, V3>();
  for (const t of r.tables) {
    const x = SHAPE_COL[t.shape]!;
    t.entries.forEach((e, i) => {
      tablePos.set(e.sc.cand.key, [x, 0.2 - i * 1.5, 0]);
      tableKept.set(e.sc.cand.key, e.kept ? [x, 2.4 - t.entries.filter((f) => f.kept).indexOf(e) * 1.5, 0] : [x, -3.6, -1]);
    });
  }
  const rx = r.compiled.filter((c) => c.lamp);
  const rxRow = (k: string) => row(rx.findIndex((c) => c.sc.cand.key === k), rx.length, 3.6);
  const RX = [-8.2, -7, -5.8];

  // Links: the calibration shift, the axis, the tables' frames, each reflex arc, the interlock, and
  // the track folder and the file beside it.
  const calKeys = passing.map((s) => s.cand.key);
  stage.link("shift", calKeys.map((k) => [`${k}.raw`, k]), "ink3");
  stage.link("axis", [[[ax(0.5), -4.3, 0], [ax(1), -4.3, 0]], ...[0.5, 0.6, 0.7, 0.8, 0.9, 1].map((x): [V3, V3] => [[ax(x), -4.3, 0], [ax(x), -4.5, 0]])], "ink3");
  stage.link("prime", [[[ax(YELLOW_RATE), -4.3, 0], [ax(YELLOW_RATE), 3.8, 0]]], "prime");
  const frame = (x: number, y0: number, y1: number): [V3, V3][] => {
    const [a, b] = [x - 1.9, x + 1.9];
    return [
      [[a, y0, 0], [b, y0, 0]],
      [[b, y0, 0], [b, y1, 0]],
      [[b, y1, 0], [a, y1, 0]],
      [[a, y1, 0], [a, y0, 0]],
    ];
  };
  stage.link("tables", Object.values(SHAPE_COL).flatMap((x) => frame(x, 3.4, -2.3)), "ink3");
  stage.link("arcs", rx.flatMap((c) => [[`${c.sc.cand.key}.a`, `${c.sc.cand.key}.b`], [`${c.sc.cand.key}.b`, `${c.sc.cand.key}.c`]] as [string, string][]), "accent");
  stage.link("interlock", interlocked.map((c) => ["avoid.b", `${c.sc.cand.key}.b`] as [string, string]), "ink2");
  const box = (x0: number, x1: number, y0: number, y1: number): [V3, V3][] => [
    [[x0, y0, 0], [x1, y0, 0]],
    [[x1, y0, 0], [x1, y1, 0]],
    [[x1, y1, 0], [x0, y1, 0]],
    [[x0, y1, 0], [x0, y0, 0]],
  ];
  stage.link("folders", [...box(-7.8, -3.2, 3.2, -3.4), ...box(-2.2, 7.4, 3.2, -3.4)], "ink3");

  // Labels.
  for (const s of S) {
    const k = s.cand.key;
    stage.label(`card.${k}`, `${esc(s.cand.text)}<br><span class="how-dim">${s.support} of ${s.opp} traces</span>`, k, { cls: "chip", dx: 14, dy: -17 });
    const why = s.passes
      ? `✓ ${esc(NAME[k]!)} <span class="how-dim">· ${s.guard ? `guard LB ${f2(s.confLb)}` : `V ${f2(s.value)}`}${s.consistency !== null ? ` · consistency ${f2(s.consistency)}` : ` · contrast ${f2(s.contrast)}`}</span>`
      : `✗ ${esc(NAME[k]!)} <span class="how-dim">· ${esc(s.gate)}</span>`;
    stage.label(`gate.${k}`, why, k, { cls: s.passes ? "chip" : "note", dx: 14, dy: -9 });
    stage.label(`axis.${k}`, `${esc(NAME[k]!)} <span class="how-dim">${f2(s.conf)} → ${f2(s.confCal)}</span>`, k, { dx: 12, dy: -18 });
    const e = entry.get(k);
    if (e) {
      stage.label(`bits.${k}`, esc(TABLE_NAME[k]!), k, { dy: 12, center: true });
      stage.label(`gain.${k}`, e.kept ? `−${e.gain.toFixed(1)} bits` : "saves 0 bits · falls away", k, { cls: "note", dy: 28, center: true });
    }
    const c = comp.get(k);
    if (c?.lamp) {
      const rd = READS[k]!;
      stage.label(
        `rx.${k}`,
        `<small>${esc(c.shape.toUpperCase())} · ${esc(rd.trigger)}</small><span class="how-lamp ${c.lamp}">●</span> ${c.lamp} · ${esc(rd.step)}`,
        `${k}.c`,
        { cls: "chip big", dx: 16, dy: -20 },
      );
      if (c.dropped) stage.label(`drop.${k}`, `dropped · ${esc(c.dropped)}`, `${k}.b`, { cls: "note", dx: 16, dy: 20 });
    }
  }
  for (const [shape, x] of Object.entries(SHAPE_COL)) {
    const t = r.tables.find((tb) => tb.shape === shape);
    const head = shape === "avoid" && t?.runner ? `avoid · ${esc(t.runner.replace(" ✗", ""))}` : shape;
    stage.label(`col.${shape}`, `<small>CODE TABLE</small>${head}`, [x, 3.4, 0], { cls: "chip", dy: -44, center: true });
  }
  stage.label("ax.lo", "0.50", [ax(0.5), -4.5, 0], { dy: 4, center: true });
  stage.label("ax.hi", "1.00", [ax(1), -4.5, 0], { dy: 4, center: true });
  stage.label("ax.name", "posterior mean · raw → calibrated", [0, -4.5, 0], { dy: 4, center: true });
  stage.label("ax.prime", "prime needs 0.50", [ax(YELLOW_RATE), 3.8, 0], { cls: "note", dy: -16, center: true });
  stage.label("f.why", "<small>BESIDE IT · PEOPLE ONLY</small>.why.json", [-5.5, 3.2, 0], { cls: "chip", dy: -44, center: true });
  stage.label("f.track", "<small>ONE TRACK</small>&lt;repo&gt;/learned", [2.6, 3.2, 0], { cls: "chip", dy: -44, center: true });
  stage.label(
    "report",
    `<small>NEXT</small>each is judged again on the traces,<br>then weighed by what telling changes (uplift)`,
    [2.6, -3.4, 0],
    { cls: "chip", dy: 12, center: true },
  );
  stage.label("why.a", "pattern · backoff · gain in bits", "why.a", { dx: 12, dy: -8 });
  stage.label("why.b", "lift · confidence · example traces", "why.b", { dx: 12, dy: -8 });
  for (const c of delivered) stage.label(`sh.${c.sc.cand.key}`, esc(c.shape), `${c.sc.cand.key}.b`, { dy: 14, center: true });

  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    const labels: string[] = [];
    for (const sc of S) {
      const k = sc.cand.key;
      const e = entry.get(k);
      const c = comp.get(k);
      if (s === 0) {
        dots[k] = { pos: cardPos(k), color: "soft", glow: 0.35 };
        labels.push(`card.${k}`);
      } else if (s === 1) {
        dots[k] = sc.passes ? { pos: gatePos(k), color: "soft", glow: 0.45 } : { pos: gatePos(k), color: "ink3", alpha: 0.5, glow: 0 };
        labels.push(`gate.${k}`);
      } else if (s === 2) {
        if (!sc.passes) continue;
        const y = axisRow(k);
        dots[`${k}.raw`] = { pos: [ax(sc.conf), y, 0], color: "ink3", alpha: 0.8, glow: 0 };
        dots[k] = { pos: [ax(sc.confCal), y, 0], color: "accent", glow: 0.6 };
        labels.push(`axis.${k}`);
      } else if (s === 3 || s === 4) {
        if (!e) continue;
        const pos = s === 3 ? tablePos.get(k)! : tableKept.get(k)!;
        const down = s === 4 && !e.kept;
        dots[k] = { pos, color: down ? "ink3" : "accent", alpha: down ? 0.45 : 1, scale: s === 4 && e.kept ? 1.25 : 1, glow: down ? 0 : 0.6 };
        labels.push(`bits.${k}`);
        if (s === 4) labels.push(`gain.${k}`);
      } else if (c?.lamp && s >= 5) {
        const y = rxRow(k);
        const off = s >= 6 && c.dropped;
        const lampColor: DotLook["color"] = off ? "ink3" : c.lamp;
        if (s <= 6) {
          const sink = off ? -0.35 : 0;
          dots[`${k}.a`] = { pos: [RX[0]!, y + sink, 0], color: "soft", alpha: off ? 0.35 : 0.9, glow: 0.2 };
          dots[`${k}.b`] = { pos: [RX[1]!, y + sink, 0], color: lampColor, alpha: off ? 0.5 : 1, glow: off ? 0 : 0.9 };
          dots[`${k}.c`] = { pos: [RX[2]!, y + sink, 0], color: "soft", alpha: off ? 0.35 : 0.9, glow: 0.2 };
          labels.push(`rx.${k}`);
          if (s === 6 && c.dropped) labels.push(`drop.${k}`);
        } else if (!c.dropped) {
          const i = delivered.findIndex((x) => x.sc.cand.key === k);
          const x0 = -1.4 + (i % 2) * 4.4;
          const y0 = 1.6 - Math.floor(i / 2) * 2.4;
          dots[`${k}.a`] = { pos: [x0, y0, 0], color: "soft", glow: 0.2 };
          dots[`${k}.b`] = { pos: [x0 + 0.9, y0, 0], color: c.lamp, glow: 0.9 };
          dots[`${k}.c`] = { pos: [x0 + 1.8, y0, 0], color: "soft", glow: 0.2 };
        }
      }
    }
    if (s === 7) {
      dots["why.a"] = { pos: [-7.1, 1.6, 0], color: "ink2", glow: 0.3 };
      dots["why.b"] = { pos: [-7.1, -0.8, 0], color: "ink2", glow: 0.3 };
      labels.push("why.a", "why.b", "f.why", "f.track", "report", ...delivered.map((c) => `sh.${c.sc.cand.key}`));
    }
    if (s === 2) labels.push("ax.lo", "ax.hi", "ax.name", "ax.prime");
    if (s === 3 || s === 4) labels.push(...Object.keys(SHAPE_COL).map((x) => `col.${x}`));
    const cam = s <= 1 ? CAM.cards : s === 2 ? CAM.axis : s <= 4 ? CAM.tables : s <= 6 ? CAM.reflexes : CAM.track;
    return {
      dots,
      labels,
      cam,
      links: {
        shift: s === 2 ? 0.7 : 0,
        axis: s === 2 ? 0.6 : 0,
        prime: s === 2 ? 0.5 : 0,
        tables: s === 3 || s === 4 ? 0.35 : 0,
        arcs: s >= 5 ? 0.75 : 0,
        interlock: s === 6 ? 0.8 : 0,
        folders: s === 7 ? 0.4 : 0,
      },
    };
  }

  // Figures.
  const font = "10px 'Red Hat Mono', ui-monospace, monospace";
  const reliability: Step["figure"] = (g, w, h, pal) => {
    const padL = 6;
    const padB = 14;
    const X = (x: number) => padL + x * (w - padL - 6);
    const Y = (y: number) => h - padB - y * (h - padB - 14);
    g.strokeStyle = pal.css("ink3");
    g.globalAlpha = 0.5;
    g.setLineDash([3, 3]);
    g.beginPath();
    g.moveTo(X(0), Y(0));
    g.lineTo(X(1), Y(1));
    g.stroke();
    g.setLineDash([]);
    g.globalAlpha = 1;
    const maxN = Math.max(...r.cal.bins.map((b) => b[2]));
    g.fillStyle = pal.css("ink3");
    for (const [ps, os, n] of r.cal.bins) {
      if (!n) continue;
      g.globalAlpha = 0.65;
      g.beginPath();
      g.arc(X(ps / n), Y(os / n), 1.2 + 2.6 * Math.sqrt(n / maxN), 0, Math.PI * 2);
      g.fill();
    }
    g.globalAlpha = 1;
    g.strokeStyle = pal.css("accent");
    g.lineWidth = 2;
    g.beginPath();
    r.cal.points.forEach(([x, y], i) => (i ? g.lineTo(X(x), Y(y)) : g.moveTo(X(x), Y(y))));
    g.stroke();
    g.lineWidth = 1;
    g.font = font;
    g.textBaseline = "top";
    g.fillStyle = pal.css("ink3");
    g.textAlign = "left";
    g.fillText("0", X(0), h - padB + 2);
    g.textAlign = "right";
    g.fillText("1", X(1), h - padB + 2);
    g.textAlign = "center";
    g.fillText("predicted → · dots: bins · line: isotonic map", w / 2, h - padB + 2);
    g.textAlign = "right";
    g.fillText(`${r.cal.pairs.toLocaleString("en")} held-out pairs`, X(1), 2);
    g.textAlign = "left";
    g.fillText("observed ↑", X(0) + 2, 2);
  };
  const gains: Step["figure"] = (g, w, h, pal) => {
    const list = [...entry.values()];
    const max = Math.max(...list.map((e) => e.bound));
    const padB = 14;
    const bw = (w - 8) / list.length;
    g.font = font;
    list.forEach((e, i) => {
      const x = 4 + i * bw;
      const bh = (v: number) => (v / max) * (h - padB - 14);
      g.strokeStyle = pal.css("ink3");
      g.strokeRect(x + 6, h - padB - bh(e.bound), bw - 12, bh(e.bound));
      if (e.kept) {
        g.fillStyle = pal.css("accent");
        g.fillRect(x + 6, h - padB - bh(e.gain), bw - 12, bh(e.gain));
      }
      g.fillStyle = pal.css(e.kept ? "accent" : "ink3");
      g.textAlign = "center";
      g.textBaseline = "bottom";
      g.fillText(e.kept ? e.gain.toFixed(1) : "0", x + bw / 2, h - padB - bh(Math.max(e.bound, e.gain)) - 1);
      g.textBaseline = "top";
      g.fillStyle = pal.css("ink3");
      g.font = "9px 'Red Hat Mono', ui-monospace, monospace";
      g.fillText(e.table.shape, x + bw / 2, h - padB + 2);
      g.font = font;
    });
    g.textAlign = "left";
    g.textBaseline = "top";
    g.fillStyle = pal.css("ink3");
    g.fillText("bits saved · outline: first estimate", 4, 0);
  };

  const text: Omit<Step, "state">[] = [
    {
      title: "Candidates arrive with their evidence",
      body: `Mining hands over closed patterns, each read as a prediction: after this context, this follows. Here are ${S.length} sample candidates over ${r.traces.length} sample traces, among them the registry pattern from the previous chapter. Support counts distinct traces; opportunities are the traces where the context occurs.`,
      chips: [{ text: `${S.length} sample candidates` }, { text: `≤ ${MAX_CLOSED.toLocaleString("en")} closed patterns scored` }, { text: `${r.traces.length} traces over ${r.days} days` }],
    },
    {
      title: "Frequent is not useful: the gates",
      body: `Confidence is a Beta posterior shrunk toward the parent context (κ ${KAPPA}), with evidence halved every ${HALF_LIFE} days, and judged by its 5% quantile. ${lead.cand.text.replace(/ → .*/, "")} predicts make gen then make check in ${lead.support} of ${lead.opp} traces, lower bound ${f2(lead.confLb)}. ${failing.map((s) => `${NAME[s.cand.key]} fails on ${s.gate.split(" ")[0]}`).join("; ")}.`,
      chips: [
        { text: `support ≥ ${MIN_SUPPORT} · days ≥ ${MIN_DAYS}` },
        { text: `contrast ≥ ${f2(CONTRAST)} · or consistency ≥ ${CONSISTENCY} after a failure` },
        { text: `V > 0 · guards LB ≥ ${f2(GUARD_LB)}` },
        { text: `${passing.length} of ${S.length} pass`, ok: true },
      ],
    },
    {
      title: "Confidence is calibrated",
      body: `On ${CAL_FOLDS} task-grouped inner folds, every candidate's posterior mean is compared with what happened in the fold it did not see. Isotonic regression over ${CAL_BINS} bins bends the raw means onto the outcomes. Each candidate keeps both: the calibrated mean and lower bound are recorded beside the raw ones, which the compiler still reads.`,
      chips: [{ text: `${CAL_BINS} bins · pool adjacent violators` }, { text: `re-mined per fold below ${CAL_REMINE_MAX} traces` }, { text: "ECE 0.017 all pairs · 0.116 selected (demo-haiku45)" }],
      figure: reliability,
    },
    {
      title: "One code table per delivery shape",
      body: `Selection is by compression: a reflex earns its place by describing the traces in fewer bits. Candidates that would deliver different directives never compete, so each delivery shape gets its own table, and avoids get one per failing runner. A pattern costs its items' codes plus ${ITEM_BITS} bits per item and log2 6 for its length.`,
      chips: [{ text: `${r.tables.length} tables` }, { text: `${ITEM_BITS} bits per item + log2 6` }, { text: "SQS-style MDL" }],
    },
    {
      title: "Compression chooses",
      body: `Each table fills best gain first, re-checking a candidate's gain before accepting it. ${kept.length} candidates save bits. ${fell.map((e) => NAME[e.sc.cand.key]).join(", ")} would code the same make gen and make check that the shorter sequence already coded, so it saves nothing and falls away.`,
      chips: [{ text: "lazy greedy, best gain first" }, { text: `${kept.length} kept · ${fell.length} saves 0 bits` }, { text: `≤ ${MAX_REFLEXES} reflexes · ≤ ${MAX_UNSELECTED.toLocaleString("en")} gated extras compile` }],
      figure: gains,
    },
    {
      title: "Winners compile to shapes",
      body: `Each winner compiles to a delivery shape, and its strength is decided here. The context route's lower bound, ${f2(by.get("ctx")!.confLb)}, reaches the ${f2(GREEN_LB)} fire bar. The sequence and the finish line compile provisional: they prime until their evidence is counted again on their own trigger. The dead end is announced at session start, because ${Math.round((comp.get("avoid")!.share ?? 0) * r.traces.length)} of ${r.traces.length} traces run npm test.`,
      chips: [
        { text: `fire: LB ≥ ${f2(GREEN_LB)} and a fire ceiling` },
        { text: `prime: mean ≥ ${f2(YELLOW_RATE)}` },
        { text: `avoid at start: ≥ ${ANNOUNCE_AT_START} of traces run it` },
        { text: `instinct prior κ ${PRIOR_KAPPA} at ${PRIOR_MEAN} · never above prime` },
      ],
    },
    {
      title: "Interlocks drop contradictions",
      body: `${interlocked.map((c) => NAME[c.sc.cand.key]).join(", ")} passed every gate and saved bits, but it directs npm test, the runner the avoid says fails here. A run or sequence that directs an avoided runner is dropped before anything is written.`,
      chips: [{ text: "interlock before emission" }, { text: `${delivered.length} reflexes delivered` }],
    },
    {
      title: "One track, with its reasons",
      body: `The ${delivered.length} survivors are written to one track, <repo>/learned: the graph engine is the only learner, so there is no track per kind of reflex. Beside it, .why.json records why each exists: the mined pattern, how many context items it conditions on, the bits it saved, its lift and confidence, and example traces. adage review reads it; the agent never sees it, and it never travels in a shared pack. Each reflex is then judged again on the traces, next.`,
      chips: [{ text: "tracks/<repo>/learned" }, { text: ".why.json · never rendered to the agent" }, { text: "stable ids: review decisions survive re-learning" }],
    },
  ];

  return {
    source: `${S.length} sample candidates · ${r.traces.length} sample traces`,
    legend: [
      { label: "candidate", role: "soft" },
      { label: "selected", role: "accent" },
      { label: "fire", role: "fire" },
      { label: "prime", role: "prime" },
      { label: "inhibit (avoid)", role: "inhibit" },
      { label: "dropped", role: "ink3" },
    ],
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

export const choose: Chapter = {
  id: "choose",
  title: "Choose and compile",
  status: "Built · the learner",
  kind: "built",
  summary:
    "Candidates are scored as predictions, calibrated, then selected by compression: a reflex earns its place by describing traces in fewer bits. Winners compile to delivery shapes, and their strength is decided at compile time.",
  source: "crates/adage-graph/src/{score,select,compile} · crates/adage-tracks/src/why.rs",
  usesRepo: false,
  build,
};
