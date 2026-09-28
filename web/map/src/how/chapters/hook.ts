// Chapter: decide at the hook (crates/adage-hook, crates/adage-runtime). Every hook event of the
// sample traces meets the no-match fast path; the ones that pass are sorted into treated and
// control by a hash of the trace id; `decide()` answers each by its kind; the caps and the inhibit
// lock prune the candidates; a checked value fills a fixed template; and the next three tool
// batches judge the directive followed or ignored. In "This repository" the lanes are the map's
// own recorded hook decisions, with their real arms.

import type { Graph, TraceEvent } from "../../api/types";
import { shortSession, strength as strengthOf } from "../../lib/words";
import type { CamSpec, DotLook, Palette, Role, SceneState, Stage, V3 } from "../engine";
import { TRACES, laneZ } from "../sample";
import type { Built, Chapter, Step } from "../types";

// ------------------------------------------------------------------ constants from the code

// crates/adage-tracks/src/params.rs (the runtime defaults `decide()` runs with).
const MAX_PER_SESSION = 8;
const MAX_PER_DECISION = 3;
const MAX_STOP_REQUESTS = 2;
const FOLLOW_WINDOW = 3;
// The control share when neither ADAGE_HOLDOUT nor `runtime.holdout` sets one.
const CONTROL_SHARE = 0.1;
// crates/adage-runtime/src/lib.rs `plain_token`.
const PLAIN_MAX = 200;

// Design §13.2 budget and the measured hook, end to end (Architecture §9, S7 addendum:
// `hook_latency.rs`, 500 runs, p95).
const LATENCY: { path: string; p50: number; p95: number; measured: number }[] = [
  { path: "no match", p50: 4, p95: 10, measured: 1.76 },
  { path: "structured", p50: 6, p95: 15, measured: 3.18 },
  { path: "intent", p50: 10, p95: 25, measured: 6.39 },
];

// ------------------------------------------------------------------ arm assignment

