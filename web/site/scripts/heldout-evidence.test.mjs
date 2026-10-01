// node --test scripts/heldout-evidence.test.mjs: the held-out converter against synthetic reports
// from the registered analysis (e2e/fixtures/heldout, made by make.py there; not study results).
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { SCHEMA, STUDY, convert, pending, sessionFailures, sessionTokens } from "./heldout-evidence.mjs";

const here = (p) => fileURLToPath(new URL(p, import.meta.url));
const report = (name) => JSON.parse(readFileSync(here(`../e2e/fixtures/heldout/${name}.json`), "utf8"));

test("pending: the registered design and no result", () => {
  const p = pending();
  assert.equal(p.schema, SCHEMA);
  assert.equal(p.status, "pending");
  assert.equal(p.result, null);
  assert.equal(p.study.tasks.length, 4);
  assert.equal(p.study.blocks, 20);
  assert.equal(p.study.pass_rule.threshold, 0.9);
});

test("the shipped file is a converted held-out report, and its numbers are the report's", () => {
  const h = JSON.parse(readFileSync(here("../public/evidence/heldout.json"), "utf8"));
  const r = JSON.parse(readFileSync(here("../public/evidence/heldout-report.json"), "utf8"));
  assert.equal(h.schema, SCHEMA);
  assert.equal(h.status, "result");
  assert.equal(h.result.outcome, r.outcome);
  const a = h.result.modes.advisory, e = h.result.modes.execution;
  assert.equal(a.ratio, r.report_only.noexec_vs_a0.ratio);
  assert.equal(a.upper, r.report_only.noexec_vs_a0.upper);
  assert.equal(a.blocks_fewer, r.report_only.noexec_vs_a0.blocks_fewer);
  assert.equal(a.hidden_test_passes, r.arms.NOEXEC.hidden_test_passes);
  assert.equal(a.mean_requests, r.arms.NOEXEC.mean_requests_included);
  assert.equal(e.ratio, r.primary.ratio);
  assert.equal(e.hidden_test_passes, r.quality.best);
  assert.equal(a.cost_change_percent, Math.round((r.arms.NOEXEC.cost_usd / r.arms.A0.cost_usd - 1) * 1000) / 10);
  assert.equal(a.fewer_requests_percent, Math.round((1 - r.report_only.noexec_vs_a0.ratio) * 100));
});

test("pass: GO, with the report's own ratio, bound, quality and controls", () => {
  const r = report("pass"), h = convert(r).result;
  assert.equal(h.verdict, "GO");
  assert.equal(h.outcome, "pass");
  assert.equal(h.primary.ratio, r.primary.ratio);
  assert.equal(h.primary.upper, r.primary.upper);
  assert.ok(h.primary.upper <= STUDY.pass_rule.threshold);
  assert.equal(h.quality.best, r.quality.best);
  assert.equal(h.quality.noexec, r.arms.NOEXEC.hidden_test_passes);
  assert.equal(h.arms.BEST.mean_requests, r.arms.BEST.mean_requests_included);
  assert.equal(h.arms.A0.cost_per_session_usd, Math.round((r.arms.A0.cost_usd / r.arms.A0.sessions) * 1e6) / 1e6);
  assert.equal(h.controls.length, 5);
  assert.equal(h.controls_passed, true);
  assert.deepEqual(h.criteria.map((c) => c.id), ["G1", "G2", "G3", "G4", "G5", "G6", "G7", "G8", "G9"]);
  assert.equal(h.sensitivity.without_has_keys.blocks, r.sensitivity.without_has_keys.blocks);
  assert.equal(h.tokens_converted, false);
});

test("pass: both modes clear, execution mode passed", () => {
  const m = convert(report("pass")).result.modes;
  assert.equal(m.advisory.status, "clear");
  assert.equal(m.advisory.arm, "NOEXEC");
  assert.equal(m.execution.status, "passed");
});

test("a mixed fail: advisory clears; execution is experimental on quality, and the outcome is kept", () => {
  const h = convert(report("fail")).result;
  assert.equal(h.verdict, "NO-GO");
  assert.equal(h.outcome, "fail");
  assert.equal(h.quality.passed, false);
  assert.equal(h.modes.advisory.status, "clear");
  assert.equal(h.modes.advisory.hidden_test_passes, h.modes.advisory.hidden_test_passes_alone);
  assert.equal(h.modes.execution.status, "experimental-quality");
  assert.deepEqual(h.modes.execution.failed_criteria, ["G5"]);
  assert.equal(h.modes.execution.failed_blocks, null);
});

test("a fail on savings: no clear advisory saving either", () => {
  const m = convert(report("fail-savings")).result.modes;
  assert.equal(m.advisory.status, "no-clear-saving");
  assert.equal(m.execution.status, "experimental-savings");
});

test("unverified and invalid: no verdict, and no modes to headline", () => {
  assert.equal(convert(report("unverified")).result.verdict, "NO VERDICT");
  assert.equal(convert(report("unverified")).result.modes, null);
  const invalid = convert({ ...report("pass"), valid: false, lapses: [{ order: 9, problem: "withheld: approval" }] }).result;
  assert.equal(invalid.verdict, "NO VERDICT");
  assert.equal(invalid.valid, false);
  assert.equal(invalid.modes, null);
  assert.deepEqual(invalid.lapses, [{ order: 9, problem: "withheld: approval" }]);
});

