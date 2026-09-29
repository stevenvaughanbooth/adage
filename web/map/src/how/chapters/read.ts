// Chapter: read traces (crates/adage-learn/src/{lib,import,codex,opencode,cache}.rs). A real
// Claude Code transcript streams in line by line, its tool calls gather into turns, results join
// their calls, commands split into steps, and each step gets a template key and an effect. Then
// the view widens to the sample traces the graph learner mines, one of them steered.

import type { CamSpec, DotLook, SceneState, Stage, V3 } from "../engine";
import { PROTOS, TRACES, laneX, laneZ, type Event } from "../sample";
import type { Built, Chapter, Step } from "../types";

// ---------------------------------------------------------------------------------------- data

// The recorded trace, record by record: evals/datasets/demo-haiku45/sessions/train-W-1041-r1.jsonl
// (Claude Haiku 4.5 on the demo repository). `P` is the typed prompt, `X` assistant text, `U` a
// tool_use block (its message id and call id, both shortened to their last six characters), `R` a
// tool_result (the call id it answers, and is_error).
type Rec = { t: "P" } | { t: "X"; m: string } | { t: "U"; m: string; id: string; tool: string } | { t: "R"; id: string; err: boolean };

const RECORDS: Rec[] = [
  { t: "P" },
  { t: "X", m: "6KuNeV" },
  { t: "U", m: "6KuNeV", id: "X7do71", tool: "Read" },
  { t: "R", id: "X7do71", err: true },
  { t: "U", m: "czdXtv", id: "EDMWaV", tool: "Bash" },
  { t: "R", id: "EDMWaV", err: false },
  { t: "U", m: "iwXKBj", id: "kAxxMv", tool: "Read" },
  { t: "R", id: "kAxxMv", err: false },
  { t: "U", m: "iwXKBj", id: "66c8xh", tool: "Bash" },
  { t: "R", id: "66c8xh", err: false },
  { t: "U", m: "7wJHcz", id: "N6szRk", tool: "Read" },
  { t: "R", id: "N6szRk", err: false },
  { t: "U", m: "ZVkzP5", id: "ue2MHT", tool: "Bash" },
  { t: "R", id: "ue2MHT", err: false },
  { t: "U", m: "AWAUs6", id: "UJVezU", tool: "Bash" },
  { t: "R", id: "UJVezU", err: true },
  { t: "U", m: "CiEQE4", id: "j2EpG7", tool: "Bash" },
  { t: "R", id: "j2EpG7", err: false },
  { t: "U", m: "KWVLt9", id: "Yct3PZ", tool: "Read" },
  { t: "R", id: "Yct3PZ", err: false },
  { t: "U", m: "aHXhJj", id: "LnZoTc", tool: "Bash" },
  { t: "R", id: "LnZoTc", err: false },
  { t: "U", m: "9pcKBA", id: "yzWUep", tool: "Read" },
  { t: "R", id: "yzWUep", err: false },
  { t: "U", m: "VErvqR", id: "FWEUcW", tool: "Read" },
  { t: "R", id: "FWEUcW", err: false },
  { t: "X", m: "gBGbiY" },
  { t: "U", m: "gBGbiY", id: "YTRPpS", tool: "Edit" },
  { t: "R", id: "YTRPpS", err: false },
  { t: "X", m: "J6MrX7" },
  { t: "U", m: "J6MrX7", id: "nYWtZq", tool: "Bash" },
  { t: "R", id: "nYWtZq", err: true },
  { t: "U", m: "RG5Nfm", id: "b7n5wm", tool: "Bash" },
  { t: "R", id: "b7n5wm", err: false },
  { t: "U", m: "fZ2Cy9", id: "H1FERp", tool: "Bash" },
  { t: "R", id: "H1FERp", err: false },
  { t: "U", m: "JtXXHZ", id: "WUE3d8", tool: "Bash" },
  { t: "R", id: "WUE3d8", err: false },
  { t: "X", m: "ruJwfK" },
  { t: "U", m: "ruJwfK", id: "o4AvwH", tool: "Bash" },
  { t: "R", id: "o4AvwH", err: false },
  { t: "X", m: "G999TY" },
];

const PROMPT = "Pick up W-1041 and get it done.";

type Effect = "observe" | "verify" | "regenerate" | "environment" | "author" | "publish";
/** Effects in the order the code ranks them (adage-learn evidence.rs `max_by_rank`). */
const EFFECTS: Effect[] = ["observe", "verify", "regenerate", "environment", "author", "publish"];
const EFFECT_HUE: Record<Effect, number> = { observe: 205, verify: 140, regenerate: 95, environment: 40, author: 262, publish: 320 };

