#!/usr/bin/env node
// The held-out result, for the site: turns the report `heldout_cohort.py report` writes
// (reports/heldout-<time>.json, the registered analysis of evals/reflex-proof/harness/
// heldout_analysis.py) into public/evidence/heldout.json, which /how-we-tested and the home page
// read. Nothing is computed that the report does not say, except sums of the sessions' own token
// counts (--sessions) and plain per-session means; the outcome is the report's.
//
//   node scripts/heldout-evidence.mjs <$H/reports/heldout-<time>.json> [--sessions <$H>] [--no-sessions]
//        [--link <repository path of the archived report>] [--out <file>]
//   node scripts/heldout-evidence.mjs --pending          # the state before the result exists
//
// The sessions: the report has cost but no tokens, and hidden-test results per arm but not per
// block, so the positive sessions' own records are read too (read-only): tokens from
// <$H>/sessions/*/summary.json, and which blocks failed in which arm from collection.json (the same
// `quality.passed` the analysis reads). $H is found from the report's path (<$H>/reports/…) unless
// --sessions names it; --no-sessions skips it, and the page then says what was not converted.
//
// Two modes are read out of the one report, as the product ships them:
//   advisory  = NOEXEC, the default (hooks, instincts and the card; nothing executed, no approvals),
//               against the agent alone: the analysis's pre-registered report-only contrast;
//   execution = BEST, opt-in (person-approved CPU test runs and the Stop gate), against the agent
//               alone: the study's gated test, whose registered outcome is the report's `outcome`.
//
// The report and its decision record are copied beside the output (heldout-report.json,
// heldout-record.md) unchanged, so the page links the exact files its numbers came from.

import { createHash } from "node:crypto";
import { copyFileSync, existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const SCHEMA = "adage-heldout-evidence-2";
const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, "../../..");
const OUT = join(HERE, "../public/evidence/heldout.json");
const ARMS = ["A0", "NOEXEC", "BEST"];

/** The study as registered (launch-gate-1 revisions 9 to 11; heldout_analysis.py). */
export const STUDY = {
  name: "Held-out comparison",
  repository: "Marshmallow",
  model: "Claude Haiku 4.5",
  tasks: ["mm-curve--has-keys", "mm-curve--iso-date", "mm-curve--json-object", "mm-curve--lat-long"],
  repetitions: 5,
  blocks: 20,
  sessions: 65,
  controls: ["failed", "later-edit", "config-change", "broader", "rerun"],
  arms: {
    A0: "The agent alone (no Adage)",
    NOEXEC: "Advisory mode (the default): instincts, the orientation card and learned advice; nothing executed",
    BEST: "Execution mode (opt-in): advisory plus person-approved CPU test runs and the finish-line check",
  },
  pass_rule: {
    threshold: 0.9,
    confidence: 0.95,
    quality_margin: 1,
    text: "The one-sided 95% upper bound of Adage's model requests divided by the agent alone's, over the paired blocks, is at most 0.90, and Adage's hidden-test passes are at least the agent alone's minus one. Every applicable G criterion must also hold.",
  },
  frozen: "The four tasks were frozen on 2026-10-01 before any development result was read and were never used for tuning. Their prompts carry no work order, no test block and no instruction to run tests.",
  protocol: {
    analysis: "heldout-analysis-1",
    revisions: "launch-gate-1 revisions 9 to 11",
    document: "docs/launch-gate-protocol.md",
    code: "evals/reflex-proof/harness/heldout_analysis.py",
    tasks: "evals/reflex-proof/harness/results/20261001-heldout-tasks/README.md",
  },
};

const VERDICTS = {
  pass: { verdict: "GO", label: "Passed", headline: "Adage passed the held-out comparison." },
  fail: { verdict: "NO-GO", label: "Did not pass", headline: "Adage did not pass the held-out comparison." },
  unverified: { verdict: "NO VERDICT", label: "Inconclusive", headline: "No verdict: too many request counts could not be verified." },
  "protocol-failure": { verdict: "NO VERDICT", label: "Inconclusive", headline: "No verdict: a safety control did not run as designed." },
  incomplete: { verdict: "NO VERDICT", label: "Incomplete", headline: "No verdict yet: not every session was collected." },
};

