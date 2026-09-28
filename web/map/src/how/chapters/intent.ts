// Chapter: group prompts by intent (crates/adage-learn/src/intent.rs, crates/adage-intent). Prompts
// are normalised, embedded, clustered by kNN + Louvain, then split, merged and kept by what their
// traces ran first; each kept family gets a centroid and a calibrated similarity threshold.
//
// The prompts and their first commands are sample data. Their embeddings are stand-ins: unit
// vectors built per family (the real ones come from the pinned model), and the 3D positions are
// illustrative. Everything else runs the real algorithm on them: the normaliser's rules, exact kNN,
// the eps graph, deterministic Louvain, the purity split and merge, the bar, quantisation, the
// Wilson calibration and the contrast gate. Every number a caption states is computed here.

import type { CamSpec, DotLook, SceneState, Stage, V3 } from "../engine";
import { f2, seeded, wilson } from "../sample";
import type { Built, Chapter, Step } from "../types";

// ------------------------------------------------------------------ constants (cited)

// crates/adage-intent/src/normalize.rs
const HEAD_TOKENS = 128;
const MAX_INPUT = 16 * 1024;
// crates/adage-intent/src/cluster.rs Params::default, with min_size = max(min_support, 2)
const K = 10;
const EPS = 0.6;
const RESOLUTION = 2.0;
const MAX_LEVELS = 10;
// crates/adage-intent/src/purity.rs Purity::default, with min_group = max(min_support, 2)
const BAR = 0.6;
const SEP = 0.85;
const MERGE_TAU = 0.8;
const MIN_SHARE = 0.15;
// adage-learn LearnConfig defaults: min_support, intent_ubiquitous, intent_contrast, green_lower_bound, yellow_rate
const MIN_SUPPORT = 5;
const UBIQUITOUS = 0.8;
const CONTRAST = 0.3;
const FIRE_LB = 0.8;
const PRIME_RATE = 0.5;
// adage-tracks model.rs: INTENT_MIN_THRESHOLD, INTENT_MAX_THRESHOLD; adage-intent lib.rs SCALE
const T_MIN = 0.5;
const T_MAX = 0.99;
const SCALE = 10_000;
const MIN = Math.max(MIN_SUPPORT, 2);

// ------------------------------------------------------------------ normalise (normalize.rs)