// The steps the importer produced for each call, as `read_session` returns them for this file
// (template key, effect, failed), in order. A call with several entries was a compound command.
const STEPS: Record<string, [string, Effect, boolean][]> = {
  X7do71: [["read {path}", "observe", true]],
  EDMWaV: [["list {path}", "observe", false]],
  kAxxMv: [["read {path}", "observe", false]],
  "66c8xh": [["list {path}", "observe", false]],
  N6szRk: [["read {path}", "observe", false]],
  ue2MHT: [["search:filename {pattern}", "observe", false]],
  UJVezU: [["list {path}", "observe", true]],
  j2EpG7: [["search:filename {pattern}", "observe", false]],
  Yct3PZ: [["read {path}", "observe", false]],
  LnZoTc: [["exec python3 scripts/wi-lookup {id}", "observe", false]],
  yzWUep: [["read {path}", "observe", false]],
  FWEUcW: [["read {path}", "observe", false]],
  YTRPpS: [["edit {path}", "author", false]],
  nYWtZq: [
    ["exec python3 -m pytest {path} -v", "verify", true],
    ["exec python3 -m unittest tests.test_billing -v", "verify", true],
  ],
  b7n5wm: [["exec make test", "verify", false]],
  H1FERp: [["exec python3", "environment", false]],
  WUE3d8: [["exec make ci", "verify", false]],
  o4AvwH: [
    ["exec git add {path}", "author", false],
    ["exec cat", "observe", false],
    ["exec git commit -m $(cat <<'EOF' …", "publish", false],
  ],
};

// The commands of the two compound calls, as the agent typed them (shortened with …).
const COMMAND: Record<string, string> = {
  nYWtZq: "python3 -m pytest tests/test_billing.py -v 2>/dev/null || python3 -m unittest tests.test_billing -v",
  o4AvwH: "git add acme/billing.py && git commit -m \"$(cat <<'EOF' …\"",
};

// One more illustrative trace, steered: Adage told the agent to run make gen after the edit.
const STEERED: Event[] = [
  ["P", "add port Bergen"],
  ["E", "edit registry/ports.csv"],
  ["G", "make gen"],
  ["C", "make check"],
];

// Import limits and cache constants (import.rs, codex.rs, opencode.rs, cache.rs).
const MAX_PROMPT_KIB = 64; // MAX_PROMPT_BYTES = 64 * 1024
const CODEX_MAX_LINE_MIB = 4; // MAX_LINE = 4 << 20
const CODEX_MAX_CALLS = 4096;
const OPENCODE_SNIFF = 64; // bytes read to find "info"
const SESSION_FORMAT = 3;

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** A failed call's failure lands on its last sub-action only, unless the command has `||`
 * (adage-core keys.rs `sub_action_failed`). */
export function subActionFailed(index: number, count: number, callFailed: boolean, orChain: boolean): boolean {
  return callFailed && (orChain || index + 1 === count);
}

const CAM: Record<"stream" | "turns" | "split" | "ladder" | "lanes", CamSpec> = {
  stream: { target: [0, 2.4, 0], yaw: -0.06, pitch: 0.14, dist: 16.5 },
  turns: { target: [0, 0, 0], yaw: -0.08, pitch: 0.2, dist: 17 },
  split: { target: [4.9, -0.8, 0], yaw: -0.04, pitch: 0.1, dist: 12.5 },
  ladder: { target: [-0.4, 0.5, 0], yaw: 0.04, pitch: 0.1, dist: 18.5 },
  lanes: { target: [-0.6, -1, 0.6], yaw: -0.2, pitch: 0.8, dist: 25 },
};

// ---------------------------------------------------------------------------------------- build

interface Call {
  id: string;
  rec: number;
  turn: number;
  /** Its place among the calls of its turn. */
  k: number;
  err: boolean;
  steps: { key: string; effect: Effect; failed: boolean; id: string; n: number }[];
}

