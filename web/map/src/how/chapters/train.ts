// Chapter: train on a skill (crates/adage-train, docs/training.md). A SKILL.md becomes typed
// claims; generated tasks run headlessly in sealed clones; the traces go through the same learner
// into staged reflexes that are never activated; each claim gets a verdict, and held-out tasks
// compare three arms.
//
// The skill, the repository's files and every session outcome below are sample data. Each count,
// rate and verdict a caption states is computed from them here, with the rules of plan.rs,
// tasks.rs, report.rs and the miners (mine.rs), whose constants are copied below.

import type { CamSpec, DotLook, SceneState, Stage, V3 } from "../engine";
import { f2, seeded, wilson } from "../sample";
import type { Built, Chapter, Step } from "../types";

// ------------------------------------------------------------------ constants (cited)

/** plan.rs `usd_per_session`: estimated cost per headless session, by model family. */
const USD_PER_SESSION = [
  ["Haiku", 0.15],
  ["Sonnet", 0.25],
  ["Opus", 0.75],
  ["other", 0.5],
] as const;
/** plan.rs DEFAULT_MODEL is claude-haiku-4-5; main.rs `--sessions` defaults to 24. */
const MODEL = "Haiku";
const SESSIONS = 24;
/** harness.rs / main.rs: `--timeout-secs` default. */
const TIMEOUT_S = 1800;
/** report.rs: acceptance bars and the "failed consistently" share. */
const FOLLOW_MIN = 0.7;
const NON_INFERIORITY = 0.1;
const COST_TOLERANCE = 0.1;
const CONSISTENT: [number, number] = [9, 10];
/** adage-tracks params.rs LearnParams::default(): the miners' bars. */
const MIN_SUPPORT = 5;
const FIRE_LB = 0.8;
const PRIME_RATE = 0.5;
const FINISH_CONTRAST = 0.3;

