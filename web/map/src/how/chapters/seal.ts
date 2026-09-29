// Chapter: seal the index (crates/adage-tracks/src/{index,interlock,io,model}.rs, adage-instincts,
// adage-pack). Reflexes live in versioned tracks; every installed track compiles into one
// index.json: duplicates by (repo, id) keep the first-hand one, the interlock resolves conflicts,
// and the survivors are hashed into the generation.
//
// Sample mode: nine sample reflexes in three tracks. Which survive, which are weakened or locked,
// and the generation, are all computed here by the rules of Index::compile and interlock::resolve.
// This repository: the map's own routes, grouped into their tracks, and the real generation.

import type { Route as ApiRoute } from "../../api/types";
import { strength, type Strength } from "../../lib/words";
import type { CamSpec, DotLook, SceneState, Stage, V3 } from "../engine";
import { f2, wilson } from "../sample";
import type { Built, Chapter, Context, Step } from "../types";
import { stable } from "./train";

// ------------------------------------------------------------------ constants (cited)

/** index.rs: the generation is this many hex digits of the SHA-256 of the serialised routes. */
const GEN_HEX = 16;
/** adage-instincts data/catalogue.toml (catalogue.rs test): entries, and those enabled
 * (U1 U3 U4 U5 U6 N1 P1 U11). */
const CATALOGUE = 19;
const ENABLED = 8;
const INSTINCT_PROV = "instinct:core@0.1.0";
/** adage-learn instincts.rs: auto-extinction (EXTINGUISH_*). */
const RETIRE_MISFIRES = 3;
const RETIRE_MIN_JUDGEMENTS = 5;
const RETIRE_IGNORED_LB = 0.5;
/** adage-tracks review.rs MATERIAL_LOWER_BOUND. */
const MATERIAL = 0.1;
/** params.rs: the fire bar. */
const FIRE_LB = 0.8;

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

// ------------------------------------------------------------------ SHA-256 (FIPS 180-4)

/** SHA-256 of a UTF-8 string, as hex. Synchronous, so a step's words can state the result; the
 * test checks it against SubtleCrypto. */
export function sha256hex(text: string): string {
  const K = new Uint32Array([
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ]);
  const bytes = new TextEncoder().encode(text);
  const len = bytes.length;
  const padded = new Uint8Array(((len + 9 + 63) >> 6) << 6);
  padded.set(bytes);
  padded[len] = 0x80;
  const bits = len * 8;
  const dv = new DataView(padded.buffer);
  dv.setUint32(padded.length - 8, Math.floor(bits / 2 ** 32));
  dv.setUint32(padded.length - 4, bits >>> 0);
  const H = new Uint32Array([0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19]);
  const W = new Uint32Array(64);
  const rotr = (x: number, n: number) => (x >>> n) | (x << (32 - n));
  for (let off = 0; off < padded.length; off += 64) {
    for (let i = 0; i < 16; i++) W[i] = dv.getUint32(off + i * 4);
    for (let i = 16; i < 64; i++) {
      const a = W[i - 15]!;
      const b = W[i - 2]!;
      const s0 = rotr(a, 7) ^ rotr(a, 18) ^ (a >>> 3);
      const s1 = rotr(b, 17) ^ rotr(b, 19) ^ (b >>> 10);
      W[i] = (W[i - 16]! + s0 + W[i - 7]! + s1) >>> 0;
    }
    let [a, b, c, d, e, f, g, h] = [H[0]!, H[1]!, H[2]!, H[3]!, H[4]!, H[5]!, H[6]!, H[7]!];
    for (let i = 0; i < 64; i++) {
      const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (h + S1 + ch + K[i]! + W[i]!) >>> 0;
      const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const mj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (S0 + mj) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }
    H[0] = (H[0]! + a) >>> 0;
    H[1] = (H[1]! + b) >>> 0;
    H[2] = (H[2]! + c) >>> 0;
    H[3] = (H[3]! + d) >>> 0;
    H[4] = (H[4]! + e) >>> 0;
    H[5] = (H[5]! + f) >>> 0;
    H[6] = (H[6]! + g) >>> 0;
    H[7] = (H[7]! + h) >>> 0;
  }
  return [...H].map((x) => x.toString(16).padStart(8, "0")).join("");
}

// ------------------------------------------------------------------ the sample reflexes