function build(stage: Stage): Built {
  // Turns: every assistant message that issued a tool call, in order (import.rs `turn_of_msg`).
  const turnOf = new Map<string, number>();
  const calls: Call[] = [];
  const perTurn = new Map<number, number>();
  RECORDS.forEach((r, i) => {
    if (r.t !== "U") return;
    if (!turnOf.has(r.m)) turnOf.set(r.m, turnOf.size);
    const turn = turnOf.get(r.m)!;
    const k = perTurn.get(turn) ?? 0;
    perTurn.set(turn, k + 1);
    calls.push({ id: r.id, rec: i, turn, k, err: false, steps: [] });
  });
  const resultOf = new Map<string, number>();
  RECORDS.forEach((r, i) => {
    if (r.t === "R") {
      resultOf.set(r.id, i);
      const c = calls.find((c) => c.id === r.id);
      if (c) c.err = r.err;
    }
  });
  let n = 0;
  for (const c of calls) {
    const parts = STEPS[c.id] ?? [];
    const orChain = (COMMAND[c.id] ?? "").includes("||");
    parts.forEach(([key, effect, failed], i) => {
      // The recorded flags must agree with the rule; the test checks it too.
      if (failed !== subActionFailed(i, parts.length, c.err, orChain)) throw new Error(`read: ${c.id} step ${i} breaks the failure rule`);
      c.steps.push({ key, effect, failed, id: `s${n}`, n: n++ });
    });
  }
  const steps = calls.flatMap((c) => c.steps.map((s) => ({ ...s, call: c })));
  const turns = turnOf.size;
  const multi = calls.filter((c) => c.k === 1).map((c) => c.turn);
  const twoCallTurn = multi[0]!;
  const failedCalls = calls.filter((c) => c.err);
  const texts = RECORDS.filter((r) => r.t === "X").length;
  const or = calls.find((c) => (COMMAND[c.id] ?? "").includes("||"))!;
  const and = calls.find((c) => c.id !== or.id && COMMAND[c.id])!;
  const lookup = steps.find((s) => s.key.includes("wi-lookup"))!;
  const byEffect = EFFECTS.map((e) => steps.filter((s) => s.effect === e).length);

  // Positions.
  const W = 14;
  const streamPos = (i: number): V3 => [-W / 2 + (i * W) / (RECORDS.length - 1), 2.6, 0];
  const colX = (t: number) => -6.5 + (t * 13) / (turns - 1);
  const usePos = (c: Call): V3 => [colX(c.turn), 0.1 + c.k * 0.62, 0];
  const resPos = (c: Call): V3 => [colX(c.turn), -0.75 - c.k * 0.62, 0];
  // Sub-steps hang below their call, in order.
  const splitPos = (c: Call, i: number): V3 => [colX(c.turn), 0.1 + c.k * 0.62 - (c.steps.length > 1 ? 0.3 + i * 0.85 : 0), 0];
  const rowY = (e: Effect) => -2.6 + EFFECTS.indexOf(e) * 1.05;
  const ladderPos = (s: (typeof steps)[number]): V3 => {
    const same = s.call.steps;
    const m = same.length;
    const i = same.findIndex((x) => x.id === s.id);
    const x = colX(s.call.turn) + (m > 1 ? (i - (m - 1) / 2) * 0.42 : 0) + s.call.k * 0.3;
    return [x, rowY(s.effect), 0];
  };
  const PROMPT_AT: V3 = [-7.7, 0.1, 0];
  const PROMPT_HIGH: V3 = [-6.6, 3.8, 0];

  // Dots.
  RECORDS.forEach((_, i) => stage.dot(`r${i}`, 0.15));
  for (const s of steps) stage.dot(s.id, 0.15);
  const rows = [...TRACES, STEERED];
  rows.forEach((tr, i) => tr.forEach((_, j) => stage.dot(`n${i}.${j}`)));

  // Links.
  stage.link(
    "join",
    calls.map((c) => [`r${c.rec}`, `r${resultOf.get(c.id)!}`] as [string, string]),
    "ink3",
  );
  stage.link(
    "seq",
    steps.slice(1).map((s, i) => [steps[i]!.id, s.id] as [string, string]),
    "ink3",
  );
  stage.link(
    "rows",
    EFFECTS.map((e) => [[-7.3, rowY(e), 0], [7, rowY(e), 0]] as [V3, V3]),
    "ink3",
  );
  const lanePairs: [string, string][] = [];
  rows.forEach((tr, i) => tr.forEach((_, j) => j && lanePairs.push([`n${i}.${j - 1}`, `n${i}.${j}`])));
  stage.link("lane", lanePairs, "ink3");
  stage.link(
    "steer",
    STEERED.slice(1).map((_, j) => [`n${TRACES.length}.${j}`, `n${TRACES.length}.${j + 1}`] as [string, string]),
    "accent",
  );

  // Labels.
  const lookupCall = calls.find((c) => c.id === lookup.call.id)!;
  stage.label("j0", `<small>LINE 1 · USER</small>"content": "${PROMPT}"`, "r0", { cls: "chip", dx: -14, dy: 22 });
  stage.label("j1", `<small>LINE ${lookupCall.rec + 1} · ASSISTANT</small>"tool_use", "name": "Bash"`, `r${lookupCall.rec}`, { cls: "chip", dy: -58, center: true });
  stage.label("j2", `<small>LINE ${resultOf.get(lookupCall.id)! + 1} · USER</small>"tool_result", "tool_use_id": "…${lookupCall.id}"`, `r${resultOf.get(lookupCall.id)!}`, {
    cls: "chip",
    dy: 20,
    center: true,
  });
  stage.label("jN", `${RECORDS.length} lines`, `r${RECORDS.length - 1}`, { dy: 16, center: true });
  stage.label("prompt", `<small>PROMPT · STEP 0</small>${PROMPT}`, "r0", { cls: "chip", dx: -12, dy: 22 });
  stage.label("tN", `turn ${turns}`, `r${calls[calls.length - 1]!.rec}`, { dy: 18, center: true });
  const two = calls.find((c) => c.turn === twoCallTurn && c.k === 1)!;
  stage.label("two", `turn ${twoCallTurn + 1}: two calls, one message.id`, `r${two.rec}`, { cls: "note", dx: 10, dy: -40 });
  for (const c of failedCalls) stage.label(`x${c.id}`, "✗ is_error", `r${resultOf.get(c.id)!}`, { cls: "note", dy: 12, center: true });
  stage.label("or", `<small>ONE CALL · ||</small>${esc(COMMAND[or.id]!).replace(" || ", "<br>|| ")}`, or.steps[0]!.id, { cls: "chip", dx: -130, dy: -76, center: true });
  stage.label("and", `<small>ONE CALL · &&</small>${esc(COMMAND[and.id]!).replace(" &amp;&amp; ", "<br>&amp;&amp; ")}`, and.steps[0]!.id, { cls: "chip", dx: 70, dy: -76, center: true });
  const short = (k: string) => esc(k.replace(/^exec /, "").replace(/ -m \$\(cat.*$/, " -m …"));
  // Left of the column, centred by an estimate of its width (11px mono is about 6.6px a character).
  or.steps.forEach((s) => {
    const t = `${short(s.key)} ✗`;
    stage.label(`sf${s.id}`, t, s.id, { cls: "note", dx: -16 - t.length * 3.3, dy: -7, center: true });
  });
  and.steps.forEach((s) => stage.label(`sk${s.id}`, `${short(s.key)} <span class="how-dim">${s.effect}</span>`, s.id, { cls: "note", dx: 14, dy: -7 }));
  EFFECTS.forEach((e) => stage.label(`e${e}`, e, [-7.3, rowY(e), 0], { dx: -8 - e.length * 3.4, dy: -7, center: true }));
  stage.label("lk", `<small>KEY · ${lookup.effect.toUpperCase()}</small>${lookup.key}`, lookup.id, { cls: "chip", dy: -58, center: true });
  stage.label("pl", PROMPT, PROMPT_HIGH, { cls: "note", dx: 12, dy: -7 });
  rows.forEach((_, i) => stage.label(`T${i}`, i < TRACES.length ? `T${i + 1}` : "T+", `n${i}.0`, { dx: -34, dy: -7 }));
  stage.label("sig", `<small>STEERED · NAMES make gen</small>&lt;adage-reflex strength="prime" kind="obligation"&gt;`, `n${TRACES.length}.${STEERED.length - 1}`, {
    cls: "chip",
    dx: 16,
    dy: -24,
  });

  // ------------------------------------------------------------------ states

  const recColor = (r: Rec): DotLook["color"] => (r.t === "P" ? "soft" : r.t === "U" ? "accent" : r.t === "R" ? "ink2" : "ink3");

  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    const labels: string[] = [];
    const links: Record<string, number> = {};
    let cam = CAM.stream;

    if (s <= 2) {
      RECORDS.forEach((r, i) => {
        const id = `r${i}`;
        let pos = streamPos(i);
        let alpha = r.t === "X" ? 0.55 : 1;
        let scale = r.t === "X" ? 0.7 : r.t === "P" ? 1.5 : 1;
        let glow = r.t === "P" ? 0.6 : 0.18;
        let color = recColor(r);
        if (s >= 1) {
          if (r.t === "X") return; // assistant text is not a step
          if (r.t === "P") pos = PROMPT_AT;
          if (r.t === "U") pos = usePos(calls.find((c) => c.id === r.id)!);
          if (r.t === "R") {
            const c = calls.find((c) => c.id === r.id)!;
            if (s === 1) alpha = 0.35;
            else {
              pos = resPos(c);
              if (r.err) {
                color = "ink1";
                scale = 1.5;
                glow = 0.6;
              } else alpha = 0.7;
            }
          }
        }
        dots[id] = { pos, color, alpha, scale, glow };
      });
      if (s === 2) for (const st of steps) dots[st.id] = { pos: usePos(st.call), color: "accent", alpha: 0, glow: 0 };
      if (s === 0) labels.push("j0", "j1", "j2", "jN");
      if (s === 1) labels.push("prompt", "tN", "two");
      if (s === 2) labels.push("prompt", ...failedCalls.map((c) => `x${c.id}`));
      links.join = s === 2 ? 0.7 : 0;
      cam = s === 0 ? CAM.stream : CAM.turns;
    } else if (s <= 4) {
      for (const st of steps) {
        const c = st.call;
        const i = c.steps.findIndex((x) => x.id === st.id);
        const compound = c.steps.length > 1;
        const pos = s === 3 ? splitPos(c, i) : ladderPos(st);
        const color: DotLook["color"] = s === 3 ? (st.failed ? "ink3" : "accent") : { hue: EFFECT_HUE[st.effect] };
        const focus = s === 3 ? compound : true;
        dots[st.id] = {
          pos,
          color,
          alpha: focus ? 1 : s === 3 ? 0.16 : 0.28,
          scale: s === 3 && compound ? 1.45 : st.id === lookup.id && s === 4 ? 1.5 : 1,
          glow: focus ? (s === 3 ? 0.6 : 0.3) : 0,
        };
      }
      // The prompt stays with its trace.
      dots.r0 = { pos: s === 3 ? PROMPT_AT : PROMPT_HIGH, color: "soft", scale: 1.5, glow: 0.5, alpha: s === 3 ? 0.4 : 1 };
      if (s === 3) labels.push("or", "and", ...or.steps.map((x) => `sf${x.id}`), ...and.steps.map((x) => `sk${x.id}`));
      if (s === 4) labels.push(...EFFECTS.map((e) => `e${e}`), "lk", "pl");
      links.seq = s === 4 ? 0.35 : 0;
      links.rows = s === 4 ? 0.14 : 0;
      cam = s === 3 ? CAM.split : CAM.ladder;
    } else {
      rows.forEach((tr, i) => {
        const steered = i === TRACES.length;
        if (steered && s !== 5) return;
        tr.forEach(([code], j) => {
          dots[`n${i}.${j}`] = {
            pos: [laneX(j), 0, laneZ(i)],
            color: steered ? "accent" : { hue: PROTOS[code].hue },
            alpha: steered ? 1 : s === 5 ? 0.45 : 0.85,
            scale: steered ? 1.3 : 1,
            glow: steered ? 0.8 : 0.14,
          };
        });
      });
      labels.push(...rows.map((_, i) => `T${i}`).filter((_, i) => s === 5 || i < TRACES.length));
      if (s === 5) labels.push("sig");
      links.lane = s === 5 ? 0.22 : 0.4;
      links.steer = s === 5 ? 0.95 : 0;
      cam = CAM.lanes;
    }
    return { dots, links, labels, cam };
  }

  const text: Omit<Step, "state">[] = [
    {
      title: "Transcript lines stream in",
      body: `This is a real Claude Code trace from the demo dataset: ${RECORDS.length} JSON lines. Adage reads it line by line and skips sub-agent (sidechain) records. Before that, it recognises the format by content: a Codex rollout, else an OpenCode export, else Claude Code JSONL.`,
      chips: [{ text: "Codex → OpenCode → Claude Code" }, { text: `OpenCode: "info" in the first ${OPENCODE_SNIFF} bytes` }, { text: "isSidechain skipped" }],
    },
    {
      title: "The prompt, then the turns",
      body: `A user string that is not meta or synthetic is a prompt; this one names ${PROMPT.match(/[A-Z][A-Z0-9]{0,9}-\d{1,7}/)?.[0] ?? "a ticket"}. Tool calls that share one message.id form one turn: ${calls.length} calls make ${turns} turns here. The ${texts} assistant text blocks are not steps.`,
      chips: [{ text: `prompt ≤ ${MAX_PROMPT_KIB} KiB` }, { text: "anchored to the step it arrived at" }, { text: `${turns} turns` }],
    },
    {
      title: "Results join their calls",
      body: `Each tool_result finds its call by tool_use_id, whatever line it sits on. is_error marks the call failed: ${failedCalls.length} of the ${calls.length} calls failed here, including a read of a directory and a missing tree command.`,
      chips: [{ text: "joined by tool_use_id" }, { text: `${failedCalls.length} failed` }],
    },
    {
      title: "Compound commands split",
      body: `A shell command is parsed, and a compound one becomes one step per sub-action: ${calls.length} calls become ${steps.length} steps. A failure is blamed on the last part only, unless the command used ||. Here the test command used ||, so both parts are marked failed.`,
      chips: [{ text: "failure → last part" }, { text: "|| → every part" }, { text: `${calls.length} calls → ${steps.length} steps` }],
    },
    {
      title: "Every step gets a key and an effect",
      body: `Each step gets a template key, with the specifics replaced (the ticket becomes {id}), and an effect, ranked from observe to publish. The lookup is ${lookup.key}, an observe step. Keys are what the learner counts.`,
      chips: EFFECTS.map((e, i) => ({ text: `${e} ${byEffect[i]}` })),
      figure: (g, w, h, pal) => {
        const padB = 16;
        const max = Math.max(...byEffect, 1);
        const bw = (w - 8) / EFFECTS.length;
        g.font = "10px 'Red Hat Mono', ui-monospace, monospace";
        g.textBaseline = "top";
        g.textAlign = "center";
        EFFECTS.forEach((e, i) => {
          const x = 4 + i * bw;
          const bh = (byEffect[i]! / max) * (h - padB - 16);
          g.fillStyle = `#${pal.hue(EFFECT_HUE[e]).getHexString()}`;
          g.fillRect(x + bw * 0.2, h - padB - bh, bw * 0.6, bh);
          g.fillStyle = pal.css("ink3");
          g.fillText(e === "regenerate" ? "regen" : e === "environment" ? "env" : e, x + bw / 2, h - padB + 3);
          g.fillStyle = pal.css("ink2");
          g.textBaseline = "bottom";
          g.fillText(String(byEffect[i]), x + bw / 2, h - padB - bh - 2);
          g.textBaseline = "top";
        });
        g.strokeStyle = pal.css("ink3");
        g.globalAlpha = 0.4;
        g.beginPath();
        g.moveTo(4, h - padB + 0.5);
        g.lineTo(w - 4, h - padB + 0.5);
        g.stroke();
        g.globalAlpha = 1;
      },
    },
    {
      title: "Steered traces are marked",
      body: "Where Adage already directed the agent, the trace carries an <adage-reflex> tag from the hook; only hook output counts, never text the agent wrote. What that directive named (here, make gen) is exposed from the point it arrived: it is not evidence for any reflex that directs it, so a reflex never learns from its own advice. The rest of the trace still counts.",
      chips: [{ text: "exposed from the directive on" }, { text: "only hook output steers" }, { text: "illustrative steered trace" }],
    },
    {
      title: "Read once, then cached",
      body: `Parsed traces are cached by path, size and modification time, and the cache is stamped with the build, so a new build starts it empty. Nothing is redacted here; that happens only when something leaves the machine. These ${TRACES.length} traces are what the graph learner reads next.`,
      chips: [
        { text: `session format ${SESSION_FORMAT}` },
        { text: "owner-only cache" },
        { text: `Codex: line ≤ ${CODEX_MAX_LINE_MIB} MiB, ≤ ${CODEX_MAX_CALLS.toLocaleString("en-US")} calls` },
      ],
    },
  ];

  return {
    source: `1 recorded trace · ${TRACES.length} sample traces`,
    legend: [
      { label: "prompt", role: "soft" },
      { label: "tool call", role: "accent" },
      { label: "tool result", role: "ink2" },
      { label: "failed result", role: "ink1" },
      { label: "assistant text", role: "ink3" },
    ],
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

export const readTraces: Chapter = {
  id: "read",
  title: "Read traces",
  status: "Built",
  kind: "built",
  summary:
    "Claude Code, Codex and OpenCode transcripts become one shape: prompts, then a flat list of steps, one per action. Every shell command is parsed and classed by effect.",
  source: "crates/adage-learn/src/{lib,import,codex,opencode,cache}.rs",
  usesRepo: false,
  build,
};

/** For tests: the recorded data the chapter replays. */
export const _data = { RECORDS, STEPS, COMMAND };
