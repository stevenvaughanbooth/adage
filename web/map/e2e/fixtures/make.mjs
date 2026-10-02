#!/usr/bin/env node
// Writes the map's test fixtures (busy.json, empty.json, health.json) in the shape
// `adage map` serves. e2e/map.spec.ts serves them in place of the demo's own data where a view
// needs something the demo never forms (a quiet reflex, a procedure card, savings, rewrites). Deterministic: a fixed clock and a seeded generator, so screenshots are
// stable. The numbers are illustrative, modelled on the approved mockups. `node e2e/fixtures/make.mjs`

import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
export const NOW = Date.UTC(2026, 8, 28, 12, 0, 0);
const DAY = 86400e3;
const START = NOW - 14 * DAY;

let seed = 7;
const rnd = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const pick = (xs) => xs[Math.floor(rnd() * xs.length)];

const REPO = "acme-billing";
const wrap = (id, strength, body) =>
  `<adage-reflex track="acme-billing/learned@0.3.0" route="${id}" strength="${strength}">\n${body}\nLearned by Adage from this repository's earlier agent sessions; not requested by the user.\n</adage-reflex>`;

function route(o) {
  const arms = (t, c) => ({
    treated: { sessions: 43, applied: t[0], followed: t[1], met: t[2] ?? 0, hit_avoided: t[3] ?? 0 },
    control: { sessions: 11, applied: c[0], followed: c[1], met: c[2] ?? 0, hit_avoided: c[3] ?? 0 },
  });
  return {
    id: o.id,
    summary: o.summary,
    track: o.track,
    version: o.version,
    reference: o.reference ?? o.track,
    repo: REPO,
    imported: o.imported ?? false,
    in_index: o.state !== "nascent",
    execution_only: o.execution_only ?? false,
    instinct: o.instinct ?? false,
    origin: o.origin ?? null,
    review: { verbs: o.verbs, reasons: o.reasons ?? [] },
    strength: o.strength,
    ceiling: o.ceiling ?? "fire",
    effect: o.effect ?? "observe",
    state: o.state,
    authored: false,
    trigger: o.trigger,
    step: o.step,
    directive: o.directive,
    evidence: {
      support: o.ev[0],
      opportunities: o.ev[1],
      rate: Math.round((o.ev[0] / o.ev[1]) * 1e4) / 1e4,
      lower_bound: o.ev[2],
      turns_saved: o.ev[3],
      days: 9,
      exposed: 0,
      dormant_since: null,
      provenance: o.instinct ? "instinct" : "desire-path",
    },
    live: {
      emitted: o.live?.[0] ?? 0,
      would_have: o.live?.[1] ?? 0,
      followed: o.live?.[2] ?? 0,
      ignored: o.live?.[3] ?? 0,
      misfired: 0,
      obligation_met: o.live?.[4] ?? 0,
      control: { unaided: o.live?.[5] ?? 0, not_reached: o.live?.[6] ?? 0, obligation_met: 0 },
      sessions: o.live?.[7] ?? 0,
    },
    arms: arms(o.t ?? [0, 0], o.c ?? [0, 0]),
    shape: o.shape ?? (o.step.kind === "avoid" ? "avoid" : o.step.kind === "read_batch" ? "read-batch" : o.step.kind),
    quiet: o.quiet ?? false,
    obligation: o.obligation ?? null,
    ...(o.extra ?? {}),
  };
}