type Source = "first-hand" | "imported" | "instinct";
type TrigKind = "after_edit" | "after_failure" | "prompt";

interface Reflex {
  key: string;
  id: string;
  source: Source;
  track: string;
  version: string;
  trigger: { kind: TrigKind; on: string };
  step: { kind: "run" | "avoid" | "read" | "advise"; command: string; instead?: string };
  strength: Strength;
  state: "forming" | "firm";
  support: number;
  opportunities: number;
}

const REPO = "registry-svc";
const FL_GEN = `finish-line-${stable(["registry/*.csv", "make gen"])}`;
const DEAD = `dead-end-${stable(["exec make check"])}`;

/** Nine reflexes in three places: learned here (one track, `<repo>/learned`), one imported pack,
 * and the bound instincts. */
const SAMPLE: Reflex[] = [
  { key: "A1", id: FL_GEN, source: "first-hand", track: `${REPO}/learned`, version: "1.0.0", trigger: { kind: "after_edit", on: "registry/*.csv" }, step: { kind: "run", command: "make gen" }, strength: "fire", state: "firm", support: 26, opportunities: 27 },
  { key: "A2", id: `finish-line-${stable(["registry/**", "make registry"])}`, source: "first-hand", track: `${REPO}/learned`, version: "1.0.0", trigger: { kind: "after_edit", on: "registry/**" }, step: { kind: "run", command: "make registry" }, strength: "fire", state: "firm", support: 20, opportunities: 20 },
  { key: "A3", id: DEAD, source: "first-hand", track: `${REPO}/learned`, version: "1.0.0", trigger: { kind: "after_failure", on: "npm test" }, step: { kind: "avoid", command: "npm test", instead: "make check" }, strength: "inhibit", state: "firm", support: 9, opportunities: 9 },
  { key: "A4", id: "lookup-carrier", source: "first-hand", track: `${REPO}/learned`, version: "1.0.0", trigger: { kind: "prompt", on: "a carrier id" }, step: { kind: "run", command: "scripts/carrier {id}" }, strength: "prime", state: "forming", support: 7, opportunities: 11 },
  { key: "B1", id: DEAD, source: "imported", track: "ops-team/registry", version: "0.3.0", trigger: { kind: "after_failure", on: "npm test" }, step: { kind: "avoid", command: "npm test", instead: "make check" }, strength: "inhibit", state: "forming", support: 14, opportunities: 15 },
  { key: "B2", id: `finish-line-${stable(["src/*.ts", "npm test"])}`, source: "imported", track: "ops-team/registry", version: "0.3.0", trigger: { kind: "after_edit", on: "src/*.ts" }, step: { kind: "run", command: "npm test" }, strength: "prime", state: "forming", support: 12, opportunities: 16 },
  { key: "B3", id: `read-linked-${stable(["registry/*.csv"])}`, source: "imported", track: "ops-team/registry", version: "0.3.0", trigger: { kind: "after_edit", on: "registry/*.csv" }, step: { kind: "read", command: "src/registry/schema.ts" }, strength: "prime", state: "forming", support: 10, opportunities: 14 },
  { key: "I1", id: FL_GEN, source: "instinct", track: `${REPO}/instincts`, version: "0.1.0", trigger: { kind: "after_edit", on: "registry/*.csv" }, step: { kind: "run", command: "make gen" }, strength: "prime", state: "forming", support: 0, opportunities: 0 },
  { key: "I2", id: `instinct-u4-${stable(["gen/**"])}`, source: "instinct", track: `${REPO}/instincts`, version: "0.1.0", trigger: { kind: "after_edit", on: "gen/**" }, step: { kind: "advise", command: "don't hand-edit generated files" }, strength: "prime", state: "forming", support: 0, opportunities: 0 },
];

const PREC: Record<Source, number> = { "first-hand": 0, imported: 1, instinct: 2 };

function globsOverlap(a: string, b: string): boolean {
  const re = (g: string) => new RegExp(`^${g.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*\*/g, "\u0000").replace(/\*/g, "[^/]*").replace(/\u0000/g, ".*")}$`);
  return a === b || re(a).test(b) || re(b).test(a);
}
const samePoint = (x: Reflex, y: Reflex) =>
  x.trigger.kind === y.trigger.kind && (x.trigger.kind === "after_edit" ? globsOverlap(x.trigger.on, y.trigger.on) : x.trigger.on === y.trigger.on);

