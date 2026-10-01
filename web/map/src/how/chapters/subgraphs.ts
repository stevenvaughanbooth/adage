// Chapter: find repeated subgraphs (Track G, crates/adage-graph: the only learner). Events
// embed, cluster into prototype labels, traces become label strings, and a closed pattern grows
// around a target, is scored as a prediction and compiles to a reflex.

import type { CamSpec, DotLook, SceneState, Stage, V3 } from "../engine";
import { PROTOS, TRACES, at, betaGrid, f2, laneX, laneZ, seeded, type Code } from "../sample";
import type { Built, Chapter, Step } from "../types";

const CAM: Record<"lanes" | "cloud" | "compile", CamSpec> = {
  lanes: { target: [-0.6, -1, 0.6], yaw: -0.2, pitch: 0.8, dist: 25 },
  cloud: { target: [0.4, 0, 0], yaw: 0.42, pitch: 0.22, dist: 18 },
  compile: { target: [0, 1.8, 1], yaw: -0.12, pitch: 0.46, dist: 24 },
};

// Track G constants shown on screen (score.rs `ScoreParams::default`, params.rs fire bar).
const KAPPA = 2;
const FIRE_BAR = 0.8;

interface Node {
  id: string;
  i: number;
  j: number;
  code: Code;
  exact: string;
  lane: V3;
  cloud: V3;
  proto: V3;
}

