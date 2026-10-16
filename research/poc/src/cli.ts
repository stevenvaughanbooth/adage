#!/usr/bin/env node --experimental-strip-types
/**
 * PoC entry point.
 *
 *   node --experimental-strip-types src/cli.ts report
 *   node --experimental-strip-types src/cli.ts windows [n]
 *   node --experimental-strip-types src/cli.ts shell '<command>'
 */

import { discover, readSession, type ImportStats } from "./import/claude.ts";
import { extractWindows, actionKey } from "./window.ts";
import { analyze } from "./analyze.ts";
import { normalizeShell } from "./normalize/shell.ts";
import type { Session, Window } from "./types.ts";

const pct = (x: number) => `${(x * 100).toFixed(1)}%`;
const bar = (x: number, w = 28) => "█".repeat(Math.round(x * w)).padEnd(w, "·");

async function load(): Promise<{ sessions: Session[]; windows: Window[]; stats: ImportStats }> {
  const found = await discover();
  const stats: ImportStats = {
    lines: 0, parsed: 0, quarantined: 0, prompts: 0, toolCalls: 0, unknownTools: 0,
  };
  const sessions: Session[] = [];
  for (const d of found) sessions.push(await readSession(d, stats));
  const windows = sessions.flatMap(extractWindows);
  return { sessions, windows, stats };
}

async function report() {
  const { sessions, windows, stats } = await load();
  const r = analyze(sessions, windows);

  console.log("\n\x1b[1mCORPUS\x1b[0m");
  console.log(`  sessions           ${r.corpus.sessions} (+${r.corpus.sidechains} subagent)`);
  console.log(`  lines parsed       ${stats.parsed}  (quarantined ${stats.quarantined})`);
  console.log(`  prompts            ${r.corpus.prompts}`);
  console.log(`  tool calls         ${r.corpus.toolCalls}`);
  console.log(`  discovery windows  ${r.corpus.windows}`);

  console.log("\n\x1b[1mNORMALIZER\x1b[0m  (gate: ≥95% coverage)");
  console.log(`  coverage           ${bar(r.normalizer.coverage)} ${pct(r.normalizer.coverage)}`);
  console.log(`  low confidence     ${pct(r.normalizer.lowConfidence / (r.normalizer.total || 1))}`);
  console.log("  by op:");
  for (const [op, n] of Object.entries(r.normalizer.byOp).sort((a, b) => b[1] - a[1])) {
    console.log(`    ${op.padEnd(10)} ${String(n).padStart(5)}  ${pct(n / r.normalizer.total)}`);
  }
  console.log("  exec classes:");
  for (const [c, n] of Object.entries(r.normalizer.byExecClass).sort((a, b) => b[1] - a[1])) {
    console.log(`    ${c.padEnd(10)} ${String(n).padStart(5)}`);
  }

  console.log("\n\x1b[1mWINDOWS\x1b[0m  (the size of the prize)");
  console.log(`  mean actions       ${r.windows.meanActions.toFixed(2)}`);
  console.log(`  mean turns         ${r.windows.meanTurns.toFixed(2)}`);
  console.log(`  closed by write    ${r.windows.closedByWrite}/${r.corpus.windows}`);
  console.log("  size histogram:");
  for (const [size, n] of Object.entries(r.windows.sizeHistogram).sort((a, b) => +a[0] - +b[0])) {
    console.log(`    ${String(size).padStart(2)} actions  ${String(n).padStart(4)}  ${bar(n / r.corpus.windows, 20)}`);
  }

  console.log("\n\x1b[1mTEMPLATES\x1b[0m  (is there a fat head?)");
  console.log(`  distinct exact     ${r.templates.distinctExact}`);
  console.log(`  distinct template  ${r.templates.distinctTemplate}`);
  console.log(`  repeat rate exact  ${pct(r.templates.repeatRateExact)}`);
  console.log(`  repeat rate templ  ${pct(r.templates.repeatRateTemplate)}   <- generalization gain`);
  console.log("  head coverage:");
  for (const h of r.templates.headCoverage) {
    console.log(`    top ${String(h.topK).padStart(2)}        ${bar(h.share)} ${pct(h.share)}`);
  }
  console.log("  most common templates:");
  for (const t of r.templates.top) {
    console.log(`    ${String(t.count).padStart(4)}  ${t.key.slice(0, 92)}`);
  }

  const p = r.predictability;
  console.log("\n\x1b[1mPREDICTABILITY\x1b[0m  \x1b[2m(leave-one-out top-1, templated actions)\x1b[0m");
  console.log(`  samples            ${p.samples}`);
  console.log(`  unigram baseline   ${bar(p.unigramTop1)} ${pct(p.unigramTop1)}`);
  console.log(`  bigram             ${bar(p.bigramTop1)} ${pct(p.bigramTop1)}`);
  console.log(`  bigram + backoff   ${bar(p.bigramBackoffTop1)} ${pct(p.bigramBackoffTop1)}  <- headline`);
  console.log(`  bigram coverage    ${pct(p.coverage)}`);
  console.log();
  console.log("  \x1b[2mThis is the floor, not the ceiling: one previous action, no session state,\x1b[0m");
  console.log("  \x1b[2mno prompt features, no repo context. A learned predictor should beat it.\x1b[0m\n");
}

async function showWindows(limit: number) {
  const { windows } = await load();
  for (const w of windows.slice(0, limit)) {
    console.log(`\n\x1b[1m${w.prompt.slice(0, 96).replace(/\n/g, " ")}\x1b[0m`);
    console.log(`  \x1b[2m${w.actions.length} actions, ${w.turns} turns, closedByWrite=${w.closedByWrite}\x1b[0m`);
    for (const a of w.actions) console.log(`    → ${actionKey(a, "template").slice(0, 100)}`);
  }
  console.log();
}

const [cmd, arg] = process.argv.slice(2);
switch (cmd) {
  case "report":  await report(); break;
  case "windows": await showWindows(Number(arg ?? 10)); break;
  case "shell":
    console.log(JSON.stringify(normalizeShell(arg ?? "", "/repo"), null, 2));
    break;
  default:
    console.log("usage: cli.ts report | windows [n] | shell '<command>'");
}
