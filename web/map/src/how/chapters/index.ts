// The chapters of "How it works", in the order Adage does the work.

import type { Chapter } from "../types";
import { storyboard } from "./storyboard";
import { seal } from "./seal";
import { subgraphs } from "./subgraphs";
import { train } from "./train";

const readTraces = storyboard({
  id: "read",
  title: "Read traces",
  status: "Built",
  kind: "built",
  summary:
    "Claude Code, Codex and OpenCode transcripts become one shape: prompts, then tool calls grouped into turns, each command parsed and classed by effect.",
  source: "crates/adage-learn/src/{import,codex,opencode,cache}.rs",
  beats: [
    { title: "Transcript lines stream in", body: "Each harness writes its own format. Adage reads Claude Code JSONL, Codex rollouts and OpenCode exports into one shape.", chips: ["3 harnesses", "prompt ≤ 64 KiB", "Codex ≤ 4,096 calls"] },
    { title: "Tool calls pair with their results", body: "Calls in one assistant message form a turn; each result joins its call by id, and an error marks the call failed.", chips: ["turns by message id"] },
    { title: "Commands split and get an effect", body: "A compound command splits into steps; each is classed from observe to publish. A failure lands on the last step, or on every step when the command used ||.", chips: ["observe < verify < regenerate < environment < author < publish"] },
    { title: "Steered traces are marked", body: "A trace where Adage already directed the agent is marked for that kind of reflex, so a reflex never learns from its own advice.", chips: ["parse cache by size + mtime"] },
  ],
});

const miners = storyboard({
  id: "miners",
  title: "Condition with miners",
  status: "Built · default",
  kind: "built",
  summary: "Five miners look for lookups, linked files, dead ends, finish lines and intents. Evidence is counted in distinct traces, with a Wilson lower bound against the fire bar.",
  source: "crates/adage-learn/src/mine.rs",
  beats: [
    { title: "Candidates light up", body: "Each miner scans the traces for its kind of habit: a command that takes the ticket from the prompt, files read after a lookup, a failing check followed by one that works.", chips: ["5 miners"] },
    { title: "Evidence counts traces", body: "Support is the number of distinct traces that did it, out of those that had the chance.", chips: ["min support 5"] },
    { title: "The lower bound meets the fire bar", body: "A reflex fires when the Wilson lower bound of its rate reaches the fire bar; it primes when the rate reaches 0.50.", chips: ["fire LB 0.80", "prime 0.50", "keep 0.30"] },
    { title: "Ceilings cap strength", body: "What a step does limits how strongly Adage may direct it: a check can fire, an install can only prime, and editing or publishing is never directed.", chips: ["effect ceilings"] },
  ],
});

const intent = storyboard({
  id: "intent",
  title: "Group prompts by intent",
  status: "Built · optional model",
  kind: "built",
  summary: "Prompts are normalised, embedded with a pinned 31 MB model, and clustered. Families split or merge by what their traces did next.",
  source: "crates/adage-learn/src/intent.rs · crates/adage-intent",
  beats: [
    { title: "Prompts are normalised", body: "Tickets, paths, versions and code become placeholders, so prompts compare by what they ask, not their details.", chips: ["≤ 128 tokens"] },
    { title: "Prompts embed and link", body: "Each prompt becomes a 256-d point; nearest neighbours link above a similarity threshold.", chips: ["k 10", "eps 0.60"] },
    { title: "Communities form families", body: "Louvain finds communities; each is split or merged by the first command its traces ran.", chips: ["resolution 2.0", "split sep 0.85", "merge 0.80"] },
  ],
});

const choose = storyboard({
  id: "choose",
  title: "Choose and compile",
  status: "Track G · behind the engine flag",
  kind: "flag",
  summary: "Candidates are calibrated, then selected by compression: a reflex earns its place by explaining traces in fewer bits. Winners compile to one of eight delivery shapes.",
  source: "crates/adage-graph/src/{select,compile}",
  beats: [
    { title: "Confidence is calibrated", body: "Isotonic regression bends raw confidence to match what happened in held-out traces.", chips: ["200 bins", "3 folds"] },
    { title: "Compression chooses", body: "A code table fills greedily; each candidate stays only if it saves bits.", chips: ["≤ 256 reflexes", "≤ 20,000 patterns scored"] },
    { title: "Winners take a shape", body: "Run, read-batch, avoid, obligation, scope, env_sync, sequence or context.", chips: ["8 shapes"] },
  ],
});

const hook = storyboard({
  id: "hook",
  title: "Decide at the hook",
  status: "Built",
  kind: "built",
  summary: "Each hook event runs a pure decision against the index. One trace in ten is control: Adage records what it would have said and stays silent.",
  source: "crates/adage-runtime/src/lib.rs · crates/adage-hook",
  beats: [
    { title: "Most events pass straight through", body: "A fast check rejects events no reflex could match before anything else runs.", chips: ["p95 10 ms, no match"] },
    { title: "A hash picks the arm", body: "The trace id hashes to treated or control; control is 10% by default.", chips: ["control 10%"] },
    { title: "The directive is judged", body: "A fixed template renders the directive; the next three batches decide followed or ignored.", chips: ["≤ 3 directives per event", "fails open"] },
  ],
});

const measure = storyboard({
  id: "measure",
  title: "Measure against control",
  status: "Built",
  kind: "built",
  summary: "Effects are measured treated against control on the same tasks, with the task as the unit and permutation tests that make no distribution assumptions.",
  source: "crates/adage-eval · evals/live",
  beats: [
    { title: "Treated and control, task by task", body: "Each task runs in both arms; the difference is measured within the task.", chips: ["task is the unit"] },
    { title: "Shuffle to find chance", body: "Arms are shuffled within each task thousands of times; the observed difference is compared with that spread.", chips: ["10,000 permutations", "Holm across metrics"] },
  ],
});

export const CHAPTERS: Chapter[] = [readTraces, miners, intent, subgraphs, choose, train, seal, hook, measure];