const num = (v) => (typeof v === "number" && Number.isFinite(v) ? v : null);
const round = (v, d = 4) => (num(v) === null ? null : Math.round(v * 10 ** d) / 10 ** d);

function contrast(c) {
  if (!c || typeof c !== "object") return null;
  return {
    ratio: num(c.ratio), lower: num(c.lower), upper: num(c.upper), blocks: num(c.blocks),
    blocks_fewer: num(c.blocks_fewer), sign_flip_p: num(c.sign_flip_p), passed: c.passed === true,
    ...(Array.isArray(c.excluded_blocks) ? { excluded_blocks: c.excluded_blocks } : {}),
  };
}

/** Token totals per arm from the positive sessions' summaries (<root>/sessions/NN-ARM-bNN-task). */
export function sessionTokens(root) {
  const dir = join(root, "sessions");
  const tally = Object.fromEntries(ARMS.map((a) => [a, { sessions: 0, input: 0, output: 0, cache_read: 0, cache_write: 0 }]));
  for (const name of readdirSync(dir)) {
    const m = /^\d+-(A0|NOEXEC|BEST)-b\d+-/.exec(name);
    const file = join(dir, name, "summary.json");
    if (!m || !existsSync(file)) continue;
    const u = JSON.parse(readFileSync(file, "utf8")).usage ?? {};
    const t = tally[m[1]];
    t.sessions += 1;
    t.input += u.input_tokens ?? 0;
    t.output += u.output_tokens ?? 0;
    t.cache_read += u.cache_read_input_tokens ?? 0;
    t.cache_write += u.cache_creation_input_tokens ?? 0;
  }
  return Object.fromEntries(ARMS.map((a) => {
    const t = tally[a];
    const total = t.input + t.output + t.cache_read + t.cache_write;
    return [a, { ...t, total, per_session: t.sessions ? Math.round(total / t.sessions) : null }];
  }));
}

/** Per arm, the positive blocks whose hidden tests failed, from the sessions' collection records. */
export function sessionFailures(root) {
  const dir = join(root, "sessions");
  const out = Object.fromEntries(ARMS.map((a) => [a, []]));
  for (const name of readdirSync(dir)) {
    const m = /^\d+-(A0|NOEXEC|BEST)-b(\d+)-(.+)$/.exec(name);
    const file = join(dir, name, "collection.json");
    if (!m || !existsSync(file)) continue;
    const c = JSON.parse(readFileSync(file, "utf8"));
    if (c.quality?.passed !== true) out[m[1]].push({ block: Number(m[2]), task: c.task ?? m[3] });
  }
  for (const a of ARMS) out[a].sort((x, y) => x.block - y.block);
  return out;
}

const pct = (v) => (num(v) === null ? null : Math.round(v * 1000) / 10);

/** One mode against the agent alone, as the pages show it. */
function mode(kind, report, c, arms, failures) {
  const arm = kind === "advisory" ? "NOEXEC" : "BEST";
  const x = arms[arm], a0 = arms.A0;
  if (!c || !x || !a0) return null;
  const per = (y) => (num(y?.cost_usd) !== null && num(y?.sessions) ? y.cost_usd / y.sessions : null);
  const cost = per(x), cost0 = per(a0);
  const threshold = num(report.primary?.threshold) ?? STUDY.pass_rule.threshold;
  const clears = num(c.upper) !== null && c.upper <= threshold;
  const passes = num(x.hidden_test_passes), passes0 = num(a0.hidden_test_passes);
  const failedHere = failures?.[arm] ?? null;
  const sharedAll = failures ? failedHere.filter((f) => ARMS.every((b) => failures[b].some((g) => g.block === f.block))) : null;
  const out = {
    arm,
    role: kind === "advisory" ? "the default" : "opt-in",
    registered: kind === "advisory" ? "report-only comparison, registered before the study" : "the study's gated test",
    ratio: num(c.ratio), lower: num(c.lower), upper: num(c.upper), threshold,
    clears_line: clears,
    fewer_requests_percent: num(c.ratio) === null ? null : Math.round((1 - c.ratio) * 100),
    blocks: num(c.blocks), blocks_fewer: num(c.blocks_fewer), sign_flip_p: num(c.sign_flip_p),
    mean_requests: num(x.mean_requests_included), mean_requests_alone: num(a0.mean_requests_included),
    cost_per_session_usd: cost === null ? null : round(cost, 6), cost_per_session_alone_usd: cost0 === null ? null : round(cost0, 6),
    cost_change_percent: cost === null || !cost0 ? null : pct(cost / cost0 - 1),
    hidden_test_passes: passes, hidden_test_passes_alone: passes0, of: num(x.sessions),
    failed_blocks: failedHere, failed_blocks_alone: failures?.A0 ?? null, failures_shared_by_all_arms: sharedAll,
  };
  if (kind === "advisory") {
    out.status = clears && passes !== null && passes0 !== null && passes >= passes0 ? "clear" : clears ? "fewer-calls-lower-quality" : "no-clear-saving";
  } else {
    const failedCriteria = (report.go_criteria ? Object.entries(report.go_criteria) : []).filter(([, g]) => g.applies && g.passed === false).map(([id]) => id);
    out.outcome = report.outcome;
    out.quality_rule_passed = report.quality?.passed === true && !failedCriteria.includes("G5");
    out.failed_criteria = failedCriteria;
    out.status = report.outcome === "pass" ? "passed" : !out.quality_rule_passed ? "experimental-quality" : report.primary?.passed === false ? "experimental-savings" : "experimental";
  }
  return out;
}

