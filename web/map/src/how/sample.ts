// The sample traces the chapters replay when they are not using this repository's data: twelve
// agent traces in a registry-driven service. They are illustrative; every number a chapter shows
// about them is computed here, never typed into the words.

export type Code = "P" | "D" | "R" | "E" | "S" | "G" | "C" | "F" | "W" | "h";

export interface Proto {
  label: string;
  /** What the event is, before it has a prototype. */
  kind: "prompt" | "observe" | "edit" | "regen" | "verify" | "fail";
  /** Its place in embedding space (the cluster centre). */
  c: [number, number, number];
  /** A hue for its prototype colour. */
  hue: number;
  single?: boolean;
}

export const PROTOS: Record<Code, Proto> = {
  P: { label: "prompt · change the registry", kind: "prompt", c: [-6.2, 2.6, 1.2], hue: 262 },
  D: { label: "prompt · change dispatch", kind: "prompt", c: [-5.6, -2.2, 3.4], hue: 300 },
  R: { label: "read src/**", kind: "observe", c: [-2.2, 4.0, -2.6], hue: 205 },
  E: { label: "edit registry/*.csv", kind: "edit", c: [1.6, 3.0, 3.2], hue: 248 },
  S: { label: "edit src/*.ts", kind: "edit", c: [1.2, -3.2, 4.2], hue: 320 },
  G: { label: "make gen", kind: "regen", c: [5.4, 1.4, -1.8], hue: 185 },
  C: { label: "make check", kind: "verify", c: [6.6, -2.4, 1.2], hue: 225 },
  F: { label: "npm test ✗", kind: "fail", c: [2.6, -4.2, -3.8], hue: 350 },
  W: { label: "git status", kind: "observe", c: [-3.0, -3.4, -2.6], hue: 160 },
  h: { label: "curl localhost:8080/health", kind: "observe", c: [-0.6, 0.2, -6.2], hue: 40, single: true },
};

export type Event = [Code, string];

export const TRACES: Event[][] = [
  [["P", "add carrier HX-7"], ["R", "read src/registry/load.ts"], ["E", "edit registry/carriers.csv"], ["G", "make gen"], ["C", "make check"]],
  [["P", "fix the Oslo port code"], ["R", "read src/registry/ports.ts"], ["E", "edit registry/ports.csv"], ["G", "make gen -B"], ["C", "make check -j4"]],
  [["P", "lane rate wrong NYC→BOS"], ["R", "read src/registry/schema.ts"], ["E", "edit registry/lanes.csv"], ["W", "git status"], ["G", "make gen"], ["C", "make check"]],
  [["P", "add port Gdańsk"], ["E", "edit registry/ports.csv"], ["G", "make gen"], ["F", "npm test"], ["C", "make check"]],
  [["P", "rename carrier Northwind"], ["R", "read src/registry/load.ts"], ["E", "edit registry/carriers.csv"], ["G", "make gen"], ["C", "make check"]],
  [["P", "update lane LA→SF"], ["E", "edit registry/lanes.csv"], ["G", "make gen"], ["W", "git status"]],
  [["P", "remove port Tacoma"], ["R", "read src/registry/ports.ts"], ["E", "edit registry/ports.csv"], ["G", "make gen"], ["C", "make check -j4"]],
  [["D", "dispatch picks the slow lane"], ["R", "read src/dispatch.ts"], ["S", "edit src/dispatch.ts"], ["C", "make check"]],
  [["D", "retry dispatch on 503"], ["S", "edit src/dispatch.ts"], ["F", "npm test"], ["C", "make check"]],
  [["D", "router ignores holidays"], ["R", "read src/router.ts"], ["W", "git status"], ["S", "edit src/router.ts"], ["C", "make check"]],
  [["D", "log dispatch latency"], ["S", "edit src/router.ts"], ["h", "curl localhost:8080/health"], ["C", "make check"]],
  [["P", "fix carrier contact"], ["R", "read src/registry/load.ts"], ["E", "edit registry/carriers.csv"], ["C", "make check"]],
];

/** The index of the first event with `code` at or after `from`, or -1. */
export function at(tr: Event[], code: Code, from = 0): number {
  return tr.findIndex((e, j) => j >= from && e[0] === code);
}

/** Row layout shared by chapters that show traces as lanes. */
export const laneZ = (i: number) => (i - (TRACES.length - 1) / 2) * 1.25;
export const laneX = (j: number) => -7.2 + j * 2.5;

/** A seeded generator, so every load draws the same scene. */
export function seeded(seed = 7): () => number {
  let s = seed;
  return () => ((s = (s * 16807) % 2147483647) / 2147483647) * 2 - 1;
}

/** Beta(a, b) on a grid: density (normalised to sum 1), and its 5% quantile. */
export function betaGrid(a: number, b: number, n = 600): { xs: number[]; ps: number[]; q05: number; mean: number } {
  const xs: number[] = [];
  const ps: number[] = [];
  let sum = 0;
  for (let i = 1; i < n; i++) {
    const x = i / n;
    const p = Math.exp((a - 1) * Math.log(x) + (b - 1) * Math.log(1 - x));
    xs.push(x);
    ps.push(p);
    sum += p;
  }
  let c = 0;
  let q05 = 0;
  for (let i = 0; i < ps.length; i++) {
    ps[i]! /= sum;
    c += ps[i]!;
    if (!q05 && c >= 0.05) q05 = xs[i]!;
  }
  return { xs, ps, q05, mean: a / (a + b) };
}

/** Wilson score lower bound at 95%, as the learner computes it (adage-tracks `wilson_lower`). */
export function wilson(k: number, n: number, z = 1.959964): number {
  if (!n) return 0;
  const p = k / n;
  const d = 1 + (z * z) / n;
  const c = p + (z * z) / (2 * n);
  const r = z * Math.sqrt((p * (1 - p)) / n + (z * z) / (4 * n * n));
  return (c - r) / d;
}

export const f2 = (x: number) => x.toFixed(2);