const routes = [
  route({
    id: "lookup-work-item",
    summary: "work items are looked up with scripts/wi-lookup",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "fire",
    state: "firm",
    trigger: { kind: "prompt", label: "a prompt names a work item" },
    step: { kind: "run", label: "python3 scripts/wi-lookup {item}" },
    directive: wrap("lookup-work-item", "fire", "In this repository, work item details are looked up with:\n  python3 scripts/wi-lookup W-2045"),
    ev: [23, 33, 0.53, 8],
    t: [18, 15],
    c: [9, 3],
    live: [31, 9, 26, 4, 0, 3, 6, 18],
    verbs: ["approve", "extinguish", "weaken", "pin"],
  }),
  route({
    id: "read-linked-files",
    summary: "files a lookup lists are read together",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "fire",
    state: "firm",
    trigger: { kind: "after_command", label: "`python3 scripts/wi-lookup {item}` succeeds" },
    step: { kind: "read_batch", label: "read ≤5 listed files at once", slot: "files" },
    directive: wrap("read-linked-files", "fire", "That result lists these files: src/billing/invoice.py, docs/billing.md.\nIf you need them, read them together in one turn rather than one at a time."),
    ev: [15, 16, 0.72, 1.9],
    t: [14, 12],
    c: [6, 2],
    live: [22, 7, 19, 2, 0, 2, 4, 14],
    verbs: ["approve", "extinguish", "weaken", "pin"],
  }),
  route({
    id: "dead-end-d41b1e",
    summary: "unittest cannot run here; make test works",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "inhibit",
    ceiling: "inhibit",
    state: "firm",
    trigger: { kind: "after_failure", label: "`python3 -m unittest` fails: No module named pytest" },
    step: { kind: "avoid", label: "make test", avoid: "python3 -m unittest", signature: "No module named pytest" },
    directive: wrap("dead-end-d41b1e", "inhibit", "In this repository `python3 -m unittest` fails (No module named pytest). Run instead:\n  make test"),
    ev: [11, 43, 0.61, 2.4],
    t: [12, 10, 0, 1],
    c: [5, 1, 0, 4],
    live: [14, 5, 10, 2, 0, 1, 4, 12],
    verbs: ["approve", "extinguish", "pin"],
  }),
  route({
    id: "start-with-make-test",
    summary: "“run the tests” means make test",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "prime",
    state: "forming",
    trigger: { kind: "intent", label: "a prompt means: run the tests" },
    step: { kind: "run", label: "make test" },
    directive: wrap("start-with-make-test", "prime", "Agents in this repository usually start the tests with:\n  make test"),
    ev: [6, 9, 0.41, 0.4],
    t: [7, 5],
    c: [3, 1],
    live: [8, 3, 5, 3, 0, 1, 2, 7],
    verbs: ["approve", "extinguish", "strengthen", "pin"],
  }),
  route({
    id: "finish-line/regenerate",
    summary: "edits to schema/*.json must be followed by make gen",
    track: "adage/finish-line",
    reference: "acme-billing/instincts",
    version: "0.1.0",
    strength: "prime",
    ceiling: "prime",
    effect: "regenerate",
    state: "firm",
    instinct: true,
    origin: "instinct U1 (from Makefile)",
    trigger: { kind: "after_edit", label: "an edit touches schema/*.json" },
    step: { kind: "run", label: "make gen" },
    directive: wrap("finish-line/regenerate", "prime", "In this repository, changes to files matching schema/*.json must be followed by running:\n  make gen"),
    ev: [7, 7, 0.65, 0.6],
    t: [10, 0, 9],
    c: [4, 0, 2],
    live: [12, 4, 0, 1, 9, 0, 0, 10],
    verbs: ["approve", "extinguish", "pin"],
  }),
  route({
    id: "regen-openapi-client",
    summary: "openapi edits need make client",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "prime",
    state: "nascent",
    effect: "regenerate",
    trigger: { kind: "after_edit", label: "an edit touches api/openapi.yaml" },
    step: { kind: "run", label: "make client" },
    directive: wrap("regen-openapi-client", "prime", "In this repository, changes to api/openapi.yaml are followed by:\n  make client"),
    ev: [3, 4, 0.3, 0.9],
    verbs: ["approve", "extinguish", "pin"],
    reasons: [{ kind: "nascent", why: "an environment step, nascent until a person approves it" }],
  }),
  route({
    id: "read-adr-before-schema",
    summary: "schema changes start from ADR 0007",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "prime",
    state: "nascent",
    trigger: { kind: "after_edit", label: "an edit touches schema/*.json" },
    step: { kind: "advise", label: "read docs/adr/0007-schema.md first", template: "read_first" },
    directive: wrap("read-adr-before-schema", "prime", "Before changing the schema, agents here read:\n  docs/adr/0007-schema.md"),
    ev: [3, 5, 0.23, 0.5],
    verbs: ["approve", "extinguish", "pin"],
    reasons: [{ kind: "new", why: "learned by `adage condition` and never reviewed" }],
  }),
  route({
    id: "prefer-uv-pip",
    summary: "pip is externally managed here; uv works",
    track: "team/python-basics",
    reference: "@imported/team/python-basics",
    version: "1.2.0",
    imported: true,
    strength: "prime",
    ceiling: "inhibit",
    state: "firm",
    trigger: { kind: "after_failure", label: "`pip install` fails: externally-managed-environment" },
    step: { kind: "avoid", label: "uv pip install", avoid: "pip install", signature: "externally-managed-environment" },
    directive: wrap("prefer-uv-pip", "prime", "Here `pip install` fails (externally-managed-environment). Consider instead:\n  uv pip install"),
    ev: [4, 6, 0.3, 0.7],
    t: [3, 2, 0, 0],
    c: [2, 0, 0, 1],
    live: [3, 2, 2, 1, 0, 0, 1, 3],
    verbs: ["approve", "extinguish", "pin"],
  }),
  route({
    id: "lint-after-edit",
    summary: "agents already run make lint after an edit",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "prime",
    state: "nascent",
    quiet: true,
    effect: "verify",
    trigger: { kind: "after_edit", label: "an edit touches src/*.py" },
    step: { kind: "run", label: "make lint" },
    directive: wrap("lint-after-edit", "prime", "After an edit here, agents run:\n  make lint"),
    ev: [9, 10, 0.62, 0.2],
    verbs: ["approve", "extinguish", "pin"],
    reasons: [
      {
        kind: "quiet",
        why: "uplift +0.04 (lower bound -0.03): followed 0.95 when told, 0.91 unaided; 9 of 10 traces did it unaided. Quiet: agents already do this, so delivering it changes little; `approve` delivers it anyway",
      },
    ],
    extra: {
      evidence: {
        support: 9, opportunities: 10, rate: 0.9, lower_bound: 0.62, turns_saved: 0.2, days: 9, exposed: 0, dormant_since: null,
        provenance: "desire-path", basis: "count", describe: "9/10 traces over 9 day(s) · rate 0.90 · Wilson lower bound 0.62",
        uplift: {
          kind: "run-after-edit", p_follow: 0.95, p_self: 0.91, mean: 0.04, lower: -0.03, prior_self: 0.6, traces: 10, traces_took: 9, gated: true,
          line: "uplift +0.04 (lower bound -0.03): followed 0.95 when told, 0.91 unaided; 9 of 10 traces did it unaided. Quiet: agents already do this, so delivering it changes little",
        },
      },
    },
  }),
  route({
    id: "card-add-invoice-field",
    summary: "adding a field to an invoice",
    track: "acme-billing/learned",
    version: "0.3.0",
    strength: "prime",
    ceiling: "prime",
    state: "firm",
    trigger: { kind: "intent", label: "a prompt means: add a field to an invoice", family: "add-invoice-field", threshold: 0.811 },
    step: {
      kind: "card",
      label: "procedure card: 3 files · from 4 traces",
      card: {
        sessions: 4,
        passed: 3,
        files: [
          { path: "schema/invoice.json", sessions: 4 },
          { path: "src/billing/invoice_<x>.py", sessions: 4 },
          { path: "tests/test_invoice.py", sessions: 3 },
        ],
        exemplars: [],
        commands: [
          { command: "make gen", sessions: 4, runs: 4, failed: 0, error: null, verified: true },
          { command: "make test", sessions: 4, runs: 6, failed: 2, error: null, verified: null },
        ],
        avoid: [{ command: "python3 -m unittest", sessions: 2, runs: 2, failed: 2, error: "No module named pytest", verified: null }],
        finish: "make test",
        threshold: 0.811,
        grey_from: 0.711,
        shared: false,
      },
    },
    directive: wrap("card-add-invoice-field", "prime", "Earlier sessions of this kind of task in this repository:\n  1. schema/invoice.json\n  2. src/billing/invoice_<x>.py"),
    ev: [3, 4, 0.3, 1.1],
    t: [5, 4],
    c: [2, 1],
    live: [5, 2, 4, 1, 0, 1, 1, 5],
    verbs: ["approve", "extinguish", "pin"],
    extra: {
      evidence: {
        support: 3, opportunities: 4, rate: 0.75, lower_bound: 0.3, turns_saved: 1.1, days: 9, exposed: 0, dormant_since: null,
        provenance: "desire-path", basis: "family", describe: "3/4 traces over 9 day(s) · rate 0.75 · Wilson lower bound 0.30",
      },
    },
  }),
];