function build(stage: Stage): Built {
  const T = TRACES;
  const rnd = seeded(7);
  const nodes: Node[] = [];
  T.forEach((tr, i) =>
    tr.forEach(([code, exact], j) => {
      const p = PROTOS[code];
      const spread = p.single ? 0 : 0.85;
      const variant = exact.length % 3; // exact variants sit apart inside their cluster
      const cloud: V3 = [p.c[0] + rnd() * spread + variant * 0.25, p.c[1] + rnd() * spread, p.c[2] + rnd() * spread];
      const proto: V3 = p.single ? cloud : [p.c[0] + (cloud[0] - p.c[0]) * 0.14, p.c[1] + (cloud[1] - p.c[1]) * 0.14, p.c[2] + (cloud[2] - p.c[2]) * 0.14];
      const id = `n${i}.${j}`;
      nodes.push({ id, i, j, code, exact, lane: [laneX(j), 0, laneZ(i)], cloud, proto });
      stage.dot(id);
    }),
  );
  const byTrace = T.map((_, i) => nodes.filter((n) => n.i === i));

  // The counts every caption states, computed from the traces.
  const withGen = (tr: (typeof T)[number]) => {
    const e = at(tr, "E");
    const g = e >= 0 ? at(tr, "G", e) : -1;
    return e >= 0 && g > e && g - e <= 8;
  };
  const opp = T.filter((tr) => at(tr, "E") >= 0).length;
  const supp = T.filter(withGen).length;
  const without = T.filter((tr) => at(tr, "E") < 0);
  const suppWithout = without.filter((tr) => at(tr, "G") >= 0).length;
  const anyEdit = T.filter((tr) => at(tr, "E") >= 0 || at(tr, "S") >= 0);
  const parentSupp = anyEdit.filter((tr) => at(tr, "G") >= 0).length;
  const fwd = T.filter((tr) => withGen(tr) && at(tr, "C", at(tr, "G")) > at(tr, "G")).length;
  const m = parentSupp / anyEdit.length;
  const post = betaGrid(supp + KAPPA * m, opp - supp + KAPPA * (1 - m));
  const parent = betaGrid(parentSupp + 1, anyEdit.length - parentSupp + 1);
  const contrast = supp / opp - suppWithout / Math.max(1, without.length);

  // What the pattern matched.
  const match = { G: new Set<string>(), E: new Set<string>(), C: new Set<string>(), miss: new Set<string>() };
  const eg: [string, string][] = [];
  const gc: [string, string][] = [];
  T.forEach((tr, i) => {
    const e = at(tr, "E");
    if (e < 0) return;
    const row = byTrace[i]!;
    if (!withGen(tr)) {
      match.miss.add(row[e]!.id);
      return;
    }
    const g = at(tr, "G", e);
    match.E.add(row[e]!.id);
    match.G.add(row[g]!.id);
    eg.push([row[e]!.id, row[g]!.id]);
    const c = at(tr, "C", g);
    if (c > g) {
      match.C.add(row[c]!.id);
      gc.push([row[g]!.id, row[c]!.id]);
    }
  });

  // Links.
  const lanePairs: [string, string][] = [];
  byTrace.forEach((row) => row.forEach((n, k) => k && lanePairs.push([row[k - 1]!.id, n.id])));
  stage.link("lane", lanePairs, "ink3");
  const knn: [string, string][] = [];
  const must: [string, string][] = [];
  const dist = (a: V3, b: V3) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]);
  for (const a of nodes) {
    const near = nodes
      .filter((b) => b !== a && b.code === a.code)
      .sort((x, y) => dist(x.cloud, a.cloud) - dist(y.cloud, a.cloud))
      .slice(0, 3);
    for (const b of near) {
      const optionsOnly = a.exact !== b.exact && (a.code === "C" || a.code === "G") && /-j4|-B/.test(a.exact + b.exact);
      (optionsOnly ? must : knn).push([a.id, b.id]);
    }
  }
  stage.link("knn", knn, "ink3");
  stage.link("must", must, "accent");
  stage.link("eg", eg, "accent");
  stage.link("gc", gc, "accent");

  // The compiled reflex: stimulus → reflex → response, above the lanes.
  const RX: V3[] = [
    [-5.6, 5.2, 1],
    [0, 5.2, 1],
    [5.6, 5.2, 1],
  ];
  stage.dot("rx0", 0.3);
  stage.dot("rx1", 0.42);
  stage.dot("rx2", 0.3);
  stage.link("rx", [
    ["rx0", "rx1"],
    ["rx1", "rx2"],
  ], "accent");

  // Labels.
  byTrace.forEach((row, i) => {
    stage.label(`t${i}`, `T${i + 1}`, row[0]!.id, { dx: -34, dy: -7 });
    stage.label(`s${i}`, row.map((n) => n.code).join(" "), row[row.length - 1]!.id, { dx: 16, dy: -7 });
  });
  for (const [code, p] of Object.entries(PROTOS)) {
    const html = p.single ? `${p.label}<br><span class="how-dim">singleton · keeps its exact label</span>` : p.label;
    stage.label(`p${code}`, html, p.c, { cls: "chip", dy: -30, center: true });
  }
  const mustAnchor = nodes.find((n) => n.exact === "make check -j4")!;
  stage.label("must", "must-link: make check ≈ make check -j4", mustAnchor.id, { cls: "note", dx: 12, dy: 10 });
  stage.label("miss", "edited, no make gen", [...match.miss][0]!, { cls: "note", dx: 12, dy: 6 });
  const sNode = nodes.find((n) => n.code === "S")!;
  stage.label("coarse", `coarser: any edit → make gen · same ${supp} traces · dropped`, sNode.id, { cls: "note", dx: 12, dy: 6 });
  stage.label("rx0", `<small>AFTER EDIT</small>registry/*.csv`, RX[0]!, { cls: "chip big", dy: -52, center: true });
  stage.label("rx1", `<small>REFLEX · SEQUENCE</small><span class="how-lamp prime">●</span> prime · provisional`, RX[1]!, { cls: "chip big", dy: -64, center: true });
  stage.label("rx2", `<small>RUN</small>make gen → make check`, RX[2]!, { cls: "chip big", dy: -52, center: true });

  // Per-step scene states.
  const laneLabels = byTrace.map((_, i) => `t${i}`);
  function state(s: number): SceneState {
    const dots: Record<string, DotLook> = {};
    for (const n of nodes) {
      const p = PROTOS[n.code];
      const kindRole = { prompt: "soft", observe: "ink2", edit: "accent", regen: "ink1", verify: "ink1", fail: "ink3" } as const;
      let color: DotLook["color"] = s >= 3 ? { hue: p.hue } : kindRole[p.kind];
      let pos = n.lane;
      let alpha = 1;
      let scale = 1;
      let glow = 0.18;
      if (s >= 1 && s <= 3) pos = s === 3 ? n.proto : n.cloud;
      if (s === 3 && p.single) {
        glow = 0.5;
        scale = 1.3;
      }
      if (s >= 5) {
        const hit = match.G.has(n.id) || (s >= 6 && match.E.has(n.id)) || (s >= 7 && match.C.has(n.id));
        if (hit) {
          color = "soft";
          scale = 1.5;
          glow = 0.9;
        } else if (s >= 6 && match.miss.has(n.id)) {
          color = "ink2";
          scale = 1.25;
          glow = 0.4;
        } else if (s === 8 && n.code === "S") {
          color = "ink2";
          scale = 1.25;
          glow = 0.35;
        } else {
          alpha = 0.3;
          glow = 0;
        }
        if (s === 10) {
          alpha *= hit ? 0.55 : 0.4;
          glow *= 0.3;
        }
      }
      dots[n.id] = { pos, color, alpha, scale, glow };
    }
    if (s === 10) {
      dots.rx0 = { pos: RX[0]!, color: "soft", glow: 0.4 };
      dots.rx1 = { pos: RX[1]!, color: "prime", glow: 0.9 };
      dots.rx2 = { pos: RX[2]!, color: "soft", glow: 0.4 };
    }
    const labels: string[] = [];
    if (s === 0 || (s >= 4 && s <= 9)) labels.push(...laneLabels);
    if (s === 4) labels.push(...byTrace.map((_, i) => `s${i}`));
    if (s === 3) labels.push(...Object.keys(PROTOS).map((c) => `p${c}`));
    if (s === 2) labels.push("must");
    if (s >= 6 && s <= 9) labels.push("miss");
    if (s === 8) labels.push("coarse");
    if (s === 10) labels.push("rx0", "rx1", "rx2");
    const cam = s >= 1 && s <= 3 ? CAM.cloud : s === 10 ? CAM.compile : CAM.lanes;
    return {
      dots,
      labels,
      cam,
      links: {
        lane: s === 0 ? 0.55 : s >= 4 && s <= 9 ? 0.3 : s === 10 ? 0.08 : 0,
        knn: s === 2 ? 0.5 : s === 3 ? 0.18 : 0,
        must: s === 2 ? 0.95 : s === 3 ? 0.4 : 0,
        eg: s >= 6 && s <= 9 ? 0.9 : s === 10 ? 0.25 : 0,
        gc: s >= 7 && s <= 9 ? 0.9 : s === 10 ? 0.25 : 0,
        rx: s === 10 ? 0.9 : 0,
      },
    };
  }

  const text: Omit<Step, "state">[] = [
    {
      title: "Past sessions, side by side",
      plain: `Here are ${T.length} past sessions, one per row. Each dot is one thing the agent did: read a file, made an edit, ran a command. Adage looks for what keeps repeating.`,
      key: true,
      body: `Each row is one of ${T.length} sample agent traces; each dot is an event: a prompt, a file read, an edit, a command. Order is kept, and so is which events came in the same tool batch. This is how Adage conditions: the graph engine is its only learner, with no list of reflex kinds written by hand.`,
      chips: [{ text: "≤ 2,000 events per trace" }, { text: "edges: follows · reads · writes · binds" }],
    },
    {
      title: "Similar actions land close together",
      plain: "Each action is placed on a map of meaning, so similar commands and files land close together even when they are not written exactly the same.",
      math: "graph",
      body: "Events leave their rows and take positions in embedding space, so similar commands and files land near each other. A command's vector mixes a hash of its exact form with the meaning of its words.",
      chips: [{ text: "act 384-d = 256 hashed + 128 potion" }, { text: "weights 0.75 / 0.25" }, { text: "quantised to i16" }],
    },
    {
      title: "Look-alikes link up",
      plain: "Each action connects to the ones most like it. A check is never lumped in with an edit, and commands that differ only in their options count as the same.",
      math: "graph",
      body: "Each event links to its nearest neighbours, but only inside its own effect class and program, so a check never merges with an edit. Commands that differ only by options are tied together.",
      chips: [{ text: "exact kNN, k = 15" }, { text: "τ act 0.90 · art 0.85 · prompt 0.80" }, { text: "must-link: options only" }],
    },
    {
      title: "Each group gets one name",
      plain: "Each tight group of similar actions gets one shared name. A one-off action keeps its own name instead of being forced into a group.",
      math: "graph",
      body: "Louvain turns each dense group into one label. An event with no close group, like the one-off curl, keeps its exact text instead of being forced into a neighbour.",
      chips: [{ text: "Louvain, resolution 1.0" }, { text: "ambiguity margin δ 0.02" }, { text: "τ calibrated on 0.80–0.95" }],
    },
    {
      title: "Sessions read like sentences",
      plain: "Now each session reads like a sentence made of those names, and finding habits becomes finding phrases that keep repeating.",
      math: "graph",
      body: "With every event relabelled, each trace reads as a string. Events in one batch are sorted by label, so the same work always spells the same string, and finding repeated subgraphs becomes finding repeated strings.",
      chips: [{ text: "no graph isomorphism test" }, { text: "labels: exact ⊂ proto ⊂ coarse ⊂ head ⊂ kind" }],
    },
    {
      title: "Start from a step worth suggesting",
      plain: `Adage starts from a step worth recommending, here make gen, which ${supp} of the ${T.length} sessions ran, and asks what usually comes right before it.`,
      body: `Mining starts from a step worth delivering and asks what predicts it. Here the target is make gen, which appears in ${supp} of the ${T.length} traces.`,
      chips: [{ text: "targets: deliverable steps" }, { text: `make gen · ${supp} traces` }],
    },
    {
      title: "What comes just before it?",
      plain: `Whenever the last edit touched a file in registry/, make gen followed in ${supp} of ${opp} sessions. Without that edit it ran ${suppWithout} times. That edit is the trigger.`,
      key: true,
      body: `The pattern grows backward one item at a time. When the last edit touched registry/*.csv, make gen followed in ${supp} of ${opp} traces. In the ${without.length} traces without that edit it ran ${suppWithout} times.`,
      chips: [{ text: "≤ 3 context items" }, { text: "within 8 events of one prompt" }, { text: `${supp}/${opp} with · ${suppWithout}/${without.length} without` }],
    },
    {
      title: "What comes just after?",
      plain: `Then it looks at what comes after: make check followed make gen in ${fwd} of those ${supp} sessions. The habit is now edit, make gen, make check.`,
      body: `Then forward, up to two items: after make gen, make check followed in ${fwd} of those ${supp} traces. The pattern is now edit, make gen, make check.`,
      chips: [{ text: "2–6 items in all" }, { text: "beam 8 · budget 2,000,000 nodes" }, { text: `${fwd} traces` }],
    },
    {
      title: "The specific habit wins",
      plain: "A vaguer version, any edit followed by make gen, covers the same sessions but says less, so the more specific habit wins.",
      body: `A coarser version, any edit followed by make gen, matches the same ${supp} traces. It says less and explains nothing more, so it is dropped and the specific one stays.`,
      chips: [{ text: "BIDE-style closure" }, { text: "same traces + same context size → drop" }],
    },
    {
      title: "Score it cautiously",
      plain: `${supp} out of ${opp} is promising, but it is a small sample, so Adage scores it cautiously: ${f2(post.q05)}. Enough to keep, not yet enough to insist.`,
      math: "graph",
      body: `Confidence is ${supp}/${opp} = ${f2(supp / opp)}. With so few traces it is shrunk toward its parent (any edit → make gen, ${parentSupp}/${anyEdit.length}) by a Beta posterior, and judged by its 5% quantile: ${f2(post.q05)}.`,
      chips: [
        { text: `support ${supp} ≥ 5`, ok: supp >= 5 },
        { text: `contrast ${f2(contrast)} ≥ 0.30`, ok: contrast >= 0.3 },
        { text: `lower bound ${f2(post.q05)} ≥ 0.50`, ok: post.q05 >= 0.5 },
        { text: `κ ${KAPPA}, parent m ${f2(m)}` },
      ],
      figure: (g, w, h, pal) => {
        const padB = 16;
        const X = (x: number) => 4 + x * (w - 8);
        const maxP = Math.max(...post.ps, ...parent.ps);
        const Y = (p: number) => h - padB - (p / maxP) * (h - padB - 8);
        g.strokeStyle = pal.css("ink3");
        g.globalAlpha = 0.4;
        g.beginPath();
        g.moveTo(4, h - padB + 0.5);
        g.lineTo(w - 4, h - padB + 0.5);
        g.stroke();
        g.globalAlpha = 1;
        g.font = "10px 'Red Hat Mono', ui-monospace, monospace";
        g.fillStyle = pal.css("ink3");
        g.textBaseline = "top";
        for (const x of [0, 0.5, 1]) {
          g.textAlign = x === 0 ? "left" : x === 1 ? "right" : "center";
          g.fillText(String(x), X(x), h - padB + 3);
        }
        const curve = (grid: typeof post, color: string, fill: boolean) => {
          g.beginPath();
          g.moveTo(X(grid.xs[0]!), Y(0));
          grid.xs.forEach((x, i) => g.lineTo(X(x), Y(grid.ps[i]!)));
          g.lineTo(X(1), Y(0));
          if (fill) {
            g.fillStyle = color + "33";
            g.fill();
          }
          g.strokeStyle = color;
          g.lineWidth = 1.5;
          g.stroke();
        };
        curve(parent, pal.css("ink3"), false);
        curve(post, pal.css("accent"), true);
        const dash = (x: number, color: string, label: string, align: CanvasTextAlign) => {
          g.setLineDash([3, 3]);
          g.strokeStyle = color;
          g.beginPath();
          g.moveTo(X(x), 6);
          g.lineTo(X(x), h - padB);
          g.stroke();
          g.setLineDash([]);
          g.fillStyle = color;
          g.textAlign = align;
          g.fillText(label, X(x) + (align === "left" ? 4 : -4), 4);
        };
        dash(post.q05, pal.css("soft"), `5% · ${f2(post.q05)}`, "right");
        dash(FIRE_BAR, pal.css("fire"), "fire bar", "left");
      },
    },
    {
      title: "The habit becomes a reflex",
      plain: "The habit becomes a reflex: after editing registry files, run make gen, then make check. It starts as a gentle suggestion until more evidence comes in.",
      math: "graph",
      key: true,
      body: "The pattern compiles to a sequence reflex: after an edit to registry/*.csv, run make gen, then make check. A sequence starts provisional, so it primes until its evidence is counted again on its own trigger; to fire, a reflex needs its lower bound at the 0.80 fire bar. This is the reflex the Map tab then shows.",
      chips: [{ text: "shape: sequence" }, { text: "shapes: run · avoid · read-batch · obligation · sequence · advise · card" }, { text: "then MDL selection, ≤ 256 reflexes" }],
    },
  ];

  return {
    source: `${T.length} sample traces`,
    legend: [
      { label: "prompt", role: "soft" },
      { label: "read / status", role: "ink2" },
      { label: "edit", role: "accent" },
      { label: "make gen / check", role: "ink1" },
      { label: "failed", role: "ink3" },
    ],
    steps: text.map((t, i) => ({ ...t, state: state(i) })),
  };
}

export const subgraphs: Chapter = {
  id: "subgraphs",
  title: "Spot the patterns",
  technical: "Find repeated subgraphs",
  status: "Built · the learner",
  kind: "built",
  summary: "Adage lines up past sessions and looks for runs of actions that keep repeating, like an edit that is always followed by the same command.",
  source: "crates/adage-graph/src/{events,embed,proto,mine} · crates/adage-learn/src/bridge.rs",
  usesRepo: false,
  build,
};