test("incomplete: no verdict, and nothing invented", () => {
  const h = convert({ protocol: "heldout-analysis-1", outcome: "incomplete", problems: ["incomplete blocks: [3]"], controls: [], valid: true, lapses: [] }).result;
  assert.equal(h.verdict, "NO VERDICT");
  assert.equal(h.primary, null);
  assert.equal(h.quality, null);
});

test("refuses what is not a held-out report", () => {
  assert.throws(() => convert({}), /no outcome/);
  assert.throws(() => convert({ outcome: "maybe" }), /unknown outcome/);
  assert.throws(() => convert({ outcome: "pass", protocol: "dev-arms-2" }), /unexpected analysis protocol/);
});

test("tokens: summed from the positive sessions' own summaries", () => {
  const root = mkdtempSync(join(tmpdir(), "heldout-"));
  const write = (name, usage) => { mkdirSync(join(root, "sessions", name), { recursive: true }); writeFileSync(join(root, "sessions", name, "summary.json"), JSON.stringify({ usage })); };
  write("05-A0-b01-has-keys", { input_tokens: 10, output_tokens: 100, cache_read_input_tokens: 1000, cache_creation_input_tokens: 50 });
  write("06-BEST-b01-has-keys", { input_tokens: 5, output_tokens: 60, cache_read_input_tokens: 500, cache_creation_input_tokens: 40 });
  write("00-BEST-failed", { input_tokens: 999, output_tokens: 999, cache_read_input_tokens: 999, cache_creation_input_tokens: 999 });
  const t = sessionTokens(root);
  assert.deepEqual(t.A0, { sessions: 1, input: 10, output: 100, cache_read: 1000, cache_write: 50, total: 1160, per_session: 1160 });
  assert.equal(t.BEST.total, 605);
  assert.equal(t.NOEXEC.sessions, 0);
  const h = convert(report("pass"), { tokens: t }).result;
  assert.equal(h.tokens_converted, true);
  assert.equal(h.arms.BEST.tokens.per_session, 605);
});

test("the command writes the page's file and names its source", () => {
  const dir = mkdtempSync(join(tmpdir(), "heldout-cli-"));
  const out = join(dir, "heldout.json");
  const src = join(dir, "heldout-1.json");
  writeFileSync(src, readFileSync(here("../e2e/fixtures/heldout/fail.json")));
  writeFileSync(join(dir, "heldout-1.md"), "# record\n");
  execFileSync(process.execPath, [here("./heldout-evidence.mjs"), src, "--out", out]);
  const h = JSON.parse(readFileSync(out, "utf8"));
  assert.equal(h.result.verdict, "NO-GO");
  assert.equal(h.source.report, "/evidence/heldout-report.json");
  assert.equal(h.source.decision_record, "/evidence/heldout-record.md");
  // The exact files are published beside it.
  assert.equal(readFileSync(join(dir, "heldout-report.json"), "utf8"), readFileSync(src, "utf8"));
  assert.equal(readFileSync(join(dir, "heldout-record.md"), "utf8"), "# record\n");
  assert.match(h.source.report_sha256, /^[0-9a-f]{64}$/);
});

test("failures by block: which arms failed where, and the blocks every arm failed", () => {
  const root = mkdtempSync(join(tmpdir(), "heldout-fail-"));
  const write = (name, passed) => { mkdirSync(join(root, "sessions", name), { recursive: true }); writeFileSync(join(root, "sessions", name, "collection.json"), JSON.stringify({ task: "mm-curve--has-keys", quality: { passed } })); };
  write("05-BEST-b01-has-keys", false); write("06-NOEXEC-b01-has-keys", false); write("07-A0-b01-has-keys", false);
  write("08-A0-b02-has-keys", true); write("09-BEST-b02-has-keys", false); write("10-NOEXEC-b02-has-keys", true);
  write("00-BEST-failed", false);
  const f = sessionFailures(root);
  assert.deepEqual(f.BEST.map((b) => b.block), [1, 2]);
  assert.deepEqual(f.A0.map((b) => b.block), [1]);
  const m = convert(report("fail"), { failures: f }).result.modes;
  assert.deepEqual(m.execution.failures_shared_by_all_arms, [{ block: 1, task: "mm-curve--has-keys" }]);
  assert.deepEqual(m.execution.failed_blocks_alone, [{ block: 1, task: "mm-curve--has-keys" }]);
});

test("the command finds the sessions beside the report, as heldout_cohort.py lays them out", () => {
  const root = mkdtempSync(join(tmpdir(), "heldout-root-"));
  mkdirSync(join(root, "reports"));
  mkdirSync(join(root, "sessions", "07-BEST-b01-iso-date"), { recursive: true });
  writeFileSync(join(root, "sessions", "07-BEST-b01-iso-date", "summary.json"), JSON.stringify({ usage: { input_tokens: 1, output_tokens: 2, cache_read_input_tokens: 3, cache_creation_input_tokens: 4 } }));
  const src = join(root, "reports", "heldout-2.json");
  writeFileSync(src, readFileSync(here("../e2e/fixtures/heldout/pass.json")));
  const out = join(root, "heldout.json");
  execFileSync(process.execPath, [here("./heldout-evidence.mjs"), src, "--out", out]);
  const h = JSON.parse(readFileSync(out, "utf8"));
  assert.equal(h.result.tokens_converted, true);
  assert.equal(h.result.arms.BEST.tokens.total, 10);
  execFileSync(process.execPath, [here("./heldout-evidence.mjs"), src, "--out", out, "--no-sessions"]);
  assert.equal(JSON.parse(readFileSync(out, "utf8")).result.tokens_converted, false);
});