// 54 traces over two weeks, one in five in the control arm, each a few decisions.
const events = [];
const learning = [];
const SESSIONS = 54;
const hex = (n) => Math.floor(rnd() * 16 ** n).toString(16).padStart(n, "0");
for (let i = 0; i < SESSIONS; i++) {
  const session = `${hex(8)}-${hex(4)}-4${hex(3)}-a${hex(3)}-${hex(12)}`;
  const arm = i % 5 === 3 ? "control" : "treated";
  const control = arm === "control";
  let ts = START + (i / SESSIONS) * 13.6 * DAY + Math.floor(rnd() * 3 * 3600e3);
  const ev = (event, o = {}) => {
    ts += 20e3 + Math.floor(rnd() * 90e3);
    const directed = control ? [] : (o.d ?? []);
    events.push({
      ts,
      session,
      arm,
      event,
      repo: REPO,
      directed,
      would: o.d ?? [],
      judged: o.j ?? [],
      observed: o.o ?? [],
      continue: Boolean(o.stop && !control),
    });
  };
  const formed = i > 8; // lookup-work-item formed after the first traces
  ev("UserPromptSubmit", formed ? { d: ["lookup-work-item"] } : {});
  const took = control ? rnd() < 0.35 : rnd() < 0.85;
  ev("PostToolBatch", {
    o: [took ? "exec python3 scripts/wi-lookup {id}" : pick(["search:content {pattern}", "list {path}"])],
    j: formed ? [[took ? "followed" : "ignored", "lookup-work-item"]] : [],
    d: took && i > 16 ? ["read-linked-files"] : [],
  });
  if (rnd() < 0.3) ev("PostToolUseFailure", { d: ["dead-end-d41b1e"], o: ["exec python3 -m unittest !failed"] });
  if (rnd() < 0.25) ev("UserPromptSubmit", { d: ["start-with-make-test"] });
  if (rnd() < 0.3) {
    ev("PostToolBatch", { o: ["edit {path}"], d: ["finish-line/regenerate"] });
    const met = control ? rnd() < 0.5 : rnd() < 0.9;
    ev("Stop", { stop: !met, j: met ? [["obligation_met", "finish-line/regenerate"]] : [] });
  }
}
// PreToolUse rewrites: the test runs pointed at the project's .venv, and one withheld in control.
for (const [i, withheld] of [[5, false], [22, false], [40, false], [48, true]]) {
  events.push({
    ts: START + i * 0.25 * DAY + 3600e3,
    session: `pretool-${i}`,
    arm: withheld ? "control" : "treated",
    event: "PreToolUse",
    repo: REPO,
    kind: "pretool",
    pretool: { rewrite: "test", interpreter: ".venv/bin/python" },
    withheld,
    directed: [],
    would: [],
    judged: [],
    observed: [],
    continue: false,
  });
}
events.sort((a, b) => a.ts - b.ts);

