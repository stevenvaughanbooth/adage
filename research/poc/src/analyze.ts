/**
 * The measurement.
 *
 * The project's load-bearing assumption is that agent context-gathering is predictable
 * enough to compile. This measures that directly, and is deliberately the cheapest thing
 * that can kill the idea.
 *
 * Headline number: leave-one-out top-1 accuracy of a next-action predictor. If a trivial
 * bigram over canonical actions can guess the agent's next move, a learned predictor over
 * richer state will do better. If it cannot, the thesis is in trouble.
 */

import type { CanonicalAction, Session, Window } from "./types.ts";
import { actionKey } from "./window.ts";

export interface Report {
  corpus: {
    sessions: number; sidechains: number; prompts: number;
    toolCalls: number; windows: number;
  };
  normalizer: {
    total: number; unknown: number; lowConfidence: number;
    coverage: number; byOp: Record<string, number>; byExecClass: Record<string, number>;
  };
  windows: {
    sizeHistogram: Record<number, number>;
    meanActions: number; meanTurns: number;
    closedByWrite: number;
  };
  templates: {
    distinctExact: number; distinctTemplate: number;
    repeatRateExact: number; repeatRateTemplate: number;
    top: { key: string; count: number }[];
    headCoverage: { topK: number; share: number }[];
  };
  predictability: {
    samples: number;
    unigramTop1: number;
    bigramTop1: number;
    bigramBackoffTop1: number;
    coverage: number;
  };
}

const BOS = "<bos>";

export function analyze(sessions: Session[], windows: Window[]): Report {
  /* ---------------------------------------------------------- normalizer */
  const byOp: Record<string, number> = {};
  const byExecClass: Record<string, number> = {};
  let total = 0, unknown = 0, lowConf = 0;

  for (const s of sessions) {
    for (const r of s.records) {
      if (r.kind !== "tool_call" || !r.normalized) continue;
      total++;
      const a = r.normalized.action;
      byOp[a.op] = (byOp[a.op] ?? 0) + 1;
      if (a.op === "exec") byExecClass[a.class] = (byExecClass[a.class] ?? 0) + 1;
      if (a.op === "unknown") unknown++;
      if (r.normalized.confidence < 0.7) lowConf++;
    }
  }

  /* ------------------------------------------------------------- windows */
  const sizeHistogram: Record<number, number> = {};
  let sumActions = 0, sumTurns = 0, closed = 0;
  for (const w of windows) {
    sizeHistogram[w.actions.length] = (sizeHistogram[w.actions.length] ?? 0) + 1;
    sumActions += w.actions.length;
    sumTurns += w.turns;
    if (w.closedByWrite) closed++;
  }

  /* ----------------------------------------------------------- templates */
  const exact = new Map<string, number>();
  const templ = new Map<string, number>();
  for (const w of windows) {
    for (const a of w.actions) {
      const e = actionKey(a, "exact");
      const t = actionKey(a, "template");
      exact.set(e, (exact.get(e) ?? 0) + 1);
      templ.set(t, (templ.get(t) ?? 0) + 1);
    }
  }
  const actionTotal = sumActions || 1;
  const repeated = (m: Map<string, number>) =>
    [...m.values()].filter((v) => v > 1).reduce((a, b) => a + b, 0) / actionTotal;

  const sortedT = [...templ.entries()].sort((a, b) => b[1] - a[1]);
  const headCoverage = [1, 5, 10, 20, 50].map((k) => ({
    topK: k,
    share: sortedT.slice(0, k).reduce((a, [, v]) => a + v, 0) / actionTotal,
  }));

  /* ------------------------------------------------------ predictability */
  const pred = predictability(windows);

  return {
    corpus: {
      sessions: sessions.filter((s) => !s.records[0]?.isSidechain).length,
      sidechains: sessions.filter((s) => s.records[0]?.isSidechain).length,
      prompts: sessions.reduce((n, s) => n + s.records.filter((r) => r.kind === "prompt").length, 0),
      toolCalls: total,
      windows: windows.length,
    },
    normalizer: {
      total, unknown, lowConfidence: lowConf,
      coverage: total ? 1 - unknown / total : 0,
      byOp, byExecClass,
    },
    windows: {
      sizeHistogram,
      meanActions: windows.length ? sumActions / windows.length : 0,
      meanTurns: windows.length ? sumTurns / windows.length : 0,
      closedByWrite: closed,
    },
    templates: {
      distinctExact: exact.size,
      distinctTemplate: templ.size,
      repeatRateExact: repeated(exact),
      repeatRateTemplate: repeated(templ),
      top: sortedT.slice(0, 15).map(([key, count]) => ({ key, count })),
      headCoverage,
    },
    predictability: pred,
  };
}

/**
 * Leave-one-out next-action prediction over templated canonical actions.
 *
 * Three predictors, each scored by removing the sample under test from the counts:
 *  - unigram:  always predict the globally most common next action
 *  - bigram:   predict argmax P(next | prev)
 *  - backoff:  bigram where seen at least twice, else unigram
 *
 * Backoff is the honest baseline: it is roughly what a next-action predictor learns from
 * the previous action alone, with no other state.
 */
function predictability(windows: Window[]): Report["predictability"] {
  const samples: { prev: string; next: string }[] = [];
  for (const w of windows) {
    let prev = BOS;
    for (const a of w.actions) {
      const next = actionKey(a, "template");
      samples.push({ prev, next });
      prev = next;
    }
  }

  const uni = new Map<string, number>();
  const bi = new Map<string, Map<string, number>>();
  for (const { prev, next } of samples) {
    uni.set(next, (uni.get(next) ?? 0) + 1);
    if (!bi.has(prev)) bi.set(prev, new Map());
    const m = bi.get(prev)!;
    m.set(next, (m.get(next) ?? 0) + 1);
  }

  const argmaxExcluding = (m: Map<string, number>, exclude: string): [string | null, number] => {
    let best: string | null = null, bestN = 0, total = 0;
    for (const [k, v] of m) {
      const n = k === exclude ? v - 1 : v;
      total += n;
      if (n > bestN) { bestN = n; best = k; }
    }
    return [bestN > 0 ? best : null, total];
  };

  let uniHits = 0, biHits = 0, backoffHits = 0, biCovered = 0;

  for (const { prev, next } of samples) {
    const [uBest] = argmaxExcluding(uni, next);
    if (uBest === next) uniHits++;

    const m = bi.get(prev)!;
    const [bBest, bTotal] = argmaxExcluding(m, next);
    if (bBest === next) biHits++;
    if (bTotal >= 2) {
      biCovered++;
      if (bBest === next) backoffHits++;
    } else if (uBest === next) {
      backoffHits++;
    }
  }

  const n = samples.length || 1;
  return {
    samples: samples.length,
    unigramTop1: uniHits / n,
    bigramTop1: biHits / n,
    bigramBackoffTop1: backoffHits / n,
    coverage: biCovered / n,
  };
}