const FENCE = /```[\s\S]*?```/g;
// Order matters: URLs and tickets before the generic path pattern.
const RULES: [RegExp, string][] = [
  [/https?:\/\/\S+/g, "{url}"],
  [/\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b/g, "{ticket}"],
  [/#\d{1,6}\b/g, "{pr}"],
  [/\b\w[\w\-./]*\/[\w\-./]+\b/g, "{path}"],
  [/\b[0-9a-f]{7,40}\b/g, "{sha}"],
  [/\b\d+\.\d+(?:\.\d+)?\b/g, "{version}"],
  [/\b[A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+\b/g, "{env_var}"],
];

export function normalize(raw: string): { text: string; placeholders: number; fence: boolean } {
  const bytes = new TextEncoder().encode(raw);
  let cut = Math.min(bytes.length, MAX_INPUT);
  while (cut < bytes.length && cut > 0 && (bytes[cut]! & 0xc0) === 0x80) cut--;
  const head = new TextDecoder().decode(bytes.subarray(0, cut));
  const fence = head.includes("```");
  let text = head.replace(FENCE, " [code] ");
  let placeholders = 0;
  for (const [re, token] of RULES) {
    placeholders += text.match(re)?.length ?? 0;
    text = text.replace(re, token);
  }
  return { text: text.split(/\s+/).filter(Boolean).slice(0, HEAD_TOKENS).join(" "), placeholders, fence };
}

// ------------------------------------------------------------------ sample prompts

/** The first directable commands (observe or verify) a trace ran before its first edit. */
type Cmd = "git status" | "make test" | "kubectl logs deploy/dispatch" | "pnpm outdated" | "go list -m -u all" | "git log --oneline";

interface Prompt {
  raw: string;
  /** Which stand-in blob its embedding is drawn around. */
  blob: "T" | "X" | "L" | "J" | "G" | "H1" | "H2" | "N1" | "N2";
  cmds: Cmd[];
}

const GS: Cmd = "git status";
const MT: Cmd = "make test";
const KL: Cmd = "kubectl logs deploy/dispatch";
const PO: Cmd = "pnpm outdated";
const GL: Cmd = "go list -m -u all";
const LOG: Cmd = "git log --oneline";

export const PROMPTS: Prompt[] = [
  { raw: "the dispatch tests fail on main since a3f9c21, fix them", blob: "T", cmds: [GS, MT] },
  { raw: "make test is red after W-4821 landed", blob: "T", cmds: [GS, MT] },
  { raw: "fix the failing test in src/router/holidays.test.ts", blob: "T", cmds: [GS, MT] },
  { raw: "lane pricing tests fail, see https://ci.example.dev/run/8812", blob: "T", cmds: [MT] },
  { raw: "CI fails on #482, the retry test times out", blob: "T", cmds: [GS, MT] },
  { raw: "fix the flaky carrier import test", blob: "T", cmds: [GS, MT, LOG] },
  { raw: "port lookup tests fail with ```AssertionError: expected OSL``` since yesterday", blob: "T", cmds: [GS, MT] },
  { raw: "unit tests for the quote engine are failing", blob: "T", cmds: [GS, MT] },
  { raw: "the ports test suite fails locally, help", blob: "T", cmds: [GS, MT] },
  { raw: "fix the broken test after PAY-311", blob: "T", cmds: [GS, MT] },
  { raw: "explain how the test fixtures are generated", blob: "X", cmds: [GS] },

  { raw: "dispatch latency spiked in prod, find out why", blob: "L", cmds: [GS, KL] },
  { raw: "customers see 503s from the router", blob: "L", cmds: [GS, KL, MT] },
  { raw: "the nightly sync job crashed, investigate", blob: "L", cmds: [GS, MT] },
  { raw: "why is SLA_BREACH_ALERT firing", blob: "L", cmds: [GS] },
  { raw: "carrier webhooks are timing out", blob: "L", cmds: [MT] },

  { raw: "bump react to 18.3.1", blob: "J", cmds: [GS, PO] },
  { raw: "upgrade vite to the latest major", blob: "J", cmds: [GS, PO] },
  { raw: "update the npm dependencies, dependabot flagged #512", blob: "J", cmds: [GS, PO, MT] },
  { raw: "bump typescript to 5.6 in web/map", blob: "J", cmds: [GS, PO] },
  { raw: "upgrade @tanstack/query to 5.59.0", blob: "J", cmds: [PO] },

  { raw: "bump the Go toolchain to 1.23", blob: "G", cmds: [GS, GL] },
  { raw: "upgrade golang.org/x/net to the latest patch", blob: "G", cmds: [GS, GL] },
  { raw: "update go modules, the grpc one is outdated", blob: "G", cmds: [GS, GL, MT] },
  { raw: "bump pgx in services/ledger", blob: "G", cmds: [GS, GL] },
  { raw: "go.mod still pins an old otel, bump it", blob: "G", cmds: [GL] },

  { raw: "what changed in dispatch since Friday?", blob: "H1", cmds: [GS, LOG] },
  { raw: "who touched the lane pricing last week", blob: "H1", cmds: [GS, LOG] },
  { raw: "what did the last release change in routing", blob: "H1", cmds: [GS, LOG] },
  { raw: "summarise the commits since the 2.3.0 release", blob: "H1", cmds: [GS, LOG] },
  { raw: "what changed in registry/ports.csv recently", blob: "H1", cmds: [GS, LOG, MT] },

  { raw: "when did the router start ignoring holidays", blob: "H2", cmds: [GS, LOG] },
  { raw: "find the commit that broke carrier import", blob: "H2", cmds: [GS, LOG, MT] },
  { raw: "which change made quotes slower", blob: "H2", cmds: [GS, LOG] },
  { raw: "bisect the Oslo port regression", blob: "H2", cmds: [GS, LOG] },
  { raw: "when was the retry limit changed to 5", blob: "H2", cmds: [GS, LOG] },

  { raw: "write a haiku about container ships", blob: "N1", cmds: [] },
  { raw: "tidy up the repo description", blob: "N2", cmds: [GS] },
];

/** The prompt that arrives at the hook in the last step. */
export const NEW_PROMPT = "ledger tests fail since 9f1c2ab, fix them";

// ------------------------------------------------------------------ stand-in embeddings

const D = 32;

function unit(v: number[]): number[] {
  const n = Math.hypot(...v);
  return n > 0 ? v.map((x) => x / n) : v;
}

const dot = (a: number[], b: number[]) => a.reduce((s, x, i) => s + x * b[i]!, 0);

interface Blob {
  base: number[];
  /** Noise per dimension (random), or along one private axis per member (`axis` set). */
  noise: number;
  /** The first of the blob's private noise axes. */
  axis?: number;
}

// Each blob is a direction; sub-blobs of one topic share a direction and differ along a second
// one. The history blobs spread along private axes, so their members are exactly as alike inside
// a blob (0.68) and less alike across (0.57, below eps), while the two centres stay 0.82 alike.
const BLOB: Record<Prompt["blob"], Blob> = (() => {
  const e = (i: number, w = 1) => Array.from({ length: D }, (_, j) => (j === i ? w : 0));
  const add = (a: number[], b: number[]) => a.map((x, i) => x + b[i]!);
  return {
    T: { base: e(0), noise: 0.2 },
    X: { base: add(e(0), e(10, 1.0)), noise: 0.05 },
    L: { base: e(1), noise: 0.24 },
    J: { base: add(e(2), e(3, 0.23)), noise: 0.04 },
    G: { base: add(e(2), e(3, -0.23)), noise: 0.04 },
    H1: { base: add(e(4), e(5, 0.3)), noise: 0.716, axis: 20 },
    H2: { base: add(e(4), e(5, -0.3)), noise: 0.716, axis: 26 },
    N1: { base: e(8), noise: 0.2 },
    N2: { base: e(9), noise: 0.2 },
  };
})();

/** Stand-in vectors for a blob's members, with noise in opposite pairs so the spread is centred
 * on the blob's direction, and a display offset (illustrative) that follows the same pairs. */
function standIns(blob: Prompt["blob"], count: number, rnd: () => number): { vec: number[]; jitter: V3 }[] {
  const b = BLOB[blob];
  for (let i = 0; i < 4; i++) rnd(); // a small seed's first draws sit near -1
  const out: { vec: number[]; jitter: V3 }[] = [];
  let prev: number[] = [];
  let pj: V3 = [0, 0, 0];
  for (let k = 0; k < count; k++) {
    let n: number[];
    let j: V3;
    if (k % 2) {
      n = prev.map((x) => -x);
      j = [-pj[0], -pj[1], -pj[2]];
    } else {
      n = b.axis !== undefined
        ? Array.from({ length: D }, (_, d) => (d === b.axis! + k / 2 ? b.noise : 0))
        : Array.from({ length: D }, () => rnd() * b.noise);
      j = [rnd(), rnd(), rnd()];
    }
    prev = n;
    pj = j;
    out.push({ vec: unit(b.base.map((x, i) => x + n[i]!)), jitter: j });
  }
  return out;
}

// ------------------------------------------------------------------ clustering (cluster.rs)

/** Exact top-k neighbours, self excluded, ordered by (similarity desc, index asc). */
function knn(vecs: number[][], k: number): [number, number][][] {
  const n = vecs.length;
  const kk = Math.min(k, n - 1);
  return vecs.map((a, i) => {
    const row: [number, number][] = [];
    for (let j = 0; j < n; j++) if (j !== i) row.push([j, dot(a, vecs[j]!)]);
    row.sort((x, y) => y[1] - x[1] || x[0] - y[0]);
    return row.slice(0, kk);
  });
}

/** Undirected weighted graph from kNN edges with similarity >= eps. */
function graph(nb: [number, number][][], eps: number): Map<number, number>[] {
  const adj = nb.map(() => new Map<number, number>());
  nb.forEach((row, i) => {
    for (const [j, s] of row) {
      if (s < eps) break;
      adj[i]!.set(j, s);
      adj[j]!.set(i, s);
    }
  });
  return adj;
}

/** Deterministic Louvain with a resolution: index-order visits, strict gains, ≤ `levels` levels. */
function louvain(adj0: Map<number, number>[], resolution: number, levels: number): number[] {
  const n = adj0.length;
  let members: number[][] = adj0.map((_, i) => [i]);
  let g: [number, number][][] = adj0.map((m) => [...m.entries()].sort((a, b) => a[0] - b[0]));
  for (let level = 0; level < levels; level++) {
    const nn = g.length;
    const deg = g.map((a) => a.reduce((s, e) => s + e[1], 0));
    const m2 = deg.reduce((s, x) => s + x, 0);
    if (m2 === 0) break;
    const comm = g.map((_, i) => i);
    const tot = deg.slice();
    let movedAny = false;
    for (;;) {
      let improved = false;
      for (let i = 0; i < nn; i++) {
        const ci = comm[i]!;
        const links = new Map<number, number>();
        for (const [j, w] of g[i]!) {
          if (j === i) continue;
          links.set(comm[j]!, (links.get(comm[j]!) ?? 0) + w);
        }
        tot[ci]! -= deg[i]!;
        let best = ci;
        let bestGain = (links.get(ci) ?? 0) - (resolution * deg[i]! * tot[ci]!) / m2;
        for (const c of [...links.keys()].sort((a, b) => a - b)) {
          const gain = links.get(c)! - (resolution * deg[i]! * tot[c]!) / m2;
          if (gain > bestGain + 1e-12) {
            best = c;
            bestGain = gain;
          }
        }
        tot[best]! += deg[i]!;
        if (best !== ci) {
          comm[i] = best;
          improved = movedAny = true;
        }
      }
      if (!improved) break;
    }
    if (!movedAny) break;
    const ids = new Map<number, number>();
    for (const c of comm) if (!ids.has(c)) ids.set(c, ids.size);
    const next: number[][] = Array.from({ length: ids.size }, () => []);
    comm.forEach((c, i) => next[ids.get(c)!]!.push(...members[i]!));
    const agg = Array.from({ length: ids.size }, () => new Map<number, number>());
    g.forEach((row, i) => {
      const a = ids.get(comm[i]!)!;
      for (const [j, w] of row) {
        const b = ids.get(comm[j]!)!;
        agg[a]!.set(b, (agg[a]!.get(b) ?? 0) + w);
      }
    });
    g = agg.map((m) => [...m.entries()].sort((x, y) => x[0] - y[0]));
    members = next;
  }
  const out = new Array<number>(n).fill(0);
  members.forEach((mem, c) => mem.forEach((p) => (out[p] = c)));
  return out;
}

/** Clusters smaller than `min` become noise (-1); the rest are renumbered by first member. */
function finish(labels: number[], min: number): number[] {
  const count = new Map<number, number>();
  for (const l of labels) if (l >= 0) count.set(l, (count.get(l) ?? 0) + 1);
  const ids = new Map<number, number>();
  return labels.map((l) => {
    if (l < 0 || count.get(l)! < min) return -1;
    if (!ids.has(l)) ids.set(l, ids.size);
    return ids.get(l)!;
  });
}

// ------------------------------------------------------------------ behaviour (purity.rs)

function membersOf(labels: number[]): Map<number, number[]> {
  const m = new Map<number, number[]>();
  labels.forEach((l, i) => l >= 0 && (m.get(l) ?? m.set(l, []).get(l)!).push(i));
  return new Map([...m.entries()].sort((a, b) => a[0] - b[0]));
}

function centroid(vecs: number[][], mem: number[]): number[] {
  const acc = new Array<number>(D).fill(0);
  for (const i of mem) vecs[i]!.forEach((x, d) => (acc[d] = acc[d]! + x));
  return unit(acc);
}

type Route = string | null;

/** Each family's dominant route (ties: the smallest), its share, and the family size. */
function dominant(labels: number[], routes: Route[]): Map<number, { route: Route; share: number; size: number }> {
  const out = new Map<number, { route: Route; share: number; size: number }>();
  for (const [c, ix] of membersOf(labels)) {
    const count = new Map<string, number>();
    for (const i of ix) if (routes[i]) count.set(routes[i]!, (count.get(routes[i]!) ?? 0) + 1);
    let best: [string, number] | null = null;
    for (const e of [...count.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1))) if (!best || e[1] > best[1]) best = e;
    out.set(c, best ? { route: best[0], share: best[1] / ix.length, size: ix.length } : { route: null, share: 0, size: ix.length });
  }
  return out;
}

/** Leave-one-out nearest-centroid accuracy among route groups. */
function separability(vecs: number[][], groups: number[][]): number {
  const sums = groups.map((g) => {
    const s = new Array<number>(D).fill(0);
    for (const i of g) vecs[i]!.forEach((x, d) => (s[d] = s[d]! + x));
    return s;
  });
  let correct = 0;
  let total = 0;
  groups.forEach((g, gi) => {
    for (const i of g) {
      let best = -1;
      let bs = -Infinity;
      sums.forEach((s, hj) => {
        const c = s.map((a, d) => (hj === gi ? a - vecs[i]![d]! : a));
        const nn = Math.hypot(...c);
        if (nn === 0) return;
        const sim = dot(c, vecs[i]!) / nn;
        if (sim > bs) {
          best = hj;
          bs = sim;
        }
      });
      correct += best === gi ? 1 : 0;
      total++;
    }
  });
  return correct / Math.max(1, total);
}

interface SplitNote {
  family: number;
  sep: number;
  groups: { route: string; size: number }[];
}

function split(vecs: number[][], labels: number[], routes: Route[], notes: SplitNote[]): number[] {
  const out = labels.slice();
  let next = Math.max(-1, ...labels) + 1;
  for (const [c, ix] of membersOf(labels)) {
    const by = new Map<string, number[]>();
    for (const i of ix) if (routes[i]) (by.get(routes[i]!) ?? by.set(routes[i]!, []).get(routes[i]!)!).push(i);
    const min = Math.max(MIN, MIN_SHARE * ix.length);
    const kept = [...by.entries()].sort((a, b) => (a[0] < b[0] ? -1 : 1)).filter(([, g]) => g.length >= min);
    if (kept.length < 2) continue;
    const groups = kept.map(([, g]) => g);
    const sep = separability(vecs, groups);
    if (sep < SEP) continue;
    notes.push({ family: c, sep, groups: kept.map(([r, g]) => ({ route: r, size: g.length })) });
    const cents = groups.map((g) => centroid(vecs, g));
    const ids = [c, ...groups.slice(1).map(() => next++)];
    for (const i of ix) {
      let best = 0;
      cents.forEach((ct, j) => dot(vecs[i]!, ct) > dot(vecs[i]!, cents[best]!) && (best = j));
      out[i] = ids[best]!;
    }
  }
  return finish(out, 1);
}

interface MergeNote {
  a: number;
  b: number;
  cos: number;
  route: string;
}

function merge(vecs: number[][], labels: number[], routes: Route[], notes: MergeNote[]): number[] {
  let out = labels.slice();
  for (;;) {
    const dom = dominant(out, routes);
    const mem = membersOf(out);
    const ids = [...dom.entries()].filter(([, v]) => v.route).map(([c]) => c);
    const cents = new Map(ids.map((c) => [c, centroid(vecs, mem.get(c)!)]));
    let best: MergeNote | null = null;
    ids.forEach((x, a) => {
      for (const y of ids.slice(a + 1)) {
        if (dom.get(x)!.route !== dom.get(y)!.route) continue;
        const s = dot(cents.get(x)!, cents.get(y)!);
        if (s >= MERGE_TAU && (!best || s > best.cos)) best = { a: x, b: y, cos: s, route: dom.get(x)!.route! };
      }
    });
    if (!best) return finish(out, 1);
    const { a, b } = best;
    notes.push(best);
    out = out.map((l) => (l === b ? a : l));
  }
}

// ------------------------------------------------------------------ the pipeline (intent.rs)

export interface Family {
  label: number;
  members: number[];
  route: string;
  share: number;
  kept: boolean;
  centroid: number[];
  sims: number[];
  /** Calibration: every candidate t with its Wilson lower bound; the chosen one. */
  curve: { t: number; lb: number; sup: number; opp: number }[];
  t: number;
  lb: number;
  support: number;
  opp: number;
  rate: number;
  rateOut: number;
  /** Whether it became an intent reflex (support and contrast). */
  reflex: boolean;
  lamp: "fire" | "prime" | null;
}

const SEED = { v: 1 };

export function run() {
  const blobs = [...new Set(PROMPTS.map((p) => p.blob))];
  const drawn = new Map(blobs.map((b, j) => [b, standIns(b, PROMPTS.filter((p) => p.blob === b).length, seeded(SEED.v + j * 7919))]));
  const items = PROMPTS.map((p, i) => {
    const n = normalize(p.raw);
    return { ...p, i, norm: n.text, placeholders: n.placeholders, ...drawn.get(p.blob)!.shift()! };
  });
  // Canonical order: normalised text (then input order, standing in for the trace id).
  const order = items.slice().sort((a, b) => (a.norm < b.norm ? -1 : a.norm > b.norm ? 1 : a.i - b.i));
  const vecs = order.map((x) => x.vec);
  const n = order.length;

  // The route: the first command that is not ubiquitous.
  const df = new Map<string, number>();
  for (const it of order) for (const c of new Set(it.cmds)) df.set(c, (df.get(c) ?? 0) + 1);
  const common = (c: string) => n >= 5 && df.get(c)! > UBIQUITOUS * n;
  const routes: Route[] = order.map((it) => it.cmds.find((c) => !common(c)) ?? null);
  const ubiquitous = [...df.entries()].filter(([c]) => common(c)).map(([c, k]) => ({ cmd: c, traces: k }));

  const nb = knn(vecs, K);
  const adj = graph(nb, EPS);
  const edgesAll: [number, number][] = [];
  const edgesEps: [number, number][] = [];
  nb.forEach((row, i) =>
    row.forEach(([j, s]) => {
      if (i < j || !nb[j]!.some(([x]) => x === i)) (s >= EPS ? edgesEps : edgesAll).push([i, j]);
    }),
  );
  const communities = louvain(adj, RESOLUTION, MAX_LEVELS);
  const clusters = finish(communities, MIN);
  const splitNotes: SplitNote[] = [];
  const mergeNotes: MergeNote[] = [];
  const afterSplit = split(vecs, clusters, routes, splitNotes);
  const afterMerge = merge(vecs, afterSplit, routes, mergeNotes);
  const mem = membersOf(afterMerge);
  const ran = order.map((it) => (r: string) => it.cmds.includes(r as Cmd));

  const families: Family[] = [];
  for (const [c, d] of dominant(afterMerge, routes)) {
    const members = mem.get(c)!;
    const q = centroid(vecs, members).map((x) => Math.max(-SCALE, Math.min(SCALE, Math.round(x * SCALE))));
    const cent = unit(q.map((x) => x / SCALE));
    const sims = vecs.map((v) => dot(v, cent));
    const kept = d.route !== null && d.share >= BAR && d.size >= MIN;
    const fam: Family = { label: c, members, route: d.route ?? "", share: d.share, kept, centroid: cent, sims, curve: [], t: 0, lb: 0, support: 0, opp: 0, rate: 0, rateOut: 0, reflex: false, lamp: null };
    families.push(fam);
    if (!kept) continue;
    const route = d.route!;
    const floor4 = (x: number) => Math.floor(x * 1e4) / 1e4;
    const ts = [...new Set(members.map((i) => floor4(sims[i]!)).filter((s) => s >= T_MIN))].sort((a, b) => b - a);
    let best: { t: number; lb: number; sup: number; opp: number } | null = null;
    for (const t0 of ts) {
      const t = Math.min(t0, T_MAX);
      const opp = sims.filter((s) => s >= t).length;
      const sup = sims.filter((s, i) => s >= t && ran[i]!(route)).length;
      const lb = wilson(sup, opp);
      fam.curve.push({ t, lb, sup, opp });
      if (!best || lb > best.lb) best = { t, lb, sup, opp };
    }
    if (!best) continue;
    const outside = n - best.opp;
    const ranOut = sims.filter((s, i) => s < best.t && ran[i]!(route)).length;
    Object.assign(fam, { t: best.t, lb: best.lb, support: best.sup, opp: best.opp, rate: best.sup / best.opp, rateOut: outside ? ranOut / outside : 0 });
    fam.reflex = fam.support >= MIN_SUPPORT && fam.rate - fam.rateOut >= CONTRAST;
    // Observe and verify steps may fire (their ceiling is fire).
    fam.lamp = !fam.reflex ? null : fam.lb >= FIRE_LB ? "fire" : fam.rate >= PRIME_RATE ? "prime" : null;
  }

  // The prompt at the hook: its stand-in embedding is drawn around the test family's blob.
  const probe = standIns("T", 2, seeded(SEED.v + 113))[0]!;
  const matches = families
    .filter((f) => f.reflex && f.lamp)
    .map((f) => ({ f, s: dot(probe.vec, f.centroid) }))
    .filter((m) => m.s >= m.f.t)
    .sort((a, b) => b.s - a.s);

  return { order, routes, ubiquitous, nb, edgesAll, edgesEps, communities, clusters, splitNotes, mergeNotes, afterSplit, afterMerge, families, probe, match: matches[0] ?? null, n };
}

// ------------------------------------------------------------------ the scene

const CAM: Record<"list" | "cloud" | "family", CamSpec> = {
  list: { target: [0, 0, 0], yaw: 0, pitch: 0.06, dist: 21 },
  cloud: { target: [0, -0.2, -0.4], yaw: 0.34, pitch: 0.24, dist: 20 },
  family: { target: [-3.6, 0.9, 1.6], yaw: 0.16, pitch: 0.18, dist: 15 },
};

/** Where each stand-in blob is drawn (illustrative: the real space has 256 dimensions). */
const ANCHOR: Record<Prompt["blob"], V3> = {
  T: [-4.8, 1.2, 1.8],
  X: [-2.6, -0.2, 2.8],
  L: [-0.4, -4.0, 3.2],
  J: [4.4, 2.0, -0.4],
  G: [4.2, -0.2, -1.0],
  H1: [-2.6, 3.4, -4.4],
  H2: [1.6, 3.0, -4.8],
  N1: [6.4, -3.8, 3.0],
  N2: [-7.0, -3.2, -2.2],
};

/** Hues for routes (categorical; lamp colours stay reserved for strength). */
const ROUTE_HUE: Record<string, number> = {
  "make test": 200,
  "git log --oneline": 250,
  "go list -m -u all": 165,
  "pnpm outdated": 335,
  "kubectl logs deploy/dispatch": 75,
};

const SHOWCASE = [
  "the dispatch tests fail on main since a3f9c21, fix them",
  "make test is red after W-4821 landed",
  "fix the failing test in src/router/holidays.test.ts",
  "lane pricing tests fail, see https://ci.example.dev/run/8812",
  "port lookup tests fail with ```AssertionError: expected OSL``` since yesterday",
  "bump typescript to 5.6 in web/map",
  "why is SLA_BREACH_ALERT firing",
];

const SHELL_R = 2.3;

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const add3 = (a: V3, b: V3, k = 1): V3 => [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k];
const mean3 = (ps: V3[]): V3 => {
  const s = ps.reduce<V3>((a, p) => add3(a, p), [0, 0, 0]);
  return [s[0] / ps.length, s[1] / ps.length, s[2] / ps.length];
};
const toward = (p: V3, c: V3, k: number): V3 => [c[0] + (p[0] - c[0]) * k, c[1] + (p[1] - c[1]) * k, c[2] + (p[2] - c[2]) * k];
const dir3 = (j: V3): V3 => {
  const n = Math.hypot(...j);
  return n > 1e-9 ? [j[0] / n, j[1] / n, j[2] / n] : [0.3, 0.9, 0.3];
};
const bold = (html: string) => html.replace(/\{(\w+)\}|\[code\]/g, (m) => `<b>${m}</b>`);

function build(stage: Stage): Built {
  const r = run();
  const { order, n } = r;
  const id = (k: number) => `p${k}`;
  order.forEach((_, k) => stage.dot(id(k)));
  stage.dot("probe", 0.22);

  // Positions.
  const cloud: V3[] = order.map((it) => add3(ANCHOR[it.blob], it.jitter, 1.05));
  const showIx = SHOWCASE.map((raw) => order.findIndex((it) => it.raw === raw));
  const listPos: V3[] = [];
  let g = 0;
  order.forEach((_, k) => {
    const s = showIx.indexOf(k);
    // The showcased prompts in a column (their chips run to the right), the rest in rows below.
    if (s >= 0) listPos[k] = [-8, 5 - s * 1.12, 0];
    else {
      listPos[k] = [-8 + (g % 11) * 1.3, -3.2 - Math.floor(g / 11) * 1.15, 0];
      g++;
    }
  });
  const byLabel = (labels: number[]) => {
    const m = new Map<number, number[]>();
    labels.forEach((l, k) => l >= 0 && (m.get(l) ?? m.set(l, []).get(l)!).push(k));
    return m;
  };
  const pulled = (labels: number[], k: number, from: V3[] = cloud): V3[] => {
    const groups = byLabel(labels);
    const centre = new Map([...groups].map(([l, ks]) => [l, mean3(ks.map((x) => from[x]!))]));
    return order.map((_, x) => (labels[x]! >= 0 ? toward(from[x]!, centre.get(labels[x]!)!, k) : from[x]!));
  };
  const commPos = pulled(r.clusters, 0.6);
  // After the split the two route groups drift apart; after the merge the two families close up.
  const jgLabel = r.clusters[order.findIndex((it) => it.blob === "J")];
  const splitIx = order.map((_, k) => k).filter((k) => r.clusters[k] === jgLabel);
  const hLabel = r.afterMerge[order.findIndex((it) => it.blob === "H1")];
  const mergeIx = order.map((_, k) => k).filter((k) => r.afterMerge[k] === hLabel);
  const subCentre = (blob: Prompt["blob"]) => mean3(order.flatMap((it, k) => (it.blob === blob ? [commPos[k]!] : [])));
  const apart: V3 = dir3(add3(subCentre("J"), subCentre("G"), -1));
  const mergedPull = pulled(r.afterMerge, 0.22, commPos);
  const finalPos: V3[] = order.map((it, k) => {
    if (splitIx.includes(k)) return add3(commPos[k]!, apart, it.blob === "J" ? 1.1 : -1.1);
    if (mergeIx.includes(k)) return mergedPull[k]!;
    return commPos[k]!;
  });

  // The test family, laid out by its real similarity: distance from its centre is (1 − cos),
  // scaled so that the shell is exactly its threshold. Directions keep near the screen's plane.
  const fam = r.families.find((f) => f.route === "make test" && f.reflex)!;
  const famIx = new Set(fam.members);
  const outsider = order.findIndex((it) => it.blob === "X");
  const famCentre = mean3(fam.members.map((k) => finalPos[k]!));
  const radial = (sim: number, j: V3): V3 => add3(famCentre, dir3([j[0], j[1], j[2] * 0.3]), (SHELL_R * (1 - sim)) / (1 - fam.t));
  const calPos: V3[] = order.map((it, k) =>
    k === outsider ? radial(fam.sims[k]!, [1, -0.3, 0]) : famIx.has(k) ? radial(fam.sims[k]!, it.jitter) : finalPos[k]!,
  );
  const probeSim = r.match?.s ?? 0;
  const probePos = radial(probeSim, r.probe.jitter);
  const probeFrom: V3 = [8.5, 6, 3];
  const probeTag = add3(famCentre, [-0.6, SHELL_R + 1.1, 0]);
  stage.link("tag", [[probeTag, "probe"]], "accent");

  // Counts every caption states.
  const placeholders = order.reduce((s, it) => s + it.placeholders, 0);
  const fences = order.filter((it) => it.raw.includes("```")).length;
  const edgesTotal = r.edgesAll.length + r.edgesEps.length;
  const commGroups = byLabel(r.clusters);
  const noise = r.clusters.filter((l) => l < 0).length;
  const ubi = r.ubiquitous[0]!;
  const split = r.splitNotes[0]!;
  const merged = r.mergeNotes[0]!;
  const kept = r.families.filter((f) => f.kept);
  const dropped = r.families.find((f) => !f.kept)!;
  const reflexes = r.families.filter((f) => f.reflex && f.lamp);
  const lower = fam.curve[fam.curve.length - 1]!;
  const contrast = fam.rate - fam.rateOut;
  const droppedIx = dropped.members;
  const f4 = (x: number) => x.toFixed(4).replace(/0+$/, "").replace(/\.$/, "");

  // Links.
  stage.link("near", r.edgesAll.map(([a, b]) => [id(a), id(b)]), "ink3");
  stage.link("eps", r.edgesEps.map(([a, b]) => [id(a), id(b)]), "accent");
  stage.link("intra", r.edgesEps.filter(([a, b]) => r.clusters[a]! >= 0 && r.clusters[a] === r.clusters[b]).map(([a, b]) => [id(a), id(b)]), "soft");
  stage.link("fam", r.edgesEps.filter(([a, b]) => r.afterMerge[a]! >= 0 && r.afterMerge[a] === r.afterMerge[b]).map(([a, b]) => [id(a), id(b)]), "soft");
  const ring = (axis: 0 | 1 | 2): [V3, V3][] => {
    const pts: V3[] = [];
    const N = 64;
    for (let i = 0; i <= N; i++) {
      const a = (i / N) * Math.PI * 2;
      const u = Math.cos(a) * SHELL_R;
      const v = Math.sin(a) * SHELL_R;
      pts.push(add3(famCentre, axis === 0 ? [u, v, 0] : axis === 1 ? [u, 0, v] : [0, u, v]));
    }
    return pts.slice(1).map((p, i) => [pts[i]!, p]);
  };
  stage.link("shell", [...ring(0), ...ring(1), ...ring(2)], "soft");

  // Labels.
  showIx.forEach((k, s) => {
    const it = order[k]!;
    stage.label(`show${s}`, `<span class="how-dim">${esc(it.raw)}</span><br>${bold(esc(it.norm))}`, id(k), { cls: "chip", dx: 14, dy: -17 });
  });
  const note = (name: string, html: string, at: string | V3, opts: { dx?: number; dy?: number; center?: boolean } = {}) =>
    stage.label(name, html, at, { cls: "note", ...opts });
  const jgCentre = mean3(splitIx.map((k) => commPos[k]!));
  const hCommCentre = mean3(mergeIx.map((k) => commPos[k]!));
  const hCentre = mean3(mergeIx.map((k) => finalPos[k]!));
  const lCentre = mean3(droppedIx.map((k) => finalPos[k]!));
  const gCentre = mean3(order.flatMap((it, k) => (it.blob === "G" ? [finalPos[k]!] : [])));
  const bottom: V3 = [0, -6.2, 0];
  note("embed", "256 dimensions, drawn here in 3D", bottom, { center: true });
  note("eps", `${r.edgesEps.length} links at cosine ≥ ${f2(EPS)} · ${r.edgesAll.length} weaker ones cut`, bottom, { center: true });
  note("noise", `noise · a community under ${MIN}`, id(order.findIndex((it) => it.blob === "N1")), { dy: 14, center: true });
  note("ubi", `${esc(ubi.cmd)} ran in ${ubi.traces} of ${n} traces · skipped`, bottom, { center: true });
  note("mixed", "one family · two routes", jgCentre, { dy: 74, center: true });
  note("same", "two families · one route", hCommCentre, { dy: -40, center: true });
  note("split", `split · leave-one-out ${f2(split.sep)} ≥ ${f2(SEP)}`, gCentre, { dy: 34, center: true });
  note("merge", `merge · centres ${f2(merged.cos)} alike ≥ ${f2(MERGE_TAU)}`, hCentre, { dy: 30, center: true });
  note("drop", `dominant route ${f2(dropped.share)} &lt; ${f2(BAR)} · not kept`, lCentre, { dy: -64, center: true });
  note("t", `threshold t = ${f4(fam.t)}`, add3(famCentre, [SHELL_R * 0.72, SHELL_R * 0.72, 0]), { dx: 10, dy: -14 });
  note("outside", `the fixtures question<br><span class="how-dim">ran no ${esc(fam.route)} · outside</span>`, id(outsider), { dx: 14, dy: -8 });
  stage.label("probe", `<span class="how-dim">${esc(NEW_PROMPT)}</span><br>${bold(esc(normalize(NEW_PROMPT).text))}`, probeTag, { cls: "chip", dy: -40, center: true });
  stage.label(
    "reflex",
    `<small>INTENT REFLEX · RUN</small><span class="how-lamp ${fam.lamp}">●</span> ${fam.lamp} · ${esc(fam.route)}<br><span class="how-dim">requests like this one start with it</span>`,
    add3(famCentre, [0, -SHELL_R, 0]),
    { cls: "chip big", dy: 22, center: true },
  );

  const routeColor = (k: number): DotLook["color"] => {
    const rt = r.routes[k];
    return rt ? { hue: ROUTE_HUE[rt] ?? 0 } : "ink3";
  };

  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    order.forEach((_, k) => {
      let pos = s === 0 ? listPos[k]! : s <= 2 ? cloud[k]! : s <= 4 ? commPos[k]! : s === 5 ? finalPos[k]! : calPos[k]!;
      let color: DotLook["color"] = s <= 2 ? "ink2" : s === 3 ? (r.clusters[k]! >= 0 ? "soft" : "ink3") : routeColor(k);
      let alpha = 1;
      let scale = 1;
      let glow = 0.18;
      if (s === 0 && showIx.includes(k)) {
        color = "soft";
        glow = 0.4;
      }
      if (s >= 3 && s <= 5 && r.clusters[k]! < 0) alpha = 0.4;
      if (s === 5 && (splitIx.includes(k) || mergeIx.includes(k))) {
        scale = 1.3;
        glow = 0.6;
      }
      if (s >= 6) {
        if (famIx.has(k)) {
          scale = 1.2;
          glow = fam.sims[k]! >= fam.t ? 0.6 : 0.2;
        } else {
          alpha = s === 6 && droppedIx.includes(k) ? 0.5 : 0.18;
          glow = 0;
        }
        if (k === outsider) {
          color = "ink2";
          alpha = s === 6 ? 1 : 0.35;
        }
      }
      dots[id(k)] = { pos, color, alpha, scale, glow };
    });
    if (s === 6) dots.probe = { pos: probeFrom, color: "accent", alpha: 0, glow: 0 };
    if (s === 7) dots.probe = { pos: probePos, color: "accent", scale: 1.3, glow: 1 };
    const labels: string[] = [];
    if (s === 0) labels.push(...SHOWCASE.map((_, i) => `show${i}`));
    if (s === 1) labels.push("embed");
    if (s === 2) labels.push("eps");
    if (s === 3) labels.push("noise");
    if (s === 4) labels.push("ubi", "mixed", "same");
    if (s === 5) labels.push("split", "merge");
    if (s === 6) labels.push("drop", "t", "outside");
    if (s === 7) labels.push("t", "probe", "reflex");
    return {
      dots,
      labels,
      cam: s === 0 ? CAM.list : s <= 5 ? CAM.cloud : CAM.family,
      links: {
        near: s === 2 ? 0.16 : 0,
        eps: s === 2 ? 0.55 : 0,
        intra: s === 3 || s === 4 ? 0.3 : 0,
        fam: s === 5 ? 0.3 : s >= 6 ? 0.1 : 0,
        shell: s >= 6 ? 0.5 : 0,
        tag: s === 7 ? 0.6 : 0,
      },
    };
  }

  // Figures.
  const axisFont = "10px 'Red Hat Mono', ui-monospace, monospace";
  const simHist: Step["figure"] = (gx, w, h, pal) => {
    const sims = r.nb.flatMap((row) => row.map(([, v]) => v));
    const bins = 24;
    const lo = Math.min(0, ...sims);
    const counts = new Array<number>(bins).fill(0);
    for (const v of sims) counts[Math.min(bins - 1, Math.floor(((v - lo) / (1 - lo)) * bins))]!++;
    const padB = 16;
    const padT = 14;
    const X = (x: number) => 4 + ((x - lo) / (1 - lo)) * (w - 8);
    const maxC = Math.max(...counts);
    const bw = (w - 8) / bins;
    counts.forEach((c, i) => {
      const x0 = lo + ((1 - lo) * i) / bins;
      const kept = x0 >= EPS - 1e-9;
      gx.fillStyle = pal.css(kept ? "accent" : "ink3");
      gx.globalAlpha = kept ? 0.85 : 0.45;
      const bh = (c / maxC) * (h - padB - padT);
      gx.fillRect(X(x0) + 1, h - padB - bh, Math.max(1, bw - 2), bh);
    });
    gx.globalAlpha = 1;
    gx.font = axisFont;
    gx.fillStyle = pal.css("ink3");
    gx.textBaseline = "top";
    for (const x of [lo, EPS, 1]) {
      gx.textAlign = x === lo ? "left" : x === 1 ? "right" : "center";
      gx.fillText(x === EPS ? `eps ${f2(EPS)}` : f2(x), X(x), h - padB + 3);
    }
    gx.setLineDash([3, 3]);
    gx.strokeStyle = pal.css("soft");
    gx.beginPath();
    gx.moveTo(X(EPS), padT - 2);
    gx.lineTo(X(EPS), h - padB);
    gx.stroke();
    gx.setLineDash([]);
    gx.textAlign = "left";
    gx.fillText(`cosine to each prompt's ${K} nearest neighbours`, 4, 0);
  };
  const calFigure: Step["figure"] = (gx, w, h, pal) => {
    const padB = 16;
    const pts = fam.curve;
    const tMin = Math.min(...pts.map((p) => p.t)) - 0.01;
    const tMax = Math.max(...pts.map((p) => p.t)) + 0.01;
    const X = (t: number) => 6 + ((tMax - t) / (tMax - tMin)) * (w - 12); // strictest t on the left
    const Y = (v: number) => h - padB - v * (h - padB - 14);
    gx.strokeStyle = pal.css("ink3");
    gx.globalAlpha = 0.4;
    gx.beginPath();
    gx.moveTo(4, h - padB + 0.5);
    gx.lineTo(w - 4, h - padB + 0.5);
    gx.stroke();
    gx.globalAlpha = 1;
    gx.setLineDash([3, 3]);
    gx.strokeStyle = pal.css("fire");
    gx.beginPath();
    gx.moveTo(4, Y(FIRE_LB));
    gx.lineTo(w - 4, Y(FIRE_LB));
    gx.stroke();
    gx.setLineDash([]);
    gx.font = axisFont;
    gx.textBaseline = "bottom";
    gx.textAlign = "left";
    gx.fillStyle = pal.css("fire");
    gx.fillText("fire bar", 6, Y(FIRE_LB) - 1);
    for (const p of pts) {
      const chosen = p.t === fam.t;
      const c = pal.css(chosen ? "accent" : "ink3");
      gx.strokeStyle = c;
      gx.fillStyle = c;
      gx.lineWidth = chosen ? 2 : 1;
      gx.beginPath();
      gx.moveTo(X(p.t), Y(0));
      gx.lineTo(X(p.t), Y(p.lb));
      gx.stroke();
      gx.beginPath();
      gx.arc(X(p.t), Y(p.lb), chosen ? 3.5 : 2.5, 0, Math.PI * 2);
      gx.fill();
    }
    gx.lineWidth = 1;
    gx.textBaseline = "top";
    gx.fillStyle = pal.css("ink3");
    gx.textAlign = "left";
    gx.fillText(`t ${f2(tMax)}`, 4, h - padB + 3);
    gx.textAlign = "center";
    gx.fillText("Wilson lower bound at each candidate t", w / 2, h - padB + 3);
    gx.textAlign = "right";
    gx.fillText(f2(tMin), w - 4, h - padB + 3);
    gx.fillStyle = pal.css("accent");
    gx.textAlign = "right";
    gx.textBaseline = "top";
    gx.fillText(`best: ${fam.support}/${fam.opp} at t ${f4(fam.t)}, LB ${f2(fam.lb)}`, w - 4, 0);
  };

  const text: Omit<Step, "state">[] = [
    {
      title: "Prompts are normalised",
      body: `Each of the ${n} sample prompts is cut to its first ${HEAD_TOKENS} words, and details become placeholders: tickets, paths, URLs, SHAs, versions, environment variables and PR numbers. Fenced code becomes [code]. Prompts then compare by what they ask, not by what they name.`,
      chips: [{ text: `≤ ${HEAD_TOKENS} tokens · ≤ 16 KiB read` }, { text: `${placeholders} placeholders in ${n} prompts` }, { text: `${fences} fenced block → [code]` }],
    },
    {
      title: "Each prompt becomes a point",
      body: "A pinned static model embeds the normalised text: WordPiece tokens, the mean of their rows, scaled to unit length. Requests that mean the same thing land near each other. The model runs in-process; without it there are no intent reflexes, and nothing fails.",
      chips: [{ text: "potion-base-8M · 31 MB · SHA-256 pinned" }, { text: "≤ 512 WordPiece tokens" }, { text: "mean · L2 · 256-d" }],
    },
    {
      title: "Near neighbours link",
      body: `Each prompt links to its ${K} most similar prompts, found exactly, with ties broken by index so every run builds the same graph. Links below cosine ${f2(EPS)} are cut: ${r.edgesEps.length} of the ${edgesTotal} remain.`,
      chips: [{ text: `exact kNN, k = ${K}` }, { text: `eps ${f2(EPS)}` }, { text: `${r.edgesEps.length} of ${edgesTotal} links kept` }],
      figure: simHist,
    },
    {
      title: "Communities become families",
      body: `Louvain groups densely linked prompts. It visits them in a fixed order and moves one only on a strictly better gain, so the result never varies. Resolution ${RESOLUTION.toFixed(1)} keeps communities small and pure. ${commGroups.size} have at least ${MIN} prompts; the ${noise} prompts left in smaller ones are noise.`,
      chips: [{ text: `resolution ${RESOLUTION.toFixed(1)} · ≤ ${MAX_LEVELS} levels` }, { text: `min size max(min_support, 2) = ${MIN}` }, { text: `${commGroups.size} communities · ${noise} noise` }],
    },
    {
      title: "Colour by what the trace ran first",
      body: `Each prompt is labelled with its trace's route: the first check or lookup it ran before editing. ${ubi.cmd} ran in ${ubi.traces} of ${n} traces, more than ${UBIQUITOUS} of them, so it says nothing about intent and is skipped. Now the families show what the text alone could not.`,
      chips: [{ text: "first observe or verify step before an edit" }, { text: `ubiquitous: > ${UBIQUITOUS} of traces (5 or more)` }],
    },
    {
      title: "Behaviour splits and merges",
      body: `One family mixes two routes: ${split.groups.map((x) => `${x.size} started with ${x.route}`).join(" and ")}. Each group is large enough and the text tells them apart, so the family splits. Two history families both start with ${merged.route}, and their centres are ${f2(merged.cos)} alike, so they merge.`,
      chips: [
        { text: `split: groups ≥ ${MIN} and ≥ ${MIN_SHARE * 100}%` },
        { text: `leave-one-out ${f2(split.sep)} ≥ ${f2(SEP)}`, ok: split.sep >= SEP },
        { text: `merge: same route, cos ${f2(merged.cos)} ≥ ${f2(MERGE_TAU)}`, ok: merged.cos >= MERGE_TAU },
      ],
    },
    {
      title: "Keep pure families, calibrate each",
      body: `A family is kept when at least ${BAR} of its traces took its dominant route. The incident family is at ${f2(dropped.share)}, so ${kept.length} of ${r.families.length} are kept. Each kept centre is stored as integers, and its threshold is chosen on this repository's own prompts: the t whose prompts ran ${fam.route} with the best Wilson lower bound, ${fam.support} of ${fam.opp} here. Taking in the fixtures question too would give ${lower.sup} of ${lower.opp}, a lower bound of ${f2(lower.lb)}.`,
      chips: [
        { text: `bar ${BAR} · ${kept.length} kept` },
        { text: `t = ${f4(fam.t)} · within [${T_MIN}, ${T_MAX}]` },
        { text: `contrast ${f2(contrast)} ≥ ${f2(CONTRAST)}`, ok: contrast >= CONTRAST },
        { text: "centroid × 10,000 → i16" },
      ],
      figure: calFigure,
    },
    {
      title: "A new prompt arrives",
      body: `At the hook, a new prompt is normalised, embedded and compared with the centre of each of the ${reflexes.length} intent reflexes. The most similar one whose threshold it clears matches: cosine ${f2(probeSim)} against t ${f4(fam.t)}. Its lower bound, ${f2(fam.lb)}, is under the ${f2(FIRE_LB)} fire bar, so the reflex primes rather than fires.`,
      chips: [{ text: `cos ${f2(probeSim)} ≥ t ${f4(fam.t)}`, ok: probeSim >= fam.t }, { text: `${fam.lamp} · lower bound ${f2(fam.lb)}` }, { text: "p95 budget 25 ms with intent" }],
    },
  ];

  return {
    source: `${n} sample prompts · positions illustrative`,
    legend: [...Object.entries(ROUTE_HUE).map(([label, hue]) => ({ label, role: "hue" as const, hue })), { label: "no route", role: "ink3" }],
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

export const intent: Chapter = {
  id: "intent",
  title: "Group prompts by intent",
  status: "Built · optional model",
  kind: "built",
  summary:
    "Prompts are normalised, embedded with a pinned 31 MB model and clustered. Families split or merge by what their traces ran first, and each kept family gets a threshold calibrated on this repository's prompts.",
  source: "crates/adage-learn/src/intent.rs · crates/adage-intent",
  usesRepo: false,
  build,
};