const run = (ts, changes, used) => ({
  ts,
  kind: "condition",
  trigger: "auto",
  repo: REPO,
  sessions_read: used + 1,
  sessions_used: used,
  duration_ms: 640,
  index_generation: hex(16),
  changes,
});
learning.push(
  run(START + 2 * DAY, [{ route: "lookup-work-item", track: "acme-billing/learned", change: "new", from: null, to: "prime", summary: "work item reflex added at prime (6 of 9 traces)" }], 9),
  run(START + 5 * DAY + 3 * 3600e3, [{ route: "lookup-work-item", track: "acme-billing/learned", change: "strength", from: "prime", to: "fire", summary: "work item reflex prime → fire (lower bound 0.51)" }], 21),
  run(START + 8 * DAY, [{ route: "read-linked-files", track: "acme-billing/learned", change: "strength", from: "prime", to: "fire", summary: "linked files reflex prime → fire (15 of 16 traces)" }], 33),
  run(START + 11 * DAY, [{ route: "lookup-work-item", track: "acme-billing/learned", change: "evidence", summary: "evidence 22/32 → 23/33 traces" }], 45),
);

const observed = [
  { key: "exec python3 scripts/wi-lookup {id}", count: 41, sessions: 38, route: "lookup-work-item", how: "directs" },
  { key: "edit {path}", count: 30, sessions: 17, route: null, how: null },
  { key: "exec python3 -m unittest !failed", count: 14, sessions: 14, route: "dead-end-d41b1e", how: "avoids" },
];