/** mine.rs `stable`: the 6-hex id suffix the miners derive from content (FNV-1a, 64-bit). */
export function stable(parts: string[]): string {
  let h = 0xcbf29ce484222325n;
  const mask = (1n << 64n) - 1n;
  for (const p of parts) {
    for (const b of [...new TextEncoder().encode(p), 0]) {
      h ^= BigInt(b);
      h = (h * 0x100000001b3n) & mask;
    }
  }
  return (h & 0xffffffn).toString(16).padStart(6, "0");
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

// ------------------------------------------------------------------ the sample skill

type Kind = "verify" | "regenerate_after" | "lookup" | "environment";

interface Claim {
  id: string;
  kind: Kind;
  sentence: string;
  command: string;
  glob?: string;
  example?: string;
  /** Why it is never run (claims.rs: above regenerate, or not one plain command). */
  unsafe?: string;
}

/** A SKILL.md for the registry service the sample traces come from. */
const SKILL: Claim[] = [
  { id: "c1", kind: "verify", sentence: "Run the tests with `npm test`.", command: "npm test" },
  { id: "c2", kind: "regenerate_after", sentence: "After editing `registry/*.csv`, run `make gen`.", command: "make gen", glob: "registry/*.csv" },
  { id: "c3", kind: "lookup", sentence: "Look up a carrier with `scripts/carrier HX-7`.", command: "scripts/carrier HX-7", example: "HX-7" },
  { id: "c4", kind: "regenerate_after", sentence: "After editing `proto/*.proto`, run `make proto`.", command: "make proto", glob: "proto/*.proto" },
  { id: "c5", kind: "environment", sentence: "Install dependencies with `npm install`.", command: "npm install", unsafe: "environment: above regenerate" },
];

/** The sample repository's files (what the planner walks). */
const FILES = ["Makefile", "package.json", "registry/carriers.csv", "registry/lanes.csv", "registry/ports.csv", "scripts/carrier", "src/dispatch.ts", "src/registry/load.ts", "src/router.ts"];

const HUE: Record<Kind, number> = { verify: 212, regenerate_after: 300, lookup: 130, environment: 40 };

function globMatch(glob: string, f: string): boolean {
  const re = new RegExp(`^${glob.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, "[^/]*")}$`);
  return re.test(f);
}

/** tasks.rs `exercisable`: why a claim can get no tasks, if it cannot. */
function notExercisable(c: Claim): string | null {
  if (c.unsafe) return `unsafe`;
  if (c.kind === "environment") return "environment claims are never exercised";
  if (c.glob && !FILES.some((f) => globMatch(c.glob!, f))) return `no file matches ${c.glob}`;
  return null;
}

// ------------------------------------------------------------------ plan: tasks (tasks.rs `generate`)

interface Task {
  claim: Claim | null;
  explicit: boolean;
}

function plan(): Task[] {
  const active = SKILL.filter((c) => !notExercisable(c));
  const n = SESSIONS;
  const controls = active.length ? Math.max(Math.ceil(n / 4), n > 0 ? 1 : 0) : n;
  const triggers = n - controls;
  const explicitN = Math.floor((triggers * 3 + 5) / 10);
  const explicit = Array.from({ length: triggers }, (_, i) => i < explicitN);
  const rnd = seeded(11);
  for (let i = explicit.length - 1; i > 0; i--) {
    const j = Math.floor(((rnd() + 1) / 2) * (i + 1)) % (i + 1);
    [explicit[i], explicit[j]] = [explicit[j]!, explicit[i]!];
  }
  const out: Task[] = [];
  for (let i = 0; i < triggers; i++) out.push({ claim: active[i % active.length]!, explicit: explicit[i]! });
  for (let i = 0; i < controls; i++) out.push({ claim: null, explicit: false });
  return out;
}

// ------------------------------------------------------------------ run: what each exploration session did

type Run = [cmd: string, ok: boolean];
interface Session {
  edits: string[];
  runs: Run[];
}

/** Sample outcomes: `npm test` has no test runner configured here and always fails. */
function explore(tasks: Task[]): Session[] {
  const perClaim: Record<string, number> = {};
  let ctl = 0;
  return tasks.map((t) => {
    const k = t.claim ? (perClaim[t.claim.id] = (perClaim[t.claim.id] ?? -1) + 1) : ctl++;
    const csv = FILES.filter((f) => f.startsWith("registry/"))[k % 3]!;
    if (!t.claim) {
      // Controls: questions and edits outside every claim. Three of them check their edit.
      if (k < 3) return { edits: ["src/router.ts"], runs: [["npm test", false], ["make check", true]] };
      return { edits: k === 3 ? ["src/dispatch.ts"] : [], runs: [["git status", true]] };
    }
    switch (t.claim.id) {
      case "c1":
        // The skill says npm test; it fails, and all but one session then find make check.
        return { edits: [], runs: k === 5 ? [["npm test", false]] : [["npm test", false], ["make check", true]] };
      case "c2":
        // Edit a registry file, regenerate after the last edit; one session also runs npm test.
        return { edits: [csv], runs: k === 0 ? [["make gen", true], ["npm test", false], ["make check", true]] : [["make gen", true]] };
      default:
        // Look up a carrier: half the sessions use the script, the rest grep the CSV.
        return { edits: [], runs: k % 2 === 0 ? [["scripts/carrier HX-7", true]] : [["grep HX-7 registry/carriers.csv", true]] };
    }
  });
}

// ------------------------------------------------------------------ validation (sample held-out outcomes)

type Arm = "baseline" | "skill" | "reflex";
const ARMS: Arm[] = ["baseline", "skill", "reflex"];
/** Per claim and arm, three held-out sessions: [followed, turns, cost in USD]. */
const HELD_OUT: Record<string, Record<Arm, [boolean, number, number][]>> = {
  c1: {
    baseline: [[false, 4, 0.11], [false, 3, 0.09], [false, 4, 0.12]],
    skill: [[false, 3, 0.09], [false, 3, 0.1], [false, 3, 0.09]],
    reflex: [[true, 2, 0.07], [true, 2, 0.06], [true, 2, 0.07]],
  },
  c2: {
    baseline: [[true, 3, 0.08], [false, 2, 0.06], [false, 2, 0.06]],
    skill: [[true, 3, 0.08], [true, 3, 0.09], [true, 3, 0.08]],
    reflex: [[true, 3, 0.08], [true, 3, 0.08], [true, 3, 0.09]],
  },
  c3: {
    baseline: [[false, 4, 0.1], [false, 3, 0.09], [false, 4, 0.1]],
    skill: [[true, 3, 0.08], [true, 3, 0.08], [false, 4, 0.1]],
    reflex: [[false, 4, 0.1], [false, 3, 0.09], [false, 4, 0.11]],
  },
};

const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / Math.max(1, xs.length);

// ------------------------------------------------------------------ the chapter

const CAM: Record<"doc" | "grid" | "pods" | "stage" | "verdict" | "held", CamSpec> = {
  doc: { target: [-3.6, 0, 0], yaw: 0.05, pitch: 0.12, dist: 17 },
  grid: { target: [0, 0.4, 0], yaw: -0.1, pitch: 0.2, dist: 22 },
  pods: { target: [2.6, 1.6, 0], yaw: -0.28, pitch: 0.3, dist: 20 },
  stage: { target: [0.4, -1.8, 0], yaw: 0.06, pitch: 0.24, dist: 22 },
  verdict: { target: [-1.6, 0, 0], yaw: -0.06, pitch: 0.14, dist: 21 },
  held: { target: [-1.4, 1.4, 0], yaw: -0.08, pitch: 0.16, dist: 19 },
};

function build(stage: Stage): Built {
  const narrow = typeof window !== "undefined" && window.innerWidth < 640;
  const tasks = plan();
  const sessions = explore(tasks);
  const controls = tasks.filter((t) => !t.claim).length;
  const triggers = tasks.length - controls;
  const explicitN = tasks.filter((t) => t.explicit).length;
  const implicitN = triggers - explicitN;
  const perUsd = USD_PER_SESSION.find(([m]) => m === MODEL)![1];
  const estimate = perUsd * tasks.length;
  const active = SKILL.filter((c) => !notExercisable(c));

  // --- mining, as the miners count it (in distinct sessions)
  const ranAll = (s: Session, cmd: string) => s.runs.filter(([c]) => c === cmd);
  // c1: sessions that ran npm test, and those where every run of it failed (report.rs `contradiction`).
  const ranTest = sessions.filter((s) => ranAll(s, "npm test").length > 0);
  const failedTest = ranTest.filter((s) => ranAll(s, "npm test").every(([, ok]) => !ok));
  const consistent = ranTest.length > 0 && failedTest.length * CONSISTENT[1] >= ranTest.length * CONSISTENT[0];
  // The dead end: npm test failed, then make check passed later in the same session.
  const deadSupport = sessions.filter((s) => {
    const f = s.runs.findIndex(([c, ok]) => c === "npm test" && !ok);
    return f >= 0 && s.runs.some(([c, ok], j) => j > f && c === "make check" && ok);
  }).length;
  // The dead-end miner also needs the avoided command to fail consistently (params.rs [9, 10]).
  const deadLearned = deadSupport >= MIN_SUPPORT && consistent;
  const deadId = `dead-end-${stable(["exec make check"])}`;
  // c2: a finish line. Opportunities: sessions that edited a matching file; support: those that
  // then ran make gen successfully; contrast against make gen in sessions that did not edit.
  const c2 = SKILL[1]!;
  const editors = sessions.filter((s) => s.edits.some((f) => globMatch(c2.glob!, f)));
  const flSupport = editors.filter((s) => s.runs.some(([c, ok]) => c === "make gen" && ok)).length;
  const others = sessions.filter((s) => !editors.includes(s));
  const genWithout = others.filter((s) => s.runs.some(([c]) => c === "make gen")).length;
  const rateWith = flSupport / Math.max(1, editors.length);
  const rateWithout = genWithout / Math.max(1, others.length);
  const flLb = wilson(flSupport, editors.length);
  const flLearned = flSupport >= MIN_SUPPORT && rateWith >= PRIME_RATE && rateWith - rateWithout >= FINISH_CONTRAST;
  const flFires = flLb >= FIRE_LB;
  const flId = `finish-line-${stable([c2.glob!, c2.command])}`;
  // c3: the lookup, followed when the script ran with the prompt's id.
  const c3Sessions = sessions.filter((_, i) => tasks[i]!.claim?.id === "c3");
  const lkFollowed = c3Sessions.filter((s) => s.runs.some(([c]) => c === "scripts/carrier HX-7")).length;
  const lkLearned = lkFollowed >= MIN_SUPPORT;

  // --- verdicts, in report.rs precedence: unsafe, contradicted, compiled, never exercised, not compiled.
  type Verdict = { kind: "unsafe" | "contradicted" | "compiled" | "never" | "not"; head: string; detail: string };
  const verdict = (c: Claim): Verdict => {
    if (c.unsafe) return { kind: "unsafe", head: "unsafe", detail: c.unsafe };
    if (c.id === "c1" && consistent && deadLearned)
      return { kind: "contradicted", head: "contradicted", detail: `failed ${failedTest.length}/${ranTest.length} · ${deadId} → make check` };
    if (c.id === "c2" && flLearned) return { kind: "compiled", head: "compiled", detail: `→ ${flId}` };
    if (c.id === "c3" && lkLearned) return { kind: "compiled", head: "compiled", detail: "→ a lookup reflex" };
    const why = notExercisable(c);
    if (why) return { kind: "never", head: "never exercised", detail: why };
    if (c.id === "c3") return { kind: "not", head: "not compiled", detail: `followed ${lkFollowed}/${c3Sessions.length} · support < ${MIN_SUPPORT}` };
    return { kind: "not", head: "not compiled", detail: "below the learning thresholds" };
  };
  const verdicts = SKILL.map(verdict);
  const vCount = (k: Verdict["kind"]) => verdicts.filter((v) => v.kind === k).length;

  // --- acceptance (report.rs): per claim, the reflex arm against the skill arm.
  const acc = ["c1", "c2", "c3"].map((id) => {
    const a = HELD_OUT[id]!;
    const st = Object.fromEntries(
      ARMS.map((arm) => {
        const xs = a[arm];
        return [arm, { rate: xs.filter(([f]) => f).length / xs.length, turns: mean(xs.map(([, t]) => t)), cost: mean(xs.map(([, , c]) => c)), n: xs.length }];
      }),
    ) as Record<Arm, { rate: number; turns: number; cost: number; n: number }>;
    const r = st.reflex;
    const s = st.skill;
    const ok = {
      follow: r.rate >= FOLLOW_MIN,
      near: r.rate >= s.rate - NON_INFERIORITY,
      turns: r.turns <= s.turns * (1 + COST_TOLERANCE) + 1e-9,
      cost: r.cost <= s.cost * (1 + COST_TOLERANCE) + 1e-9,
    };
    return { id, st, ok, accepted: ok.follow && ok.near && ok.turns && ok.cost };
  });
  const heldOut = acc.reduce((n, a) => n + a.st.skill.n, 0);

  // ------------------------------------------------------------------ geometry

  const CLAIM_Y = [3.6, 1.8, 0, -1.8, -3.6];
  const cPos = (i: number): V3 => [-7, CLAIM_Y[i]!, 0];
  const COLS = 6;
  const tPos = (i: number): V3 => [1.7 + (i % COLS) * 1.15, 2.7 - Math.floor(i / COLS) * 1.45, 0];
  const REPO: V3 = [4.0, 5.2, 0];
  const LEARN: V3 = [1.0, -4.8, 0];
  const FL: V3 = [4.3, -4.1, 0];
  const DE: V3 = [4.3, -5.5, 0];
  const IDX: V3 = [-3.4, -4.8, 0];
  // Where a staged reflex stands beside the claim it decides (the verdict step).
  const FL_V: V3 = [2.6, CLAIM_Y[1]!, 0];
  const DE_V: V3 = [2.6, CLAIM_Y[0]!, 0];
  const rnd = seeded(5);
  const tracePos: V3[] = tasks.map(() => [LEARN[0] + rnd() * 0.55, LEARN[1] + rnd() * 0.55, rnd() * 0.55]);
  // Validation: rows for c1..c3, three arm groups of three sessions.
  const armX = (a: number, k: number) => -2.6 + a * 2.5 + k * 0.6;

  SKILL.forEach((c) => stage.dot(c.id, 0.26));
  tasks.forEach((_, i) => {
    stage.dot(`t${i}`, 0.15);
    stage.dot(`tr${i}`, 0.08);
  });
  stage.dot("repo", 0.3);
  stage.dot("learn", 0.34);
  stage.dot("fl", 0.28);
  stage.dot("de", 0.28);
  stage.dot("idx", 0.22);
  for (const a of acc) ARMS.forEach((arm, ai) => a.st[arm] && HELD_OUT[a.id]![arm].forEach((_, k) => stage.dot(`v${a.id}${ai}${k}`, 0.15)));

  // Links.
  const own: [string, string][] = [];
  tasks.forEach((t, i) => t.claim && own.push([t.claim.id, `t${i}`]));
  stage.link("own", own, "ink3");
  const box = (cx: number, cy: number, hw: number, hh: number): [V3, V3][] => {
    const a: V3 = [cx - hw, cy - hh, 0];
    const b: V3 = [cx + hw, cy - hh, 0];
    const c: V3 = [cx + hw, cy + hh, 0];
    const d: V3 = [cx - hw, cy + hh, 0];
    return [[a, b], [b, c], [c, d], [d, a]];
  };
  stage.link("pods", tasks.flatMap((_, i) => box(tPos(i)[0], tPos(i)[1], 0.5, 0.5)), "ink3");
  stage.link("clone", tasks.map((_, i) => ["repo", [tPos(i)[0], tPos(i)[1] + 0.5, 0]] as [string, V3]), "ink3");
  stage.link("flow", tasks.map((_, i) => [[tPos(i)[0], tPos(i)[1] - 0.5, 0], "learn"] as [V3, string]), "ink3");
  stage.link("stagedBox", box(6.4, (FL[1] + DE[1]) / 2, 2.6, 1.3), "accent");
  stage.link("idxBox", box(IDX[0], IDX[1], 1.3, 0.85), "ink3");
  stage.link("stageOut", [["learn", "fl"]], "accent");
  stage.link("toFl", [["c2", "fl"]], "accent");
  stage.link("toDe", [["c1", "de"]], "accent");

  // Labels.
  SKILL.forEach((c, i) => {
    const html = esc(c.sentence).replace(/`([^`]+)`/g, "<b>$1</b>");
    stage.label(`s${i}`, html, c.id, { cls: "chip big", dx: 18, dy: -14 });
    const what = c.kind === "regenerate_after" ? `regenerate_after(${esc(c.glob!)})` : c.kind;
    stage.label(`k${i}`, `<small>${c.id} · ${what}</small>${esc(c.command)}${c.unsafe ? ` <span class="how-dim">· unsafe</span>` : ""}`, c.id, {
      cls: "chip big",
      dx: 18,
      dy: -20,
    });
    stage.label(`q${i}`, `${c.id} <span class="how-dim">${c.kind === "regenerate_after" ? "regenerate" : c.kind}</span>`, c.id, { cls: "chip", dx: 16, dy: -10 });
    const v = verdicts[i]!;
    stage.label(`h${i}`, `${c.id} <span class="how-dim">${v.head}</span>`, c.id, { cls: "chip", dx: 16, dy: -10 });
    stage.label(`v${i}`, `<small>${c.id} · ${v.head}</small>${esc(v.detail)}`, c.id, { cls: "chip big", dx: 18, dy: -20 });
  });
  stage.label("stale", "your skill is stale: it says <b>npm test</b>, agents found <b>make check</b>", "c1", { cls: "note", dx: 18, dy: 25 });
  stage.label("grid", `${tasks.length} tasks · ${triggers} trigger · ${controls} control`, [tPos(2)[0] + 0.65, 3.9, 0], { cls: "chip", center: true, dy: -10 });
  stage.label("ctl", "controls", `t${tasks.length - 3}`, { cls: "note", dy: 16, center: true });
  stage.label("repo", "<small>YOUR REPOSITORY</small>HEAD · working tree untouched", "repo", { cls: "chip big", center: true, dy: -62 });
  stage.label("noorigin", "clones: git clone --local --no-hardlinks · origin removed", [tPos(0)[0] - 0.5, tPos(18)[1] - 0.9, 0], { cls: "note", dy: 4 });
  stage.label("learn", "<small>LEARNER</small>the same miners", "learn", { cls: "chip big", center: true, dy: 26 });
  stage.label("fl", `<small>PRIME · FORMING</small>${esc(flId)}`, "fl", { cls: "chip big", dx: 18, dy: -20 });
  stage.label("de", `<small>INHIBIT · FORMING</small>${esc(deadId)}`, "de", { cls: "chip big", dx: 18, dy: -20 });
  stage.label("staged", "staged/tracks · not activated", [3.8, FL[1] + 0.7, 0], { cls: "note", dy: -18 });
  stage.label("idx", "<small>INDEX.JSON</small>nothing installed", "idx", { cls: "chip big", center: true, dy: 20 });
  ARMS.forEach((arm, ai) =>
    stage.label(`arm${ai}`, arm, [armX(ai, 1), CLAIM_Y[0]! + 1.1, 0], { cls: "chip", center: true, dy: -10 }),
  );
  acc.forEach((a, r) =>
    stage.label(`acc${r}`, a.accepted ? "accepted" : "not accepted", `v${a.id}22`, { cls: a.accepted ? "note" : "", dx: 20, dy: -7 }),
  );

  // ------------------------------------------------------------------ states

  const kindColor = (c: Claim): DotLook["color"] => (c.unsafe ? "ink3" : { hue: HUE[c.kind] });
  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    const labels: string[] = [];
    const links: Record<string, number> = {};
    // Claims: a column on the left, in every step.
    SKILL.forEach((c, i) => {
      let look: DotLook = { pos: cPos(i), color: s === 0 ? "ink2" : kindColor(c), glow: 0.3 };
      if (s >= 2 && s <= 5) look = { ...look, alpha: c.unsafe || notExercisable(c) ? 0.35 : 0.85, glow: 0.15 };
      if (s === 6) {
        const v = verdicts[i]!.kind;
        look =
          v === "compiled"
            ? { pos: cPos(i), color: "accent", scale: 1.35, glow: 0.9 }
            : v === "contradicted"
              ? { pos: cPos(i), color: "ink1", scale: 1.35, glow: 0.9 }
              : v === "not"
                ? { pos: cPos(i), color: "ink2", glow: 0.2 }
                : { pos: cPos(i), color: "ink3", alpha: v === "unsafe" ? 0.35 : 0.6, glow: 0 };
      }
      if (s === 7) look = { pos: cPos(i), color: kindColor(c), alpha: i < 3 ? 1 : 0.2, glow: i < 3 ? 0.3 : 0 };
      // The pods step and the held-out step are about sessions: claims that play no part leave.
      if ((s === 3 || s === 4) && notExercisable(c)) return;
      if (s === 7 && i >= 3) return;
      dots[c.id] = look;
    });
    // On a phone the scene is narrow: short labels, and the caption says the rest.
    if (s <= 2) labels.push(...SKILL.map((_, i) => (narrow ? `q${i}` : s === 0 ? `s${i}` : `k${i}`)));
    if (s === 6) labels.push(...SKILL.map((_, i) => (narrow ? `h${i}` : `v${i}`)), ...(narrow ? [] : ["stale"]));
    if (s === 7) labels.push("q0", "q1", "q2");

    // Tasks: a grid of pods.
    if (s >= 2 && s <= 5) {
      tasks.forEach((t, i) => {
        const c = t.claim;
        const faded = s === 5;
        dots[`t${i}`] = {
          pos: tPos(i),
          color: c ? { hue: HUE[c.kind] } : "ink3",
          scale: t.explicit ? 1.35 : 1,
          alpha: faded ? 0.25 : c ? 1 : 0.7,
          glow: faded ? 0 : s === 3 ? 0.5 : 0.25,
        };
      });
      if (s < 5) labels.push("grid");
      if (s === 2) labels.push("ctl");
    }
    if (s === 2) links.own = 0.28;
    if (s >= 3 && s <= 4) links.pods = 0.5;
    if (s === 5) links.pods = 0.15;
    if (s === 3) {
      dots.repo = { pos: REPO, color: "ink1", glow: 0.5 };
      links.clone = 0.25;
      labels.push("repo", "noorigin");
    }
    // Traces leave the pods for the learner.
    if (s === 3) tasks.forEach((_, i) => (dots[`tr${i}`] = { pos: tPos(i), color: "ink2", alpha: 0 }));
    if (s === 4 || s === 5) {
      tasks.forEach((t, i) => {
        dots[`tr${i}`] = { pos: tracePos[i]!, color: t.claim ? { hue: HUE[t.claim.kind] } : "ink3", alpha: s === 5 ? 0.35 : 0.9, glow: 0.2 };
      });
      dots.learn = { pos: LEARN, color: "soft", glow: s === 4 ? 0.9 : 0.5, scale: 1 };
      links.flow = s === 4 ? 0.22 : 0.05;
      labels.push("learn");
    }
    // Staged reflexes: dimmed, never in the index.
    if (s === 5 || s === 6) {
      const dim = 0.5;
      const v = s === 6;
      if (flLearned) dots.fl = { pos: v ? FL_V : FL, color: flFires ? "fire" : "prime", alpha: dim, glow: 0.35 };
      if (deadLearned) dots.de = { pos: v ? DE_V : DE, color: "inhibit", alpha: dim, glow: 0.35 };
      if (!v) {
        links.stagedBox = 0.5;
        labels.push("fl", "de", "staged");
      }
      if (s === 5) {
        dots.idx = { pos: IDX, color: "ink3", alpha: 0.4, glow: 0 };
        links.idxBox = 0.4;
        links.stageOut = 0.5;
        labels.push("idx");
      }
      if (s === 6) {
        if (flLearned) links.toFl = 0.55;
        if (deadLearned) links.toDe = 0.55;
      }
    }
    // Validation: three arms per claim.
    if (s === 7) {
      acc.forEach((a, r) =>
        ARMS.forEach((arm, ai) =>
          HELD_OUT[a.id]![arm].forEach(([followed], k) => {
            dots[`v${a.id}${ai}${k}`] = {
              pos: [armX(ai, k), CLAIM_Y[r]!, 0],
              color: followed ? "accent" : "ink3",
              alpha: followed ? 1 : 0.55,
              scale: followed ? 1.3 : 1,
              glow: followed ? 0.6 : 0,
            };
          }),
        ),
      );
      labels.push("arm0", "arm1", "arm2", ...acc.map((_, r) => `acc${r}`));
    }
    const cam = s === 0 || s === 1 ? CAM.doc : s === 2 ? CAM.grid : s === 3 || s === 4 ? CAM.pods : s === 5 ? CAM.stage : s === 6 ? CAM.verdict : CAM.held;
    return { dots, links, labels, cam };
  }

  // ------------------------------------------------------------------ words

  const pct = (x: number) => `${Math.round(x * 100)}%`;
  const usd = (x: number) => `$${x.toFixed(2)}`;
  const text: Omit<Step, "state">[] = [
    {
      title: "A skill is a set of claims",
      body: `A skill file says what an agent should do here. This sample SKILL.md for the registry service makes ${SKILL.length} instructions. Adage treats each one as a hypothesis to test, not as a fact.`,
      chips: [{ text: "SKILL.md · CONTRIBUTING.md · runbook" }, { text: "Makefile · package.json · CI workflow" }, { text: "no model reads it: rules only" }],
    },
    {
      title: "Each claim gets a type",
      body: `Commands come from code spans in imperative sentences, and each is classed by effect. ${SKILL.filter((c) => c.unsafe).length} claim installs packages, which is above regenerate, so it is marked unsafe and never run.`,
      chips: [
        { text: "verify · regenerate_after(glob) · lookup · avoid(x→y) · environment" },
        { text: "unsafe: above regenerate, or not one plain command" },
      ],
    },
    {
      title: "Tasks are generated to test them",
      body: `The plan makes ${tasks.length} tasks for ${active.length} claims: ${triggers} give a claim its chance, and ${controls} are controls that touch nothing a claim covers. ${implicitN} of the ${triggers} never name the skill or the command. No file matches proto/*.proto, so that claim gets no tasks.`,
      chips: [
        { text: `${pct(implicitN / triggers)} implicit (about 70%)` },
        { text: `${controls}/${tasks.length} controls ≥ 25%`, ok: controls * 4 >= tasks.length },
        { text: "3 templates × 2 verbosities" },
        { text: `estimate ${tasks.length} × ${usd(perUsd)} = ${usd(estimate)}` },
      ],
      figure: (g, w, h, pal) => {
        // The estimate for this plan, by model family, in the cost colour.
        const rows = USD_PER_SESSION.map(([m, u]) => [m, u * tasks.length] as const);
        const max = Math.max(...rows.map(([, v]) => v));
        const rowH = (h - 4) / rows.length;
        g.font = "10px 'Red Hat Mono', ui-monospace, monospace";
        g.textBaseline = "middle";
        rows.forEach(([m, v], i) => {
          const y = 2 + i * rowH + rowH / 2;
          g.fillStyle = pal.css("ink3");
          g.textAlign = "left";
          g.fillText(m, 0, y);
          const x0 = 56;
          const bw = ((w - x0 - 60) * v) / max;
          g.fillStyle = pal.css("cost");
          g.globalAlpha = m === MODEL ? 1 : 0.45;
          g.fillRect(x0, y - rowH * 0.3, bw, rowH * 0.6);
          g.globalAlpha = 1;
          g.fillStyle = m === MODEL ? pal.css("ink1") : pal.css("ink3");
          g.fillText(`${usd(v)}${m === MODEL ? " · default" : ""}`, x0 + bw + 6, y);
        });
      },
    },
    {
      title: "Each task runs in a sealed clone",
      body: `With --execute, your own agent runs each task headlessly. Every session gets a fresh local clone with its origin removed, its own home and config directory, and a short tool list. Nothing can be pushed, and your working tree is never touched.`,
      chips: [
        { text: "tools: Bash Read Edit Write Glob Grep" },
        { text: "--permission-mode dontAsk · sandbox on" },
        { text: "environment allowlist" },
        { text: `timeout ${TIMEOUT_S.toLocaleString("en")} s · budget cap` },
      ],
    },
    {
      title: "The traces go to the same learner",
      body: `Exploration runs with the skill in the system prompt and Adage switched off, so no reflex steers it. The ${sessions.length} traces then go through the same miners as your own traces.`,
      chips: [{ text: "ADAGE_DISABLE=1" }, { text: "synthetic:skill:<sha256>@<run>" }, { text: `min support ${MIN_SUPPORT}` }],
    },
    {
      title: "Reflexes are staged, never activated",
      body: `The miners found ${[flLearned, deadLearned].filter(Boolean).length} reflexes. They are written to staged/tracks, capped at prime and forming, and never reach the index your hooks read. The finish line ${flFires ? "would fire, but is capped" : "primes"}: ${flSupport} of ${editors.length} sessions regenerated after the edit, a lower bound of ${f2(flLb)}.`,
      chips: [
        { text: "never fire · never firm" },
        { text: "a dead end stays inhibit" },
        { text: `finish line ${flSupport}/${editors.length} · LB ${f2(flLb)}` },
        { text: `dead end ${deadSupport} traces` },
      ],
    },
    {
      title: "Each claim gets a verdict",
      body: `The skill says npm test. It failed in ${failedTest.length} of the ${ranTest.length} sessions that ran it, and the agents found make check instead, so a dead end was learned: the claim is contradicted. The regenerate claim compiled. The lookup was followed only ${lkFollowed} times, too few to learn.`,
      chips: [
        { text: `stale: failed ≥ ${CONSISTENT[0]}/${CONSISTENT[1]} + a dead end`, ok: consistent && deadLearned },
        { text: `${vCount("compiled")} compiled · ${vCount("contradicted")} contradicted · ${vCount("never")} never exercised · ${vCount("not")} not compiled · ${vCount("unsafe")} unsafe` },
        { text: "unsafe > contradicted > compiled > never exercised > not compiled" },
      ],
    },
    {
      title: "Held-out tasks test the reflexes",
      body: `${heldOut} new tasks run in three arms: no skill, the skill in the prompt, and the staged reflexes through Adage's hooks with no control arm. A reflex is accepted when its arm follows it in at least 70% of sessions, within 10 points of the skill arm, and costs no more than 10% extra in turns or dollars.`,
      chips: [
        ...acc.map((a) => ({ text: `${a.id} reflex ${pct(a.st.reflex.rate)} · skill ${pct(a.st.skill.rate)}`, ok: a.accepted })),
        { text: `follow ≥ ${f2(FOLLOW_MIN)} · within ${f2(NON_INFERIORITY)} · ≤ +${pct(COST_TOLERANCE)}` },
        { text: "reflex arm: ADAGE_HOLDOUT=0" },
      ],
      figure: (g, w, h, pal) => {
        // Followed share per claim and arm, against the acceptance bar.
        const padB = 14;
        const top = 18;
        const Y = (v: number) => h - padB - v * (h - padB - top);
        const groupW = (w - 8) / acc.length;
        const bw = Math.min(22, groupW / 5);
        const armColor = ["ink3", "ink2", "accent"] as const;
        g.font = "10px 'Red Hat Mono', ui-monospace, monospace";
        acc.forEach((a, gi) => {
          const gx = 4 + gi * groupW + groupW / 2 - (bw * 3 + 8) / 2;
          ARMS.forEach((arm, ai) => {
            const v = a.st[arm].rate;
            g.fillStyle = pal.css(armColor[ai]!);
            g.fillRect(gx + ai * (bw + 4), Y(v), bw, Math.max(1, Y(0) - Y(v)));
          });
          g.fillStyle = pal.css("ink3");
          g.textAlign = "center";
          g.textBaseline = "top";
          g.fillText(a.id, gx + (bw * 3 + 8) / 2, h - padB + 3);
        });
        g.setLineDash([3, 3]);
        g.strokeStyle = pal.css("soft");
        g.beginPath();
        g.moveTo(4, Y(FOLLOW_MIN));
        g.lineTo(w - 4, Y(FOLLOW_MIN));
        g.stroke();
        g.setLineDash([]);
        g.fillStyle = pal.css("soft");
        g.textAlign = "right";
        g.textBaseline = "bottom";
        g.fillText(`follow ${f2(FOLLOW_MIN)}`, w - 4, Y(FOLLOW_MIN) - 2);
        // Which bar is which arm.
        g.textAlign = "left";
        g.textBaseline = "top";
        let lx = 4;
        ARMS.forEach((arm, ai) => {
          g.fillStyle = pal.css(armColor[ai]!);
          g.fillRect(lx, 2, 8, 8);
          g.fillStyle = pal.css("ink2");
          g.fillText(arm, lx + 12, 1);
          lx += 12 + g.measureText(arm).width + 12;
        });
      },
    },
  ];

  return {
    source: `a sample SKILL.md · ${tasks.length} sample sessions`,
    legend: [
      { label: "verify", role: "hue", hue: HUE.verify },
      { label: "regenerate_after", role: "hue", hue: HUE.regenerate_after },
      { label: "lookup", role: "hue", hue: HUE.lookup },
      { label: "unsafe / control", role: "ink3" },
      { label: "compiled · followed", role: "accent" },
    ],
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

export const train: Chapter = {
  id: "train",
  title: "Train on a skill",
  status: "Built",
  kind: "built",
  summary:
    "A SKILL.md, Makefile or CI file becomes typed claims. Your agent runs headlessly on generated tasks in disposable clones, the same learner stages what it did, and each claim gets a verdict.",
  source: "crates/adage-train · docs/training.md",
  usesRepo: false,
  build,
};

/** For tests: the numbers the captions state. */
export const _internal = { plan, explore, SKILL, notExercisable };