/** heldout.json before the result exists. */
export function pending() {
  return { schema: SCHEMA, status: "pending", study: STUDY, result: null, source: null };
}

/**
 * heldout.json from a held-out report (the object `heldout_cohort.py report` writes).
 * @param {any} report
 * @param {{ tokens?: Record<string, any> | null, failures?: Record<string, { block: number, task: string }[]> | null, source?: { report: string, report_sha256: string, decision_record: string | null, converted_at: string, converter: string } | null }} [options]
 */
export function convert(report, { tokens = null, failures = null, source = null } = {}) {
  if (!report || typeof report !== "object" || typeof report.outcome !== "string") throw new Error("not a held-out report: no outcome");
  if (!(report.outcome in VERDICTS)) throw new Error(`unknown outcome ${JSON.stringify(report.outcome)}`);
  if (report.protocol && report.protocol !== STUDY.protocol.analysis) throw new Error(`unexpected analysis protocol ${report.protocol}`);
  const valid = report.valid !== false;
  const v = valid ? VERDICTS[report.outcome] : { verdict: "NO VERDICT", label: "Invalid", headline: "No verdict: an approval lapsed during the study, so this report is not evidence." };
  const arms = report.arms ?? {};
  const armRow = (a) => {
    const x = arms[a];
    if (!x) return null;
    return {
      label: STUDY.arms[a], sessions: num(x.sessions), mean_requests: num(x.mean_requests_included),
      requests_included: num(x.requests_included), observed_requests: num(x.observed_requests),
      hidden_test_passes: num(x.hidden_test_passes), censored: num(x.censored),
      verification_requests: num(x.verification_requests), cost_usd: num(x.cost_usd),
      cost_per_session_usd: num(x.cost_usd) !== null && num(x.sessions) ? round(x.cost_usd / x.sessions, 6) : null,
      tokens: tokens?.[a] ?? null,
    };
  };
  const criteria = Object.entries(report.go_criteria ?? {}).sort(([a], [b]) => a.localeCompare(b, "en", { numeric: true }))
    .map(([id, c]) => ({ id, applies: c.applies === true, passed: c.passed === true ? true : c.passed === false ? false : null, rule: c.applies ? c.form ?? "" : c.reason ?? "" }));
  const controls = (report.controls ?? []).map((c) => ({
    condition: c.condition, collected: c.collected === true, realized: c.realized === true, necessary_work_skipped: c.necessary_work_skipped === true,
  }));
  const s = report.sensitivity ?? {};
  return {
    schema: SCHEMA,
    status: "result",
    study: STUDY,
    result: {
      outcome: report.outcome,
      valid,
      verdict: v.verdict,
      label: v.label,
      headline: v.headline,
      best_arm: report.best?.arm ?? report.best_arm ?? null,
      primary: report.primary ? { ...contrast(report.primary), threshold: num(report.primary.threshold), included_blocks: num(report.primary.included_blocks) } : null,
      quality: report.quality ? {
        best: num(report.quality.best), a0: num(report.quality.a0), noexec: num(arms.NOEXEC?.hidden_test_passes),
        of: num(arms.BEST?.sessions), margin: num(report.quality.margin), passed: report.quality.passed === true,
      } : null,
      arms: Object.fromEntries(ARMS.map((a) => [a, armRow(a)]).filter(([, r]) => r)),
      tokens_converted: tokens !== null,
      modes: valid && (report.outcome === "pass" || report.outcome === "fail") ? {
        advisory: mode("advisory", report, report.report_only?.noexec_vs_a0, arms, failures),
        execution: mode("execution", report, report.primary, arms, failures),
      } : null,
      criteria,
      controls,
      controls_passed: controls.length === STUDY.controls.length && controls.every((c) => c.realized && !c.necessary_work_skipped),
      sensitivity: {
        without_censored_blocks: contrast(s.without_censored_blocks),
        without_has_keys: contrast(s.without_has_keys),
      },
      report_only: {
        noexec_vs_a0: contrast(report.report_only?.noexec_vs_a0),
        best_vs_noexec: contrast(report.report_only?.best_vs_noexec),
      },
      excluded_blocks: report.request_verification?.excluded_blocks ?? [],
      warnings: report.warnings ?? [],
      problems: report.problems ?? [],
      lapses: (report.lapses ?? []).map((l) => ({ order: l.order, problem: l.problem })),
      assignments: num(report.assignments),
      collected: num(report.collected),
      build_commit: report.build_commit ?? null,
    },
    source,
  };
}