const graph = (o) => ({
  generation: "8dd5ec01a3f2b7c4",
  home: "~/.adage",
  sessions: o.sessions,
  stop_requests: o.stops ?? 0,
  routes: o.routes,
  observed: o.observed ?? [],
  events: o.events ?? [],
  learning: o.learning ?? [],
  bars: { min_support: 3, fire_lower_bound: 0.5, uplift_min: 0.1 },
  savings: o.savings ?? [],
  pretool: o.pretool ?? [],
  runtime: { rewrite: { on: true, config: true, marker: false }, control: 0.1, card_threshold: 0.811 },
});

// The savings estimate: past the gate (12 control, 42 treated sessions), with its interval.
const est = (delta, lo, hi, c, t) => ({ delta, lo, hi, mean_control: c, mean_treated: t, n_control: 12, n_treated: 42, winsorized_at: 0.9 });
const savings = {
  repo: REPO,
  computed_at: NOW,
  pricing_as_of: "2026-06-24",
  n_treated: 42,
  n_control: 12,
  gate_met: true,
  verdict: "saved",
  cost_usd: est(0.034, 0.011, 0.058, 0.212, 0.178),
  model_turns: est(3.1, 1.2, 5.0, 24.2, 21.1),
  turns_to_first_edit: est(1.4, -0.3, 3.0, 6.1, 4.7),
  unpriced: 0,
  asked_for_help_treated: 0.05,
  asked_for_help_control: 0.08,
  counts: { days: 30, sessions: 54, directives: 131, followed: 96, dead_ends_headed_off: 9, finish_lines_met: 11, rewritten: 3 },
  need: { control: 10, treated: 30 },
  lines: [
    "savings: ~$0.034 and 3.1 turns per session, API-list equivalent (95% CI $0.011–$0.058)",
    "         ≈ $1.43 over 42 sessions, vs 12 where Adage stayed quiet (priced as of 2026-06-24)",
  ],
  guardrail: "ended asking you for help: 5% with Adage, 8% without",
};

const treated = new Set(events.filter((e) => e.arm === "treated").map((e) => e.session)).size;
const busy = graph({
  sessions: { total: SESSIONS, treated, control: SESSIONS - treated },
  stops: events.filter((e) => e.continue).length,
  routes,
  observed,
  events: events.slice(-400),
  learning,
  savings: [savings],
  pretool: [{ repo: REPO, rewritten: 3, advised: 0, withheld: 1 }],
});
const empty = graph({
  sessions: { total: 0, treated: 0, control: 0 },
  routes: routes.filter((r) => r.instinct).map((r) => ({ ...r, arms: { treated: { sessions: 0, applied: 0, followed: 0, met: 0, hit_avoided: 0 }, control: { sessions: 0, applied: 0, followed: 0, met: 0, hit_avoided: 0 } }, live: { ...r.live, emitted: 0, would_have: 0, obligation_met: 0, sessions: 0 } })),
});
const health = {
  checks: [
    { check: "binary", status: "pass", detail: "adage 0.1.0 at ~/.local/bin/adage" },
    { check: "data directory", status: "pass", detail: "~/.adage is writable" },
    { check: "index", status: "pass", detail: "generation 8dd5ec01a3f2b7c4, 8 route(s) in 1 repository" },
    { check: "tracks", status: "pass", detail: "4 track(s) (1 imported), 8 route(s), all valid" },
    { check: "instincts", status: "pass", detail: "on; 1 bound for acme-billing" },
    { check: "hooks (Claude Code)", status: "pass", detail: "installed in .claude/settings.local.json" },
    { check: "hooks (Codex)", status: "pass", detail: "installed in .codex/hooks.json" },
    { check: "embedding model", status: "pass", detail: "potion-base-8m-i8: bundled in this binary (int8, SHA-256 verified)" },
    { check: "spools", status: "pass", detail: "54 session spool(s), 1.2 MB" },
    { check: "fail-open", status: "pass", detail: "no fail-open events in 7 days" },
  ],
  summary: { pass: 10, warn: 0, fail: 0 },
  runtime: {
    rewrite: { on: true, config: true, marker: false },
    embeddings: { model: "potion-base-8m-i8 (bundled)", bundled: true, fallback: false },
  },
};

writeFileSync(join(here, "busy.json"), JSON.stringify(busy, null, 1) + "\n");
writeFileSync(join(here, "empty.json"), JSON.stringify(empty, null, 1) + "\n");
writeFileSync(join(here, "health.json"), JSON.stringify(health, null, 1) + "\n");
console.log(`busy: ${busy.routes.length} reflexes, ${busy.events.length} events, ${SESSIONS} traces`);