/** Index::compile and interlock::resolve, on the sample. */
export function compileSample(rs: Reflex[] = SAMPLE) {
  // De-duplicate by (repo, id): first-hand, then imported, then instinct.
  const sorted = [...rs].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : PREC[a.source] - PREC[b.source]));
  const kept: Reflex[] = [];
  const dropped: { lost: Reflex; to: Reflex }[] = [];
  for (const r of sorted) {
    const w = kept.find((k) => k.id === r.id);
    if (w) dropped.push({ lost: r, to: w });
    else kept.push(r);
  }
  kept.sort((a, b) => (a.track + "\u0000" + a.id < b.track + "\u0000" + b.id ? -1 : 1));
  // Polarity: an avoid step against a route that runs the avoided command. The dead end here is
  // announced after a failure, not at session start, so the other route stays and is locked at
  // runtime (never emitted beside or after the inhibit).
  const locked: { by: Reflex; route: Reflex }[] = [];
  for (const red of kept)
    for (const o of kept)
      if (red !== o && red.step.kind === "avoid" && o.step.kind === "run" && o.step.command === red.step.command) locked.push({ by: red, route: o });
  // Two fires at one decision point: learned first, then more support, then the id.
  const out = kept.map((r) => ({ ...r }));
  const greens = out
    .filter((r) => r.strength === "fire" && r.step.kind === "run")
    .sort((a, b) => (a.source === "first-hand" ? 0 : 1) - (b.source === "first-hand" ? 0 : 1) || b.support - a.support || (a.id < b.id ? -1 : 1));
  const keptGreen: Reflex[] = [];
  const demoted: { winner: Reflex; loser: Reflex }[] = [];
  for (const g of greens) {
    const k = keptGreen.find((x) => samePoint(x, g) && x.step.command !== g.step.command);
    if (k) {
      demoted.push({ winner: k, loser: { ...g } });
      g.strength = "prime";
    } else keptGreen.push(g);
  }
  // The generation: SHA-256 over the serialised routes, first 16 hex digits.
  const body = JSON.stringify(
    out.map((r) => ({
      repo: REPO,
      track: r.track,
      version: r.version,
      route: { id: r.id, trigger: r.trigger, step: r.step, strength: r.strength, state: r.state, support: r.support, opportunities: r.opportunities },
    })),
  );
  const hash = sha256hex(body);
  return { out, dropped, locked, demoted, body, hash, generation: hash.slice(0, GEN_HEX) };
}

// ------------------------------------------------------------------ geometry

const CAM: Record<"tracks" | "merge" | "seal", CamSpec> = {
  tracks: { target: [0, 0, 0], yaw: -0.08, pitch: 0.14, dist: 22 },
  merge: { target: [0, 0.2, 0], yaw: 0.1, pitch: 0.16, dist: 19 },
  seal: { target: [0, 0, 0], yaw: -0.18, pitch: 0.3, dist: 19 },
};

const box = (cx: number, cy: number, hw: number, hh: number): [V3, V3][] => {
  const a: V3 = [cx - hw, cy - hh, 0];
  const b: V3 = [cx + hw, cy - hh, 0];
  const c: V3 = [cx + hw, cy + hh, 0];
  const d: V3 = [cx - hw, cy + hh, 0];
  return [[a, b], [b, c], [c, d], [d, a]];
};

const ring = (i: number, n: number, r = 3.4): V3 => {
  const t = -Math.PI / 2 + (i / Math.max(1, n)) * Math.PI * 2;
  return [Math.cos(t) * r, -Math.sin(t) * r, 0];
};

const stepText = (r: Reflex) =>
  r.step.kind === "avoid" ? `avoid ${r.step.command} → ${r.step.instead}` : r.step.kind === "read" ? `read ${r.step.command}` : r.step.kind === "advise" ? "advise" : r.step.command;

// ------------------------------------------------------------------ sample walk