/** SHA-256 of a UTF-8 string (FIPS 180-4), synchronous so the chapter builds in one pass. */
export function sha256(text: string): Uint8Array {
  const K = new Uint32Array([
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ]);
  const msg = new TextEncoder().encode(text);
  const len = Math.ceil((msg.length + 9) / 64) * 64;
  const buf = new Uint8Array(len);
  buf.set(msg);
  buf[msg.length] = 0x80;
  const bits = msg.length * 8;
  const dv = new DataView(buf.buffer);
  dv.setUint32(len - 8, Math.floor(bits / 2 ** 32));
  dv.setUint32(len - 4, bits >>> 0);
  const H = new Uint32Array([0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]);
  const W = new Uint32Array(64);
  const rot = (x: number, n: number) => (x >>> n) | (x << (32 - n));
  for (let o = 0; o < len; o += 64) {
    for (let t = 0; t < 16; t++) W[t] = dv.getUint32(o + t * 4);
    for (let t = 16; t < 64; t++) {
      const a = W[t - 15]!;
      const b = W[t - 2]!;
      const s0 = rot(a, 7) ^ rot(a, 18) ^ (a >>> 3);
      const s1 = rot(b, 17) ^ rot(b, 19) ^ (b >>> 10);
      W[t] = (W[t - 16]! + s0 + W[t - 7]! + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, h] = H as unknown as number[];
    for (let t = 0; t < 64; t++) {
      const S1 = rot(e!, 6) ^ rot(e!, 11) ^ rot(e!, 25);
      const ch = (e! & f!) ^ (~e! & g!);
      const t1 = (h! + S1 + ch + K[t]! + W[t]!) >>> 0;
      const S0 = rot(a!, 2) ^ rot(a!, 13) ^ rot(a!, 22);
      const mj = (a! & b!) ^ (a! & c!) ^ (b! & c!);
      const t2 = (S0 + mj) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d! + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }
    [a, b, c, d, e, f, g, h].forEach((v, i) => (H[i] = (H[i]! + v!) >>> 0));
  }
  const out = new Uint8Array(32);
  const ov = new DataView(out.buffer);
  H.forEach((v, i) => ov.setUint32(i * 4, v));
  return out;
}

/** `assign_arm`'s draw: the first 4 bytes of sha256("adage-arm:" + id), little-endian, over u32::MAX. */
export function armDraw(session: string): number {
  const h = sha256(`adage-arm:${session}`);
  return ((h[0]! | (h[1]! << 8) | (h[2]! << 16) | (h[3]! << 24)) >>> 0) / 0xffffffff;
}

/** `plain_token`: whatever a contract says, a bound value is a plain token. */
export function plainToken(v: string): boolean {
  return v.length > 0 && v.length <= PLAIN_MAX && !v.startsWith("-") && /^[A-Za-z0-9_./:@=+,-]+$/.test(v);
}

// ------------------------------------------------------------------ the sample index

type Kind = "start" | "prompt" | "batch" | "failure" | "stop";
type Strength = "fire" | "prime" | "inhibit";

interface Outcome {
  route: string;
  strength: Strength;
  /** What it is, for a label. */
  what: string;
  /** Dropped by the inhibit lock. */
  struck?: boolean;
  /** A stop request rather than a directive. */
  stop?: boolean;
}

interface Ev {
  id: string;
  kind: Kind;
  text: string;
  pass: boolean;
  out: Outcome[];
  judged: string[];
}

interface Sess {
  id: string;
  name: string;
  x: number;
  control: boolean;
  events: Ev[];
}

// Three reflexes of the sample repository, as `decide()` would read them (illustrative).
const FINISH = { id: "finish-line-registry", glob: "registry/*.csv", run: "make gen" };
const DEAD = { id: "dead-end-npm-test", avoid: "npm test", word: "npm", instead: "make check" };
const NODE = { id: "npm-test-after-edit", glob: "src/*.ts", run: "npm test" };
const EDIT_GLOBS = [FINISH.glob, NODE.glob];

const globRe = (g: string) => new RegExp(`^${g.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*")}$`);
/** `Triggers::call_might_match`: a trigger word, or every literal piece of an edit glob. */
const mightMatch = (hay: string) => hay.includes(DEAD.word) || EDIT_GLOBS.some((g) => g.split("*").every((f) => hay.includes(f)));
/** `pattern::meets` for these keys: the command, or it with options added. */
const meets = (directed: string, observed: string) => observed === directed || observed.startsWith(`${directed} -`);

/** Replay the sample traces through the fast path and `decide()` with the sample index. */
function simulate(): Sess[] {
  return TRACES.map((tr, i) => {
    const name = `T${i + 1}`;
    const x = armDraw(name);
    const st = { prompts: 0, emitted: new Set<string>(), obligations: [] as { met: boolean; asked: boolean }[], pending: 0, stops: 0 };
    const events: Ev[] = [];
    const push = (kind: Kind, text: string, pass: boolean, out: Outcome[] = [], judged: string[] = []) =>
      events.push({ id: `e${i}.${events.length}`, kind, text, pass, out, judged });
    push("start", "session start", false);
    for (const [code, exact] of tr) {
      if (code === "P" || code === "D") {
        // The first prompt is always decided: dead ends are announced there, before anything else.
        const pass = st.prompts === 0;
        const out: Outcome[] = [];
        if (pass) {
          st.prompts++;
          st.emitted.add(DEAD.id);
          out.push({ route: DEAD.id, strength: "inhibit", what: `${DEAD.avoid} fails · use ${DEAD.instead}` });
        }
        push("prompt", exact, pass, out);
        continue;
      }
      if (code === "F") {
        push("failure", `${exact} ✗`, mightMatch(exact) || st.pending > 0);
        continue;
      }
      const pass = mightMatch(exact) || st.pending > 0 || st.obligations.length > 0;
      const out: Outcome[] = [];
      const judged: string[] = [];
      if (pass) {
        for (const o of st.obligations.filter((o) => !o.met && meets(FINISH.run, exact))) {
          o.met = true;
          judged.push("met");
        }
        const path = exact.startsWith("edit ") ? exact.slice(5) : null;
        if (path && globRe(FINISH.glob).test(path)) {
          const o = st.obligations[0];
          if (o) {
            if (o.met) Object.assign(o, { met: false, asked: false });
          } else if (!st.emitted.has(FINISH.id)) {
            st.emitted.add(FINISH.id);
            st.obligations.push({ met: false, asked: false });
            out.push({ route: FINISH.id, strength: "fire", what: `finish line · ${FINISH.run}` });
          }
        }
        if (path && globRe(NODE.glob).test(path)) {
          // A candidate, then `approach_lock`: the dead end announced at the first prompt avoids
          // `npm test`, so the directive is dropped with its pending judgement.
          const struck = st.emitted.has(DEAD.id);
          out.push({ route: NODE.id, strength: "prime", what: `run ${NODE.run}`, struck });
          if (!struck) st.emitted.add(NODE.id);
        }
      }
      push("batch", exact, pass, out, judged);
    }
    const open = st.obligations.find((o) => !o.met && !o.asked);
    const stopOut: Outcome[] = [];
    if (open && st.stops < MAX_STOP_REQUESTS) {
      open.asked = true;
      st.stops++;
      stopOut.push({ route: FINISH.id, strength: "fire", what: `stop request · ${FINISH.run} not run`, stop: true });
    }
    push("stop", "stop", Boolean(open), stopOut);
    return { id: name, name, x, control: x < CONTROL_SHARE, events };
  });
}

// ------------------------------------------------------------------ this repository

function eventKind(name: string): Kind {
  if (/prompt/i.test(name)) return "prompt";
  if (/fail/i.test(name)) return "failure";
  if (/stop/i.test(name)) return "stop";
  if (/start|end/i.test(name)) return "start";
  return "batch";
}

/** The most recent traces' recorded hook decisions, as lanes. Every recorded event passed the fast path. */
function fromRepo(g: Graph): Sess[] {
  const strengths = new Map(g.routes.map((r) => [r.id, (strengthOf(r.aspect) ?? "prime") as Strength]));
  const by = new Map<string, TraceEvent[]>();
  for (const e of g.events) by.set(e.session, [...(by.get(e.session) ?? []), e]);
  const latest = (evs: TraceEvent[]) => Math.max(...evs.map((e) => e.ts));
  return [...by.entries()]
    .sort((a, b) => latest(b[1]) - latest(a[1]))
    .slice(0, 12)
    .map(([id, evs], i) => {
      const x = armDraw(id);
      const control = evs.some((e) => e.arm === "holdout");
      const shown = [...evs].sort((a, b) => a.ts - b.ts).slice(-7);
      return {
        id,
        name: shortSession(id),
        x,
        control,
        events: shown.map((e, j) => ({
          id: `e${i}.${j}`,
          kind: eventKind(e.event),
          text: e.event,
          pass: true,
          out: [
            ...(control ? e.would : e.directed).map((r) => ({ route: r, strength: strengths.get(r) ?? "prime", what: r })),
            ...(e.continue ? [{ route: "stop", strength: "fire" as const, what: "stop request", stop: true }] : []),
          ],
          judged: e.judged.map(([k]) => (k === "derailed" ? "ignored" : k === "obligation_met" ? "met" : k.replace(/_/g, " "))),
        })),
      };
    });
}

// ------------------------------------------------------------------ the scene

const KIND_HUE: Record<Kind, number> = { start: 40, prompt: 262, batch: 205, failure: 300, stop: 160 };
const GATE_X = -1.4;
const QX = (j: number) => -9 + j * 0.98;
const PX = (k: number) => 0.4 + k * 1.2;
const NL = (x: number) => -6 + 12 * x;
const NL_Y = 4.4;

const CAM: Record<"queue" | "coin" | "lanes" | "lock" | "bind" | "follow", CamSpec> = {
  queue: { target: [-1.4, -0.6, 0.4], yaw: -0.42, pitch: 0.72, dist: 24 },
  coin: { target: [0, NL_Y - 0.6, 0], yaw: 0, pitch: 0.18, dist: 17 },
  lanes: { target: [4.2, 0.4, 0.2], yaw: -0.1, pitch: 0.95, dist: 20 },
  lock: { target: [3.4, 0, 0], yaw: -0.12, pitch: 0.62, dist: 15 },
  bind: { target: [-1.2, 0.2, 0], yaw: 0, pitch: 0.1, dist: 17 },
  follow: { target: [-0.6, 0, 0], yaw: 0.08, pitch: 0.16, dist: 17 },
};

// The example: a prompt reflex in acme-billing (the map's demo fixtures), bound and followed.
const EX = {
  prompt: "Pick up W-1042…",
  contract: "^W-\\d{4}$",
  command: "python3 scripts/wi-lookup {id}",
  key: "exec python3 scripts/wi-lookup {id}",
  // Values a binder could be offered, each checked for real below.
  values: ["W-1042", "W-10423", "--help", "W-1042;rm", "$(curl x)"],
  batches: ["grep -rn W-1042 src", "python3 scripts/wi-lookup W-1042", ""],
};

function build(stage: Stage, ctx: { graph: Graph | null; repo: boolean }): Built {
  const repo = ctx.repo && ctx.graph ? fromRepo(ctx.graph) : [];
  const useRepo = repo.length > 0;
  const S = useRepo ? repo : simulate();
  const all = S.flatMap((s) => s.events);
  const passed = all.filter((e) => e.pass);
  const skipped = all.length - passed.length;
  const skippedBy = (k: Kind) => all.filter((e) => !e.pass && e.kind === k).length;
  const treated = S.filter((s) => !s.control);
  const control = S.filter((s) => s.control);
  const outs = S.flatMap((s) => s.events.flatMap((e) => e.out.map((o) => ({ s, e, o }))));
  const live = outs.filter((x) => !x.o.struck);
  const struck = outs.filter((x) => x.o.struck);
  const delivered = live.filter((x) => !x.s.control && !x.o.stop).length;
  const would = live.filter((x) => x.s.control && !x.o.stop).length;
  const stops = live.filter((x) => x.o.stop);
  const perDecision = Math.max(0, ...all.map((e) => e.out.filter((o) => !o.struck && !o.stop).length));
  const perSession = Math.max(0, ...S.map((s) => s.events.reduce((n, e) => n + e.out.filter((o) => !o.struck && !o.stop).length, 0)));
  const judged = (k: string) => all.flatMap((e) => e.judged).filter((j) => j === k).length;
  const met = judged("met");
  const followed = judged("followed");
  const ignored = judged("ignored");
  const agree = S.filter((s) => (s.x < CONTROL_SHARE) === s.control).length;

  // Lanes: in queue order first, then treated before control once the arms are known.
  const order = [...treated, ...control];
  const zQueue = (i: number) => (S.length === TRACES.length ? laneZ(i) : (i - (S.length - 1) / 2) * 1.25);
  const zArm = (s: Sess) => {
    const r = order.indexOf(s);
    return -6.4 + r * 1.05 + (s.control ? 0.9 : 0);
  };

  // Dots: every event, its outcomes, and each trace's coin on the number line.
  for (const s of S) {
    stage.dot(`coin${s.id}`, 0.2);
    for (const e of s.events) {
      stage.dot(e.id, 0.15);
      e.out.forEach((_, k) => stage.dot(`${e.id}.o${k}`, 0.2));
    }
  }
  const pos = new Map<string, { queue: V3; pass: V3; pile: V3; arm: V3 }>();
  S.forEach((s, i) => {
    let k = 0;
    let skip = 0;
    s.events.forEach((e, j) => {
      const zq = zQueue(i);
      const pk = e.pass ? k++ : k;
      pos.set(e.id, {
        queue: [QX(j), 0, zq],
        pass: [PX(pk), 0, zq],
        pile: [GATE_X - 0.9 - skip * 0.42, -2.7, zq],
        arm: [PX(pk), 0, zArm(s)],
      });
      if (!e.pass) skip++;
    });
  });

  // Links.
  const Z = Math.abs(zQueue(0)) + 0.8;
  stage.link(
    "gate",
    [
      [
        [GATE_X, -1.2, -Z],
        [GATE_X, -1.2, Z],
      ],
      [
        [GATE_X, 1.2, -Z],
        [GATE_X, 1.2, Z],
      ],
      [
        [GATE_X, -1.2, -Z],
        [GATE_X, 1.2, -Z],
      ],
      [
        [GATE_X, -1.2, Z],
        [GATE_X, 1.2, Z],
      ],
    ],
    "accent",
  );
  stage.link(
    "axis",
    [
      [
        [NL(0), NL_Y, 0],
        [NL(1), NL_Y, 0],
      ],
      ...[0, 0.5, 1].map(
        (t) =>
          [
            [NL(t), NL_Y - 0.18, 0],
            [NL(t), NL_Y + 0.18, 0],
          ] as [V3, V3],
      ),
    ],
    "ink3",
  );
  stage.link(
    "share",
    [
      [
        [NL(CONTROL_SHARE), NL_Y - 1.3, 0],
        [NL(CONTROL_SHARE), NL_Y + 1.4, 0],
      ],
    ],
    "accent",
  );
  const stems: [string, string][] = [];
  const struckStems: [string, string][] = [];
  for (const e of all) e.out.forEach((o, k) => (o.struck ? struckStems : stems).push([e.id, `${e.id}.o${k}`]));
  stage.link("stems", stems, "ink3");
  stage.link("struckStems", struckStems, "ink3");
  const metLinks: [string, string][] = [];
  for (const s of S) {
    const fin = s.events.find((e) => e.out.some((o) => o.route === FINISH.id && !o.stop));
    const m = s.events.find((e) => e.judged.includes("met"));
    if (fin && m) metLinks.push([`${fin.id}.o${fin.out.findIndex((o) => o.route === FINISH.id)}`, m.id]);
  }
  stage.link("met", metLinks, "accent");

  // Labels.
  const nameDx = (n: string) => -(n.length * 6.6 + 14);
  S.forEach((s, i) => stage.label(`lane${i}`, s.name, s.events[0]!.id, { dx: nameDx(s.name), dy: -7 }));
  // In the arms view, each lane is named at its first decided event; control lanes say so.
  S.forEach((s, i) => {
    const first = s.events.find((e) => e.pass);
    if (first) stage.label(`arm${i}`, s.control ? `${s.name} <span class="how-dim">control</span>` : s.name, first.id, { cls: s.control ? "note" : "", dx: nameDx(s.name) - (s.control ? 54 : 0), dy: -7 });
  });
  stage.label("gate", `<small>FAST PATH</small>could this event change anything?`, [GATE_X, 1.2, -Z], { cls: "chip", dy: -44, center: true });
  stage.label("pile", `${skipped} skipped · no lock, no spool`, [GATE_X - 1.6, -2.7, zQueue(S.length - 1)], { cls: "note", dy: 16, center: true });
  stage.label("passed", `${passed.length} decided`, [PX(3), 0.6, zQueue(0)], { cls: "note", dy: -26, center: true });
  stage.label("ax0", "0", [NL(0), NL_Y, 0], { dx: -12, dy: -7 });
  stage.label("ax1", "1", [NL(1), NL_Y, 0], { dx: 8, dy: -7 });
  stage.label("share", `control if x &lt; ${CONTROL_SHARE}`, [NL(CONTROL_SHARE), NL_Y + 1.4, 0], { cls: "chip", dy: -30, center: true });
  stage.label("treatedSide", `treated · ${treated.length}`, [NL(0.62), NL_Y - 2.3, 0], { cls: "note", center: true });
  // Coin labels: control ones below the line with their x, treated ones above. Each takes the
  // first row where it clears its left neighbour; one that fits no row is left unlabelled.
  const coinLabels: string[] = [];
  const rows = { above: [-26, -42, -58].map((dy) => ({ dy, right: -1 })), below: [14, 46].map((dy) => ({ dy, right: -1 })) };
  [...S]
    .sort((a, b) => a.x - b.x)
    .forEach((s) => {
      const i = S.indexOf(s);
      const html = s.control ? `${s.name}<br><span class="how-dim">x ${s.x.toFixed(3)}</span>` : s.name;
      const w = (Math.max(s.name.length, 7) * 6.6 + 10) / 560; // in units of x, at about 560 px for 0 to 1
      const row = (s.control ? rows.below : rows.above).find((r) => s.x - w / 2 > r.right);
      if (!row) return;
      row.right = s.x + w / 2;
      stage.label(`coin${i}`, html, `coin${s.id}`, { dy: row.dy, center: true });
      coinLabels.push(`coin${i}`);
    });
  // One label per kind of outcome, on its first occurrence in a treated trace.
  const firstOut = (f: (o: Outcome) => boolean, ctl = false) => live.find((x) => f(x.o) && x.s.control === ctl);
  // The inhibit is labelled on the last treated trace (below the lanes), the finish line on the first.
  const firstInhibit = [...live].reverse().find((x) => x.o.strength === "inhibit" && !x.s.control);
  const firstFinish = firstOut((o) => o.route === FINISH.id && !o.stop);
  const firstStop = stops[0];
  const firstWould = [...live].reverse().find((x) => x.s.control);
  const outId = (x: { e: Ev; o: Outcome }) => `${x.e.id}.o${x.e.out.indexOf(x.o)}`;
  if (firstInhibit) stage.label("lInhibit", `<span class="how-lamp inhibit">●</span> ${firstInhibit.o.what}`, outId(firstInhibit), { cls: "chip", dx: 70, dy: 14, center: true });
  if (firstFinish) stage.label("lFinish", `<span class="how-lamp fire">●</span> ${firstFinish.o.what}`, outId(firstFinish), { cls: "chip", dy: -30, center: true });
  if (firstStop) stage.label("lStop", firstStop.o.what, outId(firstStop), { cls: "note", dy: -26, center: true });
  if (firstWould) stage.label("lWould", "would have · not delivered", outId(firstWould), { cls: "note", dx: 14, dy: -7 });
  const strike = struck.find((x) => !x.s.control) ?? struck[0];
  if (strike) {
    stage.label("lStruck", `<span class="how-lamp prime">●</span> ${strike.o.what} · struck`, outId(strike), { cls: "chip", dx: 14, dy: -9 });
    stage.label("lWhy", `the inhibit announced at the first prompt:<br>${DEAD.avoid} fails`, outId(strike), { cls: "note", dx: 14, dy: 14 });
  }
  // Each struck candidate, tied to the inhibit of its trace that struck it.
  const lockLinks: [string, string][] = struck.flatMap((x) => {
    const red = x.s.events.flatMap((e) => e.out.map((o, k) => ({ e, o, k }))).find((y) => y.o.strength === "inhibit");
    return red ? [[`${red.e.id}.o${red.k}`, outId(x)] as [string, string]] : [];
  });
  stage.link("lock", lockLinks, "inhibit");

  // The example scene: a prompt, the values offered to the binder, the template.
  stage.dot("xPrompt", 0.3);
  stage.dot("xTpl", 0.36);
  const re = new RegExp(EX.contract);
  const checks = EX.values.map((v) => ({ v, contract: re.test(v), plain: plainToken(v) }));
  checks.forEach((_, k) => stage.dot(`xv${k}`, 0.17));
  const VY = (k: number) => 2.4 - k * 1.2;
  const XG = -0.6;
  stage.link("xIn", checks.map((_, k) => ["xPrompt", `xv${k}`] as [string, string]), "ink3");
  stage.link("xOk", checks.flatMap((c, k) => (c.contract && c.plain ? [[`xv${k}`, "xTpl"] as [string, string]] : [])), "accent");
  stage.link(
    "xGate",
    [
      [
        [XG, -3.6, 0],
        [XG, 3.4, 0],
      ],
    ],
    "accent",
  );
  stage.label("xPrompt", `<small>PROMPT</small>${EX.prompt}`, "xPrompt", { cls: "chip big", dy: 22, center: true });
  stage.label("xGate", `<small>SLOT CONTRACT · PLAIN TOKEN</small>${EX.contract}`, [XG, 3.4, 0], { cls: "chip", dy: -40, center: true });
  const mark = (ok: boolean) => (ok ? "✓" : `<span class="how-dim">✗</span>`);
  checks.forEach((c, k) =>
    stage.label(`xv${k}`, `${c.v.replace(/</g, "&lt;")}&nbsp; ${mark(c.contract)}<span class="how-dim">contract</span> ${mark(c.plain)}<span class="how-dim">plain</span>`, `xv${k}`, {
      dx: 14,
      dy: -8,
    }),
  );
  const bound = checks.find((c) => c.contract && c.plain)?.v ?? "";
  const rendered = EX.command.replace("{id}", bound);
  stage.label(
    "xTpl",
    `<small>FIXED TEMPLATE · PROMPT LOOKUP</small>In this repository, work item details<br>are looked up with:<br>&nbsp;&nbsp;${rendered}`,
    "xTpl",
    { cls: "chip big", dy: 22, center: true },
  );

  // The follow window: the directive, then batches, counted down.
  stage.dot("fDir", 0.3);
  EX.batches.forEach((_, k) => stage.dot(`fB${k}`, 0.24));
  const FX = (k: number) => -5.6 + k * 3.6;
  stage.link("fLine", [["fDir", "fB0"], ["fB0", "fB1"], ["fB1", "fB2"]], "ink3");
  const hit = EX.batches.findIndex((b) => b && meets(EX.command.replace("{id}", bound), b));
  stage.label("fDir", `<small>DIRECTED · PENDING</small>${rendered}<br><span class="how-dim">${FOLLOW_WINDOW} batches left</span>`, "fDir", { cls: "chip big", dy: 24, center: true });
  EX.batches.forEach((b, k) => {
    const left = FOLLOW_WINDOW - k - 1;
    const html =
      k < hit
        ? `<small>BATCH ${k + 1}</small>${b}<br><span class="how-dim">no match · ${left} left</span>`
        : k === hit
          ? `<small>BATCH ${k + 1}</small>${b}<br>✓ followed`
          : `<small>BATCH ${k + 1}</small><span class="how-dim">not needed. Without a match<br>here, it counts as ignored</span>`;
    stage.label(`fB${k}`, html, `fB${k}`, { cls: k === hit ? "chip big" : "chip", dy: k % 2 ? 24 : -66, center: true });
  });
  stage.label("fKey", `meets ${EX.key}`, `fB${Math.max(0, hit)}`, { cls: "note", dy: 94, center: true });

  // ---------------------------------------------------------------- states

  const kindColor = (e: Ev): DotLook["color"] => ({ hue: KIND_HUE[e.kind] });
  const struckIn = new Set(struck.map((x) => x.s));
  function lanes(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    for (const sess of S) {
      for (const e of sess.events) {
        const p = pos.get(e.id)!;
        let look: DotLook = { pos: p.queue, color: kindColor(e), glow: 0.2 };
        if (s === 1) look = e.pass ? { pos: p.pass, color: kindColor(e), glow: 0.3 } : { pos: p.pile, color: "ink3", alpha: 0.35, glow: 0 };
        if (s === 2) {
          // The trace's events gather into its coin.
          look = e.pass ? { pos: [NL(sess.x), NL_Y, 0], color: kindColor(e), alpha: 0.15, scale: 0.5, glow: 0 } : { pos: p.pile, color: "ink3", alpha: 0 };
        }
        if (s >= 3) {
          if (!e.pass) continue;
          look = { pos: p.arm, color: kindColor(e), alpha: sess.control ? 0.4 : 0.9, glow: sess.control ? 0.05 : 0.2 };
          if (s === 4) look.alpha = (look.alpha ?? 1) * (struckIn.has(sess) ? 0.5 : 0.15);
        }
        dots[e.id] = look;
        if (s >= 3) {
          e.out.forEach((o, k) => {
            const [x, , z] = p.arm;
            const role: Role = o.stop ? "accent" : o.strength;
            if (o.struck) {
              if (s === 3) dots[`${e.id}.o${k}`] = { pos: [x, 1.2, z], color: role, alpha: sess.control ? 0.35 : 1, glow: 0.6 };
              if (s === 4) dots[`${e.id}.o${k}`] = { pos: [x, -1.3, z], color: "ink3", alpha: 0.6, scale: 0.8, glow: 0 };
              return;
            }
            const ghost = sess.control;
            // At the lock, only the inhibits that strike something stay bright.
            const lockDim = s === 4 && !(o.strength === "inhibit" && lockLinks.some(([a]) => a === `${e.id}.o${k}`));
            dots[`${e.id}.o${k}`] = {
              pos: [x, o.stop ? 1.6 : 1.2, z],
              color: role,
              alpha: s === 4 && !struckIn.has(sess) ? 0.1 : ghost ? (s === 7 ? 0.45 : 0.3) : lockDim ? 0.3 : 1,
              scale: ghost ? 0.85 : 1,
              glow: ghost ? (s === 7 ? 0.35 : 0.1) : lockDim ? 0 : s === 7 || s === 4 ? 0.9 : 0.6,
            };
          });
        }
      }
      if (s === 2) dots[`coin${sess.id}`] = { pos: [NL(sess.x), NL_Y, 0], color: sess.control ? "ink2" : "accent", scale: 1.1, glow: sess.control ? 0.25 : 0.6 };
    }
    const labels: string[] = [];
    if (s === 0) labels.push(...S.map((_, i) => `lane${i}`));
    if (s <= 1) labels.push("gate");
    if (s === 1) labels.push(...(skipped ? ["pile"] : []), "passed");
    if (s === 2) labels.push("ax0", "ax1", "share", "treatedSide", ...coinLabels);
    if (s === 3 || s === 7) labels.push(...S.map((_, i) => `arm${i}`));
    if (s === 3) labels.push("lInhibit", "lFinish", "lStop");
    if (s === 4) labels.push("lStruck", "lWhy");
    if (s === 7) labels.push("lWould");
    const cam = s <= 1 ? CAM.queue : s === 2 ? CAM.coin : s === 4 && strike ? { ...CAM.lock, target: lockTarget() } : CAM.lanes;
    return {
      dots,
      labels,
      cam,
      links: {
        gate: s <= 1 ? 0.7 : 0,
        axis: s === 2 ? 0.8 : 0,
        share: s === 2 ? 0.9 : 0,
        stems: s === 3 || s === 7 ? 0.35 : s === 4 ? 0.08 : 0,
        struckStems: s === 3 ? 0.35 : s === 4 ? 0.3 : 0,
        met: s === 3 ? 0.8 : s === 7 ? 0.5 : 0,
        lock: s === 4 ? 0.75 : 0,
      },
    };
  }
  function lockTarget(): V3 {
    const ps = struck.map((x) => pos.get(x.e.id)!.arm);
    const avg = (i: 0 | 2) => ps.reduce((a, p) => a + p[i], 0) / Math.max(1, ps.length);
    return [avg(0), 0, avg(2)];
  }
  function example(s: 5 | 6): SceneState {
    const dots: Record<string, DotLook> = {};
    const labels: string[] = [];
    if (s === 5) {
      dots.xPrompt = { pos: [-7.4, 0, 0], color: { hue: KIND_HUE.prompt }, glow: 0.5 };
      checks.forEach((c, k) => {
        const ok = c.contract && c.plain;
        dots[`xv${k}`] = { pos: [ok ? XG + 1.2 : -4.6, VY(k), 0], color: ok ? "accent" : "ink3", alpha: ok ? 1 : 0.55, scale: ok ? 1.3 : 1, glow: ok ? 0.8 : 0 };
        labels.push(`xv${k}`);
      });
      dots.xTpl = { pos: [3.6, -0.6, 0], color: "fire", glow: 0.9 };
      labels.push("xPrompt", "xGate", "xTpl");
    } else {
      dots.fDir = { pos: [FX(-1), 0, 0], color: "fire", glow: 0.8 };
      EX.batches.forEach((_, k) => {
        dots[`fB${k}`] = k === hit ? { pos: [FX(k), 0, 0], color: "accent", scale: 1.3, glow: 0.9 } : { pos: [FX(k), 0, 0], color: "ink3", alpha: k > hit ? 0.35 : 0.8, glow: 0 };
        labels.push(`fB${k}`);
      });
      labels.push("fDir", "fKey");
    }
    return {
      dots,
      labels,
      cam: s === 5 ? CAM.bind : CAM.follow,
      links: s === 5 ? { xIn: 0.35, xOk: 0.9, xGate: 0.7 } : { fLine: 0.5 },
    };
  }

  // ---------------------------------------------------------------- words

  const traces = useRepo ? `your ${S.length} most recent traces` : `the ${S.length} sample traces`;
  const text: Omit<Step, "state">[] = [
    {
      title: "Every hook event is a decision point",
      body: useRepo
        ? `Each row is one of ${traces}; each dot is a hook event the map recorded: a prompt, a tool batch, a failure, a stop. For each, the hook reads the payload, translates it through the harness adapter, and asks a pure function, decide(), what to say.`
        : `Each row is one of ${traces}, replayed as hook events: a session start, a prompt, tool batches, a failure, a stop. For each, the hook reads the payload, translates it through the harness adapter, and asks a pure function, decide(), what to say.`,
      chips: [
        { text: "payload → adapter → fast path → lock → arm → decide → spool → print" },
        { text: "fails open: errors go to fail-open.log" },
      ],
    },
    {
      title: "The fast path skips what cannot matter",
      body: useRepo
        ? `Before the state lock, the hook asks whether an event could change anything. Skipped events leave no spool record, so all ${passed.length} of the events shown here are ones that passed; the map never sees the rest.`
        : `Before the state lock, the hook asks whether an event could change anything. ${skipped} of ${all.length} events here stop: ${skippedBy("start")} session starts, ${skippedBy("stop")} stops with no finish line open, and ${skippedBy("batch") + skippedBy("failure")} batches with no trigger word while nothing was pending. The check is on words, not parsed commands, so a read of src/registry/load.ts passes: it contains every piece of the glob src/*.ts.`,
      chips: [
        { text: `no match: p95 budget ${LATENCY[0]!.p95} ms · measured ${LATENCY[0]!.measured} ms` },
        { text: "never skips an event that would decide something" },
      ],
      figure: latencyFigure,
    },
    {
      title: "A hash picks the arm",
      body: `The trace id is hashed: x is the first 4 bytes of sha256("adage-arm:" + id), little-endian, divided by u32::MAX. A trace is control if x is below the share, ${CONTROL_SHARE} unless ADAGE_HOLDOUT or runtime.holdout says otherwise. ${
        useRepo
          ? `Computed here in your browser for your ${S.length} traces, ${agree} of ${S.length} land on the arm the hook recorded; an arm forced with ADAGE_ARM, or drawn with another share, would not.`
          : `Computed here in your browser: ${control.map((s) => `${s.name} (x ${s.x.toFixed(3)})`).join(" and ")} are control; the other ${treated.length} are treated.`
      }`,
      chips: [{ text: `control share ${CONTROL_SHARE}` }, { text: "same id → same arm, every event" }, { text: "stored with its propensity" }],
    },
    {
      title: "decide() answers by event",
      body: useRepo
        ? `Above each event are the reflexes it directed, coloured by strength: ${delivered} delivered in treated traces, ${would} recorded as would-have in control traces. A prompt runs inhibit announcements first, then prompt reflexes by strength and support, then intent; a batch judges follows and opens finish lines; a failure fires only on its signature; a stop asks at most ${MAX_STOP_REQUESTS} times.`
        : `At the first prompt the dead end is announced (inhibit). An edit to registry/*.csv raises the finish line and opens an obligation; make gen meets it in ${met} traces. ${stops.length ? `${stops.map((x) => x.s.name).join(", ")} tries to stop without it, so the stop hook asks the agent to run it first.` : ""} Edits to src/*.ts raise a candidate: run npm test.`,
      chips: [
        { text: "prompt: inhibit first · then strength, support · intent" },
        { text: "batch: follows, finish lines" },
        { text: "failure: signature in output" },
        { text: `stop: ≤ ${MAX_STOP_REQUESTS}, unmet obligations only` },
      ],
    },
    {
      title: "Caps and the inhibit lock",
      body: struck.length
        ? `Two rules prune what decide() found. At most ${MAX_PER_DECISION} directives per decision and ${MAX_PER_SESSION} per trace. And once an inhibit has spoken, nothing it avoids is said: the dead end announced at the first prompt says npm test fails, so ${struck.length} npm test candidates are struck, with their pending judgements.`
        : `Two rules prune what decide() found. At most ${MAX_PER_DECISION} directives per decision and ${MAX_PER_SESSION} per trace; here the most in one decision is ${perDecision} and in one trace ${perSession}. And once an inhibit has spoken, nothing it avoids is said. A struck directive leaves no trace in the record, so none shows here.`,
      chips: [
        { text: `≤ ${MAX_PER_DECISION} per decision · most here ${perDecision}`, ok: perDecision <= MAX_PER_DECISION },
        { text: `≤ ${MAX_PER_SESSION} per trace · most here ${perSession}`, ok: perSession <= MAX_PER_SESSION },
        { text: "approach_lock: inhibit beats run" },
      ],
    },
    {
      title: "Only a checked value reaches the template",
      body: `An example from acme-billing: the prompt "${EX.prompt}" meets the work-item lookup reflex. A value binds only if it matches the slot contract ${EX.contract} and is a plain token: at most ${PLAIN_MAX} characters, no leading dash, only letters, digits and _./:@=+,-. It fills a fixed template; the track never supplies prose.`,
      chips: [
        { text: `${checks.filter((c) => c.contract && c.plain).length} of ${checks.length} values bind` },
        { text: "template text only" },
        { text: "sanitised: no backticks, <, >, newlines" },
      ],
    },
    {
      title: "Three batches decide followed or ignored",
      body: `The directive is pending for ${FOLLOW_WINDOW} tool batches. Each batch's commands are matched on their canonical key: the directed command, or it with options added. Batch ${hit + 1} runs the lookup, so the directive is judged followed. Had ${FOLLOW_WINDOW} batches passed without it, it would count as ignored.${useRepo ? ` In your recorded decisions: ${followed} followed, ${ignored} ignored, ${met} finish lines met.` : ""}`,
      chips: [{ text: `follow window ${FOLLOW_WINDOW} batches` }, { text: "pattern::meets" }, { text: "followed · ignored · misfired" }],
    },
    {
      title: "Control records, and stays silent",
      body: `The same decision runs in both arms. Treated traces received ${delivered} directive${delivered === 1 ? "" : "s"}; the ${control.length} control trace${control.length === 1 ? "" : "s"} logged ${would} would-have directive${would === 1 ? "" : "s"} and received none. That gives the next chapter its comparison: what agents did with a reflex, against what they did without it.`,
      chips: [{ text: "would_have: both arms" }, { text: "directives: treated only" }, { text: `control ≈ ${Math.round(CONTROL_SHARE * 100)}% of traces` }],
    },
  ];

  return {
    source: useRepo ? `your ${S.length} latest traces` : `${S.length} sample traces`,
    legend: [
      { label: "session start", role: "hue", hue: KIND_HUE.start },
      { label: "prompt", role: "hue", hue: KIND_HUE.prompt },
      { label: "tool batch", role: "hue", hue: KIND_HUE.batch },
      { label: "failure", role: "hue", hue: KIND_HUE.failure },
      { label: "stop", role: "hue", hue: KIND_HUE.stop },
      { label: "fire", role: "fire" },
      { label: "prime", role: "prime" },
      { label: "inhibit", role: "inhibit" },
    ],
    steps: text.map((t, i) => ({ ...t, state: i === 5 ? example(5) : i === 6 ? example(6) : lanes(i) })),
  };
}

/** The latency budget (Design §13.2) against the measured hook, p95, as bars. */
function latencyFigure(g: CanvasRenderingContext2D, w: number, h: number, pal: Palette) {
  const max = Math.max(...LATENCY.map((l) => l.p95));
  const left = 78;
  const right = 54;
  const X = (ms: number) => left + (ms / max) * (w - left - right);
  const rowH = (h - 16) / LATENCY.length;
  g.font = "10px 'Red Hat Mono', ui-monospace, monospace";
  g.textBaseline = "middle";
  LATENCY.forEach((l, i) => {
    const y = 4 + i * rowH + rowH / 2;
    g.fillStyle = pal.css("ink2");
    g.textAlign = "left";
    g.fillText(l.path, 0, y);
    g.fillStyle = pal.css("line");
    g.globalAlpha = 0.25;
    g.fillRect(X(0), y - 5, X(l.p95) - X(0), 10);
    g.globalAlpha = 1;
    g.fillStyle = pal.css("accent");
    g.fillRect(X(0), y - 3, X(l.measured) - X(0), 6);
    g.strokeStyle = pal.css("ink3");
    g.beginPath();
    g.moveTo(X(l.p95) + 0.5, y - 7);
    g.lineTo(X(l.p95) + 0.5, y + 7);
    g.stroke();
    g.fillStyle = pal.css("ink1");
    g.fillText(`${l.measured} ms`, X(l.measured) + 5, y);
    g.fillStyle = pal.css("ink3");
    g.textAlign = "left";
    g.fillText(`≤ ${l.p95} ms`, X(l.p95) + 5, y);
  });
  g.fillStyle = pal.css("ink3");
  g.textAlign = "left";
  g.textBaseline = "bottom";
  g.fillText("p95: measured (bar) against budget (tick)", left, h);
}

export const hook: Chapter = {
  id: "hook",
  title: "Decide at the hook",
  status: "Built",
  kind: "built",
  summary:
    "Each hook event runs a pure decision against the index. About one trace in ten is control: Adage records what it would have said and stays silent.",
  source: "crates/adage-hook · crates/adage-runtime/src/{lib,fast,render}.rs",
  usesRepo: true,
  build,
};