function main(argv) {
  const args = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--pending") args.pending = true;
    else if (a === "--sessions" || a === "--link" || a === "--out") args[a.slice(2)] = argv[++i];
    else if (a === "--no-sessions") args.noSessions = true;
    else if (a === "-h" || a === "--help") args.help = true;
    else args._.push(a);
  }
  if (args.help || (!args.pending && args._.length !== 1)) {
    console.log("usage: node scripts/heldout-evidence.mjs <$H/reports/heldout-<time>.json> [--sessions <$H>] [--no-sessions] [--link <repo path>] [--out <file>]\n       node scripts/heldout-evidence.mjs --pending [--out <file>]");
    return args.help ? 0 : 2;
  }
  const out = resolve(args.out ?? OUT);
  let value;
  if (args.pending) value = pending();
  else {
    const path = resolve(args._[0]);
    const text = readFileSync(path, "utf8");
    const report = JSON.parse(text);
    const record = path.replace(/\.json$/, ".md");
    const inRepo = (p) => (p.startsWith(REPO + "/") ? relative(REPO, p) : null);
    const guess = dirname(dirname(path));
    const sessions = args.noSessions ? null : args.sessions ? resolve(args.sessions) : existsSync(join(guess, "sessions")) ? guess : null;
    if (sessions) console.log(`tokens and per-block hidden-test results from ${join(sessions, "sessions")} (read only)`);
    // The exact report and decision record, published beside the page's file.
    copyFileSync(path, join(dirname(out), "heldout-report.json"));
    const hasRecord = existsSync(record);
    if (hasRecord) copyFileSync(record, join(dirname(out), "heldout-record.md"));
    value = convert(report, {
      tokens: sessions ? sessionTokens(sessions) : null,
      failures: sessions ? sessionFailures(sessions) : null,
      source: {
        report: "/evidence/heldout-report.json",
        report_sha256: createHash("sha256").update(text).digest("hex"),
        decision_record: hasRecord ? "/evidence/heldout-record.md" : null,
        archived: args.link ?? inRepo(path),
        converted_at: new Date().toISOString(),
        converter: "web/site/scripts/heldout-evidence.mjs",
      },
    });
  }
  writeFileSync(out, JSON.stringify(value, null, 2) + "\n");
  const r = value.result;
  const m = r?.modes;
  console.log(r ? `${out}: ${r.verdict} (${r.outcome})${m ? `; advisory ${m.advisory?.ratio}× (upper ${m.advisory?.upper}), execution ${m.execution?.ratio}× (upper ${m.execution?.upper}, ${m.execution?.status})` : ""}` : `${out}: pending`);
  if (r && !r.valid) console.error("The report is INVALID (a lapsed approval): the page says so and shows no verdict.");
  return 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)));