function buildSample(stage: Stage): Built {
  const narrow = typeof window !== "undefined" && window.innerWidth < 640;
  const c = compileSample();
  const byKey = new Map(SAMPLE.map((r) => [r.key, r]));
  const SRC: Source[] = ["first-hand", "imported", "instinct"];
  const colX: Record<Source, number> = { "first-hand": -6, imported: 0, instinct: 6 };
  const inCol = (s: Source) => SAMPLE.filter((r) => r.source === s);
  const colPos = (r: Reflex): V3 => {
    const list = inCol(r.source);
    const i = list.indexOf(r);
    return [colX[r.source] - 1.9, 1.8 - i * 1.25, 0];
  };
  const mergedPos = (r: Reflex): V3 => {
    const i = c.out.findIndex((o) => o.key === r.key);
    return [-3.4, 3.6 - i * 1.2, 0];
  };
  const lamp = (a: Strength): DotLook["color"] => a;
  const outByKey = new Map(c.out.map((r) => [r.key, r]));
  const lockedR = c.locked[0];
  const demotedR = c.demoted[0];

  for (const r of SAMPLE) stage.dot(r.key, 0.22);
  stage.dot("seal", 0.4);
  SRC.forEach((s) => stage.link(`folder-${s}`, box(colX[s], -0.1, 2.6, 3.1), "ink3"));
  stage.link("ringBox", box(0, 0, 4.4, 4.4), "accent");
  stage.link("toSeal", c.out.map((r) => [r.key, "seal"] as [string, string]), "accent");
  if (lockedR) stage.link("lock", [[lockedR.by.key, lockedR.route.key]], "inhibit");
  if (demotedR) stage.link("pair", [[demotedR.winner.key, demotedR.loser.key]], "ink2");
  const dupPairs = c.dropped.map((d) => [d.lost.key, d.to.key] as [string, string]);
  stage.link("dup", dupPairs, "ink2");

  // Labels.
  const folderHead: Record<Source, string> = {
    "first-hand": `<small>FIRST-HAND · LEARNED HERE</small>tracks/${REPO}/…`,
    imported: `<small>IMPORTED PACK · FORMING</small>@imported/ops-team/registry`,
    instinct: `<small>INSTINCTS</small>${INSTINCT_PROV}`,
  };
  SRC.forEach((s) => stage.label(`h-${s}`, narrow ? s : folderHead[s], [colX[s], 3.0, 0], { cls: "chip big", center: true, dy: -52 }));
  for (const r of SAMPLE) {
    const trig = r.trigger.kind === "after_edit" ? `edit ${r.trigger.on}` : r.trigger.kind === "after_failure" ? `${r.trigger.on} fails` : r.trigger.on;
    const short = r.step.kind === "avoid" ? `avoid ${r.step.command}` : r.step.kind === "read" ? `read ${r.step.command.split("/").pop()}` : stepText(r);
    stage.label(`l-${r.key}`, `${esc(short)}<br><span class="how-dim">${esc(trig)}</span>`, r.key, { dx: 14, dy: -13 });
    stage.label(`m-${r.key}`, narrow ? esc(r.id) : `${esc(r.id)} <span class="how-dim">· ${r.source} · ${esc(stepText(r))}</span>`, r.key, { dx: 16, dy: -15 });
  }
  const a1 = byKey.get("A1")!;
  stage.label("ver", `<small>${esc(a1.track)} · track.toml + routes/&lt;id&gt;.yaml</small>0.1.0 created → 0.1.1 evidence → 0.2.0 route added → 1.0.0 route changed`, [colX["first-hand"], -3.2, 0], {
    cls: "chip big",
    dx: -120,
    dy: 16,
  });
  stage.label("pack", "pack.toml<br>routes/*.yaml<br>scripts/sha256-&lt;hex&gt;<br>evidence.json<br>CLOSURE<br>pack.sig <span class=\"how-dim\">· sshsig</span>", [colX.imported, -3.2, 0], {
    cls: "chip",
    center: true,
    dy: 14,
  });
  stage.label("inst", `${ENABLED} of ${CATALOGUE} enabled · prime at most`, [colX.instinct, -3.2, 0], { cls: "chip", center: true, dy: 14 });
  for (const d of c.dropped) stage.label(`drop-${d.lost.key}`, `${d.lost.source} copy dropped: ${d.to.source} wins`, d.to.key, { cls: "note", dx: 16, dy: 3 });
  if (lockedR) {
    stage.label("lockNote", `locked: never beside or after the inhibit`, lockedR.route.key, { cls: "note", dx: 16, dy: 10 });
  }
  if (demotedR) stage.label("demNote", `fewer traces (${demotedR.loser.support} vs ${demotedR.winner.support}): compiled as prime`, demotedR.loser.key, { cls: "note", dx: 16, dy: 10 });
  stage.label("gen", `<small>INDEX.JSON · GENERATION</small>${c.generation}`, [0, -4.4, 0], { cls: "chip big", center: true, dy: 12 });
  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    const labels: string[] = [];
    const links: Record<string, number> = {};
    if (s <= 3) {
      const focus: Source | null = s === 1 ? "first-hand" : s === 2 ? "imported" : s === 3 ? "instinct" : null;
      for (const r of SAMPLE) {
        const on = !focus || r.source === focus;
        dots[r.key] = {
          pos: colPos(r),
          color: lamp(r.strength),
          alpha: on ? (r.state === "forming" ? 0.8 : 1) : 0.2,
          scale: s === 1 && r.key === "A1" ? 1.35 : 1,
          glow: on ? 0.5 : 0,
        };
        if (on && !narrow) labels.push(`l-${r.key}`);
      }
      SRC.forEach((src) => {
        links[`folder-${src}`] = !focus || focus === src ? 0.45 : 0.12;
        if (!focus || focus === src) labels.push(`h-${src}`);
      });
      if (s === 1 && !narrow) labels.push("ver");
      if (s === 2) labels.push("pack");
      if (s === 3) labels.push("inst");
    } else if (s <= 6) {
      // One column: the survivors in index order; each lost duplicate sits on its winner.
      for (const r of SAMPLE) {
        const o = outByKey.get(r.key);
        const lost = c.dropped.find((d) => d.lost.key === r.key);
        if (lost) {
          if (s === 4) {
            const w = mergedPos(lost.to);
            dots[r.key] = { pos: [w[0] - 1.1, w[1], w[2]], color: lamp(r.strength), alpha: 0.3, scale: 0.8, glow: 0 };
          }
          continue;
        }
        let pos = mergedPos(r);
        let color = lamp(o!.strength);
        let alpha = 1;
        let scale = 1;
        let glow = 0.4;
        if (s === 6 && demotedR && r.key !== demotedR.winner.key && r.key !== demotedR.loser.key) alpha = 0.2;
        if (s === 5 && lockedR && r.key !== lockedR.by.key && r.key !== lockedR.route.key) alpha = 0.2;
        if (s === 5 && lockedR && r.key === lockedR.route.key) {
          pos = [pos[0] + 2.2, pos[1], pos[2]];
          alpha = 0.6;
          scale = 1.2;
        }
        if (s === 5 && lockedR && r.key === lockedR.by.key) {
          scale = 1.35;
          glow = 0.9;
        }
        if (s === 6 && demotedR && (r.key === demotedR.winner.key || r.key === demotedR.loser.key)) {
          scale = 1.3;
          glow = 0.8;
          if (r.key === demotedR.loser.key) pos = [pos[0] + 2.2, pos[1], pos[2]];
        }
        if (s < 6 && demotedR && r.key === demotedR.loser.key) color = lamp(demotedR.loser.strength);
        dots[r.key] = { pos, color, alpha, scale, glow };
        if (s === 4 || (s === 5 && lockedR && (r.key === lockedR.by.key || r.key === lockedR.route.key)) || (s === 6 && demotedR && (r.key === demotedR.winner.key || r.key === demotedR.loser.key)))
          labels.push(`m-${r.key}`);
      }
      if (s === 4) {
        links.dup = 0.5;
        if (!narrow) labels.push(...c.dropped.map((d) => `drop-${d.lost.key}`));
      }
      if (s === 5 && lockedR) {
        links.lock = 0.95;
        if (!narrow) labels.push("lockNote");
      }
      if (s === 6 && demotedR) {
        links.pair = 0.5;
        if (!narrow) labels.push("demNote");
      }
    } else {
      c.out.forEach((r, i) => (dots[r.key] = { pos: ring(i, c.out.length), color: lamp(r.strength), glow: 0.5 }));
      dots.seal = { pos: [0, 0.2, 0], color: "accent", scale: 1, glow: 1 };
      links.toSeal = 0.35;
      links.ringBox = 0.5;
      labels.push("gen");
    }
    const cam = s <= 3 ? CAM.tracks : s <= 6 ? CAM.merge : CAM.seal;
    return { dots, labels, links, cam };
  }

  const fh = SAMPLE.filter((r) => r.source === "first-hand").length;
  const im = SAMPLE.filter((r) => r.source === "imported").length;
  const ins = SAMPLE.filter((r) => r.source === "instinct").length;
  const text: Omit<Step, "state">[] = [
    {
      title: "Reflexes live in tracks",
      body: `Everything Adage conditions in a repository goes into one track directory, <repo>/learned: a track.toml, one routes/<id>.yaml per reflex, and a .why.json beside them that says why each exists (read by people, never sent to the agent). Instincts get their own track. Tracks and the index are readable by their owner only. This sample repository has ${fh} reflexes learned here, ${im} from an imported pack, and ${ins} instincts.`,
      chips: [{ text: "tracks/<repo>/learned/track.toml" }, { text: "routes/<id>.yaml · .why.json" }, { text: "mode 0600" }, { text: `${SAMPLE.length} sample reflexes` }],
    },
    {
      title: "Versions say what changed",
      body: "A new track starts at 0.1.0. When only the evidence moves, the patch number goes up. An added reflex bumps the minor number; a changed or removed reflex bumps the major number, so anything that reads the track can tell.",
      chips: [{ text: "new: 0.1.0" }, { text: "evidence only: patch" }, { text: "added: minor" }, { text: "changed or removed: major" }],
    },
    {
      title: "Imported packs arrive forming",
      body: `A pack is a signed, self-contained track from a teammate: its reflexes, any scripts named by their own hash, its evidence and a closure listing. Imported reflexes start forming and never fire until your own traces confirm them.`,
      chips: [{ text: "adage-pack/1" }, { text: "sshsig · namespace adage-pack" }, { text: "imports never fire" }, { text: `material change: lower bound moves ≥ ${f2(MATERIAL)}` }],
    },
    {
      title: "Instincts ship with Adage",
      body: `Instincts are bound on day one from files the repository declares, like its Makefile, package.json, CI and lockfiles. They are delivered as prime at most, only react to events, and never block the agent from stopping. One, U11, points a Python test run at the project's verified .venv before it runs. An instinct that keeps misfiring extinguishes itself.`,
      chips: [
        { text: `${ENABLED} of ${CATALOGUE} enabled` },
        { text: INSTINCT_PROV },
        { text: `extinguish: ${RETIRE_MISFIRES} misfires` },
        { text: `or ≥ ${RETIRE_MIN_JUDGEMENTS} judgements, ignored LB ≥ ${f2(RETIRE_IGNORED_LB)}` },
      ],
    },
    {
      title: "Duplicates collide",
      body: `All installed tracks compile into one list. The same behaviour gets the same id wherever it was found, so ids collide: ${c.dropped.length} copies are dropped. Your own evidence beats a teammate's pack, and both beat an instinct. ${c.out.length} reflexes remain.`,
      chips: [{ text: "key: (repo, id)" }, { text: "first-hand > imported > instinct" }, ...c.dropped.map((d) => ({ text: `${d.lost.key} → ${d.to.key}` }))],
    },
    {
      title: "Inhibit beats fire",
      body: lockedR
        ? `The dead end says npm test fails here; the imported reflex says to run it. Inhibit wins, whatever the source. Because the dead end speaks after a failure, not at the first prompt, the other reflex stays in the index but is locked: it is never delivered beside or after the warning.`
        : "No reflex here runs a command a dead end avoids.",
      chips: [{ text: "suppressed if the inhibit speaks at session start" }, { text: "otherwise locked at runtime" }],
    },
    {
      title: "One fire per decision point",
      body: demotedR
        ? `Two first-hand reflexes fire after an edit to overlapping paths, with different commands. One keeps fire: learned beats imported, then the one with more supporting traces (${demotedR.winner.support} against ${demotedR.loser.support}). The other is compiled as prime.`
        : "No two fires share a decision point here.",
      chips: demotedR
        ? [
            { text: `${demotedR.winner.support}/${demotedR.winner.opportunities} · LB ${f2(wilson(demotedR.winner.support, demotedR.winner.opportunities))}`, ok: wilson(demotedR.winner.support, demotedR.winner.opportunities) >= FIRE_LB },
            { text: `${demotedR.loser.support}/${demotedR.loser.opportunities} · LB ${f2(wilson(demotedR.loser.support, demotedR.loser.opportunities))} → prime` },
            { text: "instincts last" },
          ]
        : [{ text: "instincts last" }],
    },
    {
      title: "The index seals",
      body: `The ${c.out.length} survivors are serialised in order and hashed. The first ${GEN_HEX} hex digits of the SHA-256 are the generation, ${c.generation}. Any change to any reflex gives a new generation, and the hook reads only this one file.`,
      chips: [{ text: `generation = sha256[..${GEN_HEX}]` }, { text: "trigger words derived, not hashed" }, { text: "written atomically" }],
    },
  ];

  return {
    source: `${SAMPLE.length} sample reflexes`,
    legend: [
      { label: "fire", role: "fire" },
      { label: "prime", role: "prime" },
      { label: "inhibit", role: "inhibit" },
    ],
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

// ------------------------------------------------------------------ this repository

type Origin = "first-hand" | "imported" | "instinct";
const originOf = (r: ApiRoute): Origin => (r.instinct ? "instinct" : r.imported ? "imported" : "first-hand");

function buildRepo(stage: Stage, routes: ApiRoute[], generation: string): Built {
  // Tracks, first-hand first, then imported, then instincts; within, by name.
  const tracks = new Map<string, ApiRoute[]>();
  for (const r of routes) tracks.set(r.track, [...(tracks.get(r.track) ?? []), r]);
  const order = [...tracks.keys()].sort((a, b) => PREC[originOf(tracks.get(a)![0]!)] - PREC[originOf(tracks.get(b)![0]!)] || (a < b ? -1 : 1));
  const shown = order.slice(0, 6);
  const hidden = order.length - shown.length;
  const PER_COL = 8;
  const colW = (t: string) => Math.ceil(tracks.get(t)!.length / PER_COL);
  const widths = shown.map((t) => colW(t) * 0.9 + 3.8);
  const total = widths.reduce((a, b) => a + b, 0);
  const x0: number[] = [];
  let acc = -total / 2;
  for (const w of widths) {
    x0.push(acc + 0.4);
    acc += w;
  }
  const pos = new Map<string, V3>();
  const ids: string[] = [];
  shown.forEach((t, ti) =>
    tracks.get(t)!.forEach((_, i) => {
      const id = `r${ids.length}`;
      ids.push(id);
      pos.set(id, [x0[ti]! + Math.floor(i / PER_COL) * 0.9, 2.6 - (i % PER_COL) * 0.85, 0]);
      stage.dot(id, 0.2);
    }),
  );
  const list = shown.flatMap((t) => tracks.get(t)!);
  const inIdx = list.map((r) => r.in_index);
  const nIn = routes.filter((r) => r.in_index).length;
  stage.dot("seal", 0.4);
  const ringIds = ids.filter((_, i) => inIdx[i]);
  stage.link("toSeal", ringIds.map((id) => [id, "seal"] as [string, string]), "accent");
  stage.link("ringBox", box(0, 0, 4.4, 4.4), "accent");
  const rows = (t: string) => Math.min(PER_COL, tracks.get(t)!.length);
  shown.forEach((t, ti) => stage.link(`folder${ti}`, box(x0[ti]! - 0.4 + widths[ti]! / 2 - 0.2, 2.6 - ((rows(t) - 1) * 0.85) / 2, widths[ti]! / 2 - 0.3, (rows(t) - 1) * 0.425 + 0.7), "ink3"));
  const camTracks: CamSpec = { target: [0, 2.6 - ((PER_COL - 1) * 0.85) / 2 + 1, 0], yaw: -0.06, pitch: 0.12, dist: Math.max(14, Math.min(24, 8 + total * 1.1)) };
  shown.forEach((t, ti) => {
    const r0 = tracks.get(t)![0]!;
    stage.label(`t${ti}`, `<small>${originOf(r0).toUpperCase()}</small>${esc(t)} <span class="how-dim">${esc(r0.version)}</span>`, [x0[ti]!, 2.6, 0], { cls: "chip big", dy: -58 });
  });
  list.forEach((r, i) => stage.label(`l${i}`, esc(r.id), ids[i]!, { dx: 12, dy: -7 }));
  stage.label("gen", `<small>YOUR INDEX · GENERATION</small>${esc(generation || "none yet")}`, [0, -4.4, 0], { cls: "chip big", center: true, dy: 12 });

  const count = (o: Origin) => routes.filter((r) => originOf(r) === o).length;
  const strengths = { fire: 0, prime: 0, inhibit: 0 } as Record<Strength, number>;
  for (const r of routes) if (r.in_index) strengths[strength(r.strength) ?? "prime"]++;
  const labelsFor = (n: number) => (list.length <= 12 ? list.map((_, i) => `l${i}`).slice(0, n) : []);

  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    const labels: string[] = shown.map((_, ti) => `t${ti}`);
    ids.forEach((id, i) => {
      const r = list[i]!;
      const lamp = strength(r.strength) ?? "prime";
      const p = pos.get(id)!;
      if (s <= 1) dots[id] = { pos: p, color: s === 0 ? "ink2" : lamp, glow: 0.4 };
      else if (s === 2) dots[id] = { pos: r.in_index ? p : [p[0], p[1] - 0.35, p[2]], color: lamp, alpha: r.in_index ? 1 : 0.2, glow: r.in_index ? 0.5 : 0 };
      else if (r.in_index) dots[id] = { pos: ring(ringIds.indexOf(id), ringIds.length), color: lamp, glow: 0.5 };
    });
    if (s < 3) labels.push(...labelsFor(99));
    else {
      labels.length = 0;
      labels.push("gen");
      dots.seal = { pos: [0, 0.2, 0], color: "accent", glow: 1 };
    }
    const links: Record<string, number> = s === 3 ? { toSeal: 0.35, ringBox: 0.5 } : {};
    if (s < 3) shown.forEach((_, ti) => (links[`folder${ti}`] = 0.4));
    return { dots, labels, links, cam: s < 3 ? camTracks : CAM.seal };
  }

  const text: Omit<Step, "state">[] = [
    {
      title: "Your reflexes, in their tracks",
      body: `This repository's map has ${routes.length} reflexes in ${order.length} track${order.length === 1 ? "" : "s"}${hidden ? `; the ${shown.length} largest sources are shown` : ""}. Each track is a directory with a track.toml and one file per reflex.`,
      chips: [{ text: `${count("first-hand")} first-hand` }, { text: `${count("imported")} imported` }, { text: `${count("instinct")} instinct` }],
    },
    {
      title: "Each has a strength",
      body: `Colour is strength as compiled: fire, prime or inhibit. Of the reflexes in your index, ${strengths.fire} fire, ${strengths.prime} prime and ${strengths.inhibit} inhibit.`,
      chips: [{ text: `${strengths.fire} fire` }, { text: `${strengths.prime} prime` }, { text: `${strengths.inhibit} inhibit` }],
    },
    {
      title: "What the index serves",
      body: `${nIn} of ${routes.length} are in the index. A reflex is left out when it is nascent or extinguished, when a first-hand copy of it exists, or when the interlock suppressed it.`,
      chips: [{ text: `${nIn} in the index` }, { text: `${routes.length - nIn} left out` }, { text: "first-hand > imported > instinct" }],
    },
    {
      title: "Your index is sealed",
      body: `adage compiled these ${nIn} reflexes into index.json. Its generation, ${generation || "not yet written"}, is your real one, read from the map: the page does not recompute it, because adage serialises the full routes, which the map does not carry.`,
      chips: [{ text: `generation = sha256[..${GEN_HEX}]` }, { text: "trigger words not hashed" }],
    },
  ];
  return {
    source: `your ${routes.length} reflexes`,
    legend: [
      { label: "fire", role: "fire" },
      { label: "prime", role: "prime" },
      { label: "inhibit", role: "inhibit" },
    ],
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

export const seal: Chapter = {
  id: "index",
  title: "Seal the index",
  status: "Built",
  kind: "built",
  summary: "Every installed track compiles into one index. Duplicates and conflicts resolve by rule, and the result is identified by a hash of its contents.",
  source: "crates/adage-tracks/src/{index,interlock}.rs · adage-instincts · adage-pack",
  usesRepo: true,
  build(stage: Stage, ctx: Context): Built {
    const routes = ctx.graph?.routes ?? [];
    if (ctx.repo && routes.length) return buildRepo(stage, routes, ctx.graph?.generation ?? "");
    return buildSample(stage);
  },
};
