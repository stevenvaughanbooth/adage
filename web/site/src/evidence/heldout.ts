// The held-out result as the pages show it, from public/evidence/heldout.json (written by
// scripts/heldout-evidence.mjs from the report `heldout_cohort.py report` produces). Pure string
// rendering, so the build puts the same markup in the pages (vite.config.ts) that they redraw when
// they read the file (src/evidence/load.ts): it reads without JavaScript, and a new result drops in
// from the data alone.
//
// One study, two modes as Adage ships them. Advisory mode (the default: nothing runs, no approvals)
// against the agent alone is the analysis's pre-registered report-only comparison, and it leads.
// Execution mode (opt-in: person-approved CPU test runs) against the agent alone is the study's gated
// test; its registered outcome is shown as it is, under the advisory result. A report that reaches
// no verdict (unverified, a protocol failure, incomplete, or invalid) shows no headline number.

export interface Contrast {
  ratio: number | null;
  lower: number | null;
  upper: number | null;
  blocks: number | null;
  blocks_fewer: number | null;
  sign_flip_p: number | null;
  passed: boolean;
  excluded_blocks?: number[];
}

export interface ArmTokens { sessions: number; input: number; output: number; cache_read: number; cache_write: number; total: number; per_session: number | null }

export interface Arm {
  label: string;
  sessions: number | null;
  mean_requests: number | null;
  requests_included: number | null;
  observed_requests: number | null;
  hidden_test_passes: number | null;
  censored: number | null;
  verification_requests: number | null;
  cost_usd: number | null;
  cost_per_session_usd: number | null;
  tokens: ArmTokens | null;
}

type Block = { block: number; task: string };

export interface Mode {
  arm: "NOEXEC" | "BEST";
  role: string;
  registered: string;
  ratio: number | null;
  lower: number | null;
  upper: number | null;
  threshold: number;
  clears_line: boolean;
  fewer_requests_percent: number | null;
  blocks: number | null;
  blocks_fewer: number | null;
  sign_flip_p: number | null;
  mean_requests: number | null;
  mean_requests_alone: number | null;
  cost_per_session_usd: number | null;
  cost_per_session_alone_usd: number | null;
  cost_change_percent: number | null;
  hidden_test_passes: number | null;
  hidden_test_passes_alone: number | null;
  of: number | null;
  failed_blocks: Block[] | null;
  failed_blocks_alone: Block[] | null;
  failures_shared_by_all_arms: Block[] | null;
  status: string;
  outcome?: string;
  quality_rule_passed?: boolean;
  failed_criteria?: string[];
}

export interface Study {
  name: string;
  repository: string;
  model: string;
  tasks: string[];
  repetitions: number;
  blocks: number;
  sessions: number;
  controls: string[];
  arms: Record<string, string>;
  pass_rule: { threshold: number; confidence: number; quality_margin: number; text: string };
  frozen: string;
  protocol: { analysis: string; revisions: string; document: string; code: string; tasks: string };
}

export interface Result {
  outcome: "pass" | "fail" | "unverified" | "protocol-failure" | "incomplete";
  valid: boolean;
  verdict: "GO" | "NO-GO" | "NO VERDICT";
  label: string;
  headline: string;
  best_arm: string | null;
  primary: (Contrast & { threshold: number | null; included_blocks: number | null }) | null;
  quality: { best: number | null; a0: number | null; noexec: number | null; of: number | null; margin: number | null; passed: boolean } | null;
  arms: Partial<Record<"A0" | "NOEXEC" | "BEST", Arm>>;
  tokens_converted: boolean;
  modes: { advisory: Mode | null; execution: Mode | null } | null;
  criteria: { id: string; applies: boolean; passed: boolean | null; rule: string }[];
  controls: { condition: string; collected: boolean; realized: boolean; necessary_work_skipped: boolean }[];
  controls_passed: boolean;
  sensitivity: { without_censored_blocks: Contrast | null; without_has_keys: Contrast | null };
  report_only: { noexec_vs_a0: Contrast | null; best_vs_noexec: Contrast | null };
  excluded_blocks: number[];
  warnings: string[];
  problems: string[];
  lapses: { order: number; problem: string }[];
  assignments: number | null;
  collected: number | null;
  build_commit: string | null;
}

export interface Heldout {
  schema: string;
  status: "pending" | "result";
  study: Study;
  result: Result | null;
  source: { report: string; report_sha256: string; decision_record: string | null; archived?: string | null; converted_at: string; converter: string } | null;
}

export const SCHEMA = "adage-heldout-evidence-2";
export const REPO_URL = "https://github.com/stevenvaughanbooth/adage";
const blob = (path: string) => `${REPO_URL}/blob/main/${path}`;

const esc = (s: unknown) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const fixed = (v: number | null | undefined, d: number) => (typeof v === "number" ? v.toFixed(d) : "n/a");
const times = (v: number | null | undefined) => (typeof v === "number" ? `${v.toFixed(2)}×` : "n/a");
const usd = (v: number | null | undefined) => (typeof v === "number" ? `$${v.toFixed(3)}` : "n/a");
const kilo = (v: number | null | undefined) => (typeof v === "number" ? (v >= 1000 ? `${Math.round(v / 1000)}k` : String(Math.round(v))) : "n/a");
const of = (n: number | null | undefined, d: number | null | undefined) => `${n ?? "?"}/${d ?? "?"}`;
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/** What each registered G criterion missing means, in plain words (heldout_analysis.GO_CRITERIA). */
const CRITERIA: Record<string, string> = {
  G1: "the learned route was not in place",
  G2: "the learned check did not run, or pass, often enough",
  G5: "it had more hidden-test failures than the agent alone",
  G6: "a safety rule was broken",
  G7: "too many of its runs were unnecessary",
  G8: "it did no better than advisory mode",
};

/** A verdict on the study's own terms: decided (the registered test came out) or not. */
export const decided = (r: Result) => r.valid && (r.outcome === "pass" || r.outcome === "fail") && !!r.modes?.advisory && !!r.modes.execution;

/** "a third", "half", "a quarter", or a percentage: plain words only where they are exact to a point. */
function share(percent: number) {
  const p = Math.abs(percent);
  if (Math.abs(p - 100 / 3) < 1) return "a third";
  if (Math.abs(p - 50) < 1) return "half";
  if (Math.abs(p - 25) < 1) return "a quarter";
  return `${Math.round(p)}%`;
}

/** The pending line, word for word the same on every page. */
export function pendingLine(s: Study) {
  return `Held-out comparison running: ${s.tasks.length} new tasks, ${s.blocks} paired blocks; pass rule: ${Math.round(s.pass_rule.confidence * 100)}% upper bound ≤ ${s.pass_rule.threshold.toFixed(2)}× requests with no quality loss.`;
}

function correctness(m: Mode) {
  const a = m.hidden_test_passes, b = m.hidden_test_passes_alone;
  if (a === null || b === null) return "";
  if (a === b) return "with the same correctness";
  if (a > b) return "with more hidden tests passing";
  return `but ${b - a} fewer hidden-test ${b - a === 1 ? "pass" : "passes"}`;
}

/** The advisory result in one sentence (the headline). */
export function advisorySentence(m: Mode) {
  if (m.ratio === null) return "Advisory mode: no comparison.";
  if (m.status === "no-clear-saving") return `Advisory mode: no clear saving in model calls on held-out tasks (${times(m.ratio)} the agent alone's; cautious estimate ${times(m.upper)}).`;
  const cost = m.cost_change_percent === null ? "" : ` and ${share(m.cost_change_percent)} ${m.cost_change_percent < 0 ? "lower" : "higher"} cost`;
  return `About ${m.fewer_requests_percent}% fewer model calls${cost} on held-out tasks, ${correctness(m)}.`;
}

/** Execution mode's status, in words a visitor reads first. */
function executionStatus(m: Mode): { flag: string; line: string } {
  const tests = `hidden tests ${of(m.hidden_test_passes, m.of)} against ${of(m.hidden_test_passes_alone, m.of)} for the agent alone`;
  switch (m.status) {
    case "passed":
      return { flag: "Passed its registered test", line: `It passed the study's registered test: ${times(m.ratio)} the agent alone's model calls (cautious estimate ${times(m.upper)}), ${tests}.` };
    case "experimental-quality":
      return { flag: "Experimental · quality bar not yet met", line: `More savings in testing, but not yet proven to keep quality: ${times(m.ratio)} the agent alone's model calls (cautious estimate ${times(m.upper)}), with ${tests}. That misses the registered quality rule, so the study's verdict for execution mode is that it did not pass.` };
    case "experimental-savings":
      return { flag: "Experimental · savings not shown", line: `Its cautious estimate, ${times(m.upper)} the agent alone's model calls, does not clear the ${times(m.threshold)} line (${tests}), so the study's verdict for execution mode is that it did not pass.` };
    default: {
      const why = (m.failed_criteria ?? []).map((g) => `${CRITERIA[g] ?? "a registered criterion did not hold"} (${g})`).join("; ");
      return { flag: "Experimental · registered test not passed", line: `${times(m.ratio)} the agent alone's model calls, but ${why || "a registered criterion did not hold"}, so the study's verdict for execution mode is that it did not pass.` };
    }
  }
}

/** The ratio on a scale: the estimate, its 95% interval, the line and "the same as without". */
function scale(m: Mode) {
  const lo = 0.3, hi = 1.3;
  const at = (v: number) => `${(((Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo)) * 100).toFixed(2)}%`;
  const ratio = m.ratio ?? 1, l = m.lower ?? ratio, u = m.upper ?? ratio, t = m.threshold;
  const aria = `Advisory mode: request ratio ${times(m.ratio)}, 95% interval ${times(m.lower)} to ${times(m.upper)}; the line is ${times(t)}, and 1.00× is the same as without Adage.`;
  return `<div class="ev-scale">
  <div class="ev-scale-track" role="img" aria-label="${esc(aria)}">
    <span class="ev-scale-same" style="left:${at(1)}"><i>1.00× same as alone</i></span>
    <span class="ev-scale-line" style="left:${at(t)}"><i>the line ${times(t)}</i></span>
    <span class="ev-scale-ci${u <= t ? " ok" : " miss"}" style="left:${at(l)};width:calc(${at(u)} - ${at(l)})"></span>
    <span class="ev-scale-dot" style="left:${at(ratio)}"><b>${times(m.ratio)}</b></span>
  </div>
  <p class="ev-scale-cap">The dot is advisory mode's model calls for each one the agent alone needed; the bar is where the true value likely sits (95%). It clears only if the whole bar is left of the line.</p>
</div>`;
}

const yes = (v: boolean | null) => (v === true ? "yes" : v === false ? "no" : "n/a");

function contrastRow(name: string, c: Contrast | null) {
  if (!c) return "";
  return `<tr><th scope="row">${esc(name)}</th><td>${times(c.ratio)}</td><td>${times(c.lower)} to ${times(c.upper)}</td><td>${of(c.blocks_fewer, c.blocks)}</td><td>${fixed(c.sign_flip_p, 4)}</td><td>${yes(c.passed)}</td></tr>`;
}

const blocks = (b: Block[] | null) => (b === null ? "not read" : b.length ? b.map((x) => `${x.block} (${x.task.replace(/^mm-curve--/, "")})`).join(", ") : "none");

function detail(r: Result, h: Heldout) {
  const s = h.study;
  const arms = (["A0", "NOEXEC", "BEST"] as const).filter((a) => r.arms[a]);
  const criteria = r.criteria.map((c) => `<tr><th scope="row">${esc(c.id)}</th><td>${c.applies ? "applies" : "not applicable"}</td><td>${c.applies ? yes(c.passed) : "–"}</td><td>${esc(c.rule)}</td></tr>`).join("");
  const name = { A0: "The agent alone", NOEXEC: "Advisory mode (default)", BEST: "Execution mode (opt-in)" } as const;
  const armRows = arms.map((a) => {
    const x = r.arms[a]!;
    return `<tr><th scope="row">${name[a]}</th><td>${fixed(x.mean_requests, 2)}</td><td>${x.requests_included ?? "n/a"}</td><td>${of(x.hidden_test_passes, x.sessions)}</td><td>${x.censored ?? "n/a"}</td><td>${x.verification_requests ?? "n/a"}</td><td>${usd(x.cost_per_session_usd)}</td><td>${x.tokens ? kilo(x.tokens.per_session) : "not converted"}</td></tr>`;
  }).join("");
  const adv = r.modes?.advisory ?? null, ex = r.modes?.execution ?? null;
  const fails = adv && ex && ex.failed_blocks !== null
    ? `<p>Blocks whose hidden tests failed: the agent alone ${blocks(ex.failed_blocks_alone)}; advisory mode ${blocks(adv.failed_blocks)}; execution mode ${blocks(ex.failed_blocks)}. Failed in all three arms: ${blocks(ex.failures_shared_by_all_arms)}.</p>`
    : "";
  const verdict = ex ? `<p>The study ran three arms on the same ${s.tasks.length} tasks, ${s.repetitions} times each, in ${s.blocks} paired blocks: the agent alone, advisory mode and execution mode. Its pass/fail test was registered for execution mode against the agent alone (the gated row below); the advisory comparison was registered to be reported beside it, with the same statistics. The study's registered outcome for execution mode: <b>${r.outcome === "pass" ? "pass" : "did not pass"}</b>${r.outcome === "pass" ? "" : ` (criteria not met: ${esc((ex.failed_criteria ?? []).join(", ") || "see below")})`}.</p>` : "";
  return `<details class="ev-more">
  <summary>More detail: every registered number</summary>
  <div class="ev-more-body">
    ${verdict}
    <h4>Model calls, with and without</h4>
    <div class="ev-table" role="region" aria-label="Request ratios" tabindex="0"><table>
      <thead><tr><th scope="col">Comparison</th><th scope="col">Ratio</th><th scope="col">95% range</th><th scope="col">Blocks fewer</th><th scope="col">Sign-flip p</th><th scope="col">Under 0.90×</th></tr></thead>
      <tbody>${contrastRow("Advisory / alone (registered, reported)", r.report_only.noexec_vs_a0)}${contrastRow("Execution / alone (gated)", r.primary)}${contrastRow("Execution / alone, without censored blocks", r.sensitivity.without_censored_blocks)}${contrastRow("Execution / alone, without the has-keys task", r.sensitivity.without_has_keys)}${contrastRow("Execution / advisory (reported)", r.report_only.best_vs_noexec)}</tbody>
    </table></div>
    <h4>Each arm</h4>
    <div class="ev-table" role="region" aria-label="Each arm" tabindex="0"><table>
      <thead><tr><th scope="col">Arm</th><th scope="col">Mean requests</th><th scope="col">Requests counted</th><th scope="col">Runs passing hidden tests</th><th scope="col">Censored</th><th scope="col">Test-run requests</th><th scope="col">Cost / session</th><th scope="col">Tokens / session</th></tr></thead>
      <tbody>${armRows}</tbody>
    </table></div>
    ${fails}
    <h4>Execution mode's G criteria</h4>
    <div class="ev-table" role="region" aria-label="G criteria" tabindex="0"><table>
      <thead><tr><th scope="col">Criterion</th><th scope="col">Applies</th><th scope="col">Held</th><th scope="col">Rule, or why it does not apply</th></tr></thead>
      <tbody>${criteria}</tbody>
    </table></div>
    <p class="ev-fine">Blocks left out for an unverified request count: ${r.excluded_blocks.length ? esc(r.excluded_blocks.join(", ")) : "none"} (at most 2 allowed). A session that stopped early counts as a quality failure in every arm, and only execution mode's is raised to 60 requests. Assignments ${r.assignments ?? "?"}, collected ${r.collected ?? "?"}. Cost is the API list price. ${r.problems.map((p) => esc(p)).join(" ")}</p>
    <p class="ev-fine">Pass rule as registered: ${esc(s.pass_rule.text)}</p>
  </div>
</details>`;
}

function sourceLine(h: Heldout) {
  const src = h.source;
  const parts = [
    `<a href="${blob(h.study.protocol.document)}">The protocol</a>`,
    `<a href="${blob(h.study.protocol.code)}">the analysis code</a>`,
    `<a href="${blob(h.study.protocol.tasks)}">the frozen tasks</a>`,
  ];
  if (src) {
    const href = (p: string) => (p.startsWith("/") ? p : p.includes("/") ? blob(p) : null);
    const link = (p: string, text: string) => (href(p) ? `<a href="${href(p)}">${text}</a>` : `<span>${text}</span>`);
    if (src.decision_record) parts.push(link(src.decision_record, "the decision record"));
    parts.push(`${link(src.report, "the report")} (sha256 <code>${esc(src.report_sha256.slice(0, 12))}…</code>)`);
  }
  return `<p class="ev-src">${parts.join(" · ")}</p>`;
}

function noVerdict(r: Result, h: Heldout) {
  const reasons = [...r.lapses.map((l) => `Session ${l.order}: ${l.problem}`), ...r.problems];
  return `<div class="ev-ho" data-state="none">
  <p class="ev-ho-flag"><span class="ev-verdict">${esc(r.verdict)}</span>${esc(r.label)}</p>
  <h3 class="ev-ho-h">${esc(r.headline)}</h3>
  <p class="ev-ho-plain">The registered rules say when a result counts. This one does not, so we draw no conclusion from it, either way. The numbers it computed stay below, marked as what they are.</p>
  ${reasons.length ? `<ul class="ev-ho-warn">${reasons.map((x) => `<li>${esc(x)}</li>`).join("")}</ul>` : ""}
  ${r.primary ? detail(r, h) : ""}
  ${sourceLine(h)}
</div>`;
}

/** The whole held-out component (the evidence page's results). */
export function renderHeldout(h: Heldout): string {
  const s = h.study, r = h.result;
  if (h.status !== "result" || !r) {
    return `<div class="ev-ho" data-state="pending">
  <p class="ev-ho-flag"><span class="ev-pulse" aria-hidden="true"></span>Running now</p>
  <h3 class="ev-ho-h">${esc(pendingLine(s))}</h3>
  <p class="ev-ho-plain">${esc(s.frozen)} The result lands here as it comes out, with its decision record.</p>
  ${sourceLine(h)}
</div>`;
  }
  if (!decided(r)) return noVerdict(r, h);
  const adv = r.modes!.advisory!, ex = r.modes!.execution!;
  const a0 = r.arms.A0, nx = r.arms.NOEXEC;
  const controlsKept = r.controls.filter((c) => c.realized && !c.necessary_work_skipped).length;
  const tokens = r.tokens_converted && a0?.tokens && nx?.tokens
    ? `<div><dt>Tokens per session</dt><dd><span class="was">${kilo(a0.tokens.per_session)}</span><span class="arrow" aria-hidden="true">→</span><span class="now">${kilo(nx.tokens.per_session)}</span><small>alone → with Adage, every token counted</small></dd></div>`
    : `<div><dt>Tokens per session</dt><dd><span class="now ev-na">n/a</span><small>not converted with this report</small></dd></div>`;
  const x = executionStatus(ex);
  const n = ex.failures_shared_by_all_arms?.length ?? 0;
  const shared = ex.status === "experimental-quality" && n ? ` ${n === 1 ? "One" : n} of its failures came in ${n === 1 ? "a block" : "blocks"} where all three arms failed.` : "";
  return `<div class="ev-ho" data-state="result" data-advisory="${esc(adv.status)}" data-execution="${esc(ex.status)}">
  <p class="ev-ho-flag"><span class="ev-verdict">Held-out</span>Pre-registered comparison · ${s.tasks.length} new tasks · advisory mode, the default</p>
  <h3 class="ev-ho-h">${esc(advisorySentence(adv))}</h3>
  <p class="ev-ho-plain">On ${s.tasks.length} tasks Adage had never seen, run ${s.repetitions} times each, the agent with Adage in advisory mode needed ${fixed(adv.mean_requests, 1)} model calls per session against ${fixed(adv.mean_requests_alone, 1)} alone, fewer in ${of(adv.blocks_fewer, adv.blocks)} paired blocks. ${adv.clears_line ? `Even the cautious estimate, ${times(adv.upper)}, clears the ${times(adv.threshold)} line.` : `The cautious estimate, ${times(adv.upper)}, does not clear the ${times(adv.threshold)} line.`} Hidden tests passed ${of(adv.hidden_test_passes, adv.of)}, against ${of(adv.hidden_test_passes_alone, adv.of)} alone. Nothing ran on the agent's behalf, and no execution approval was involved.</p>
  ${scale(adv)}
  <dl class="ev-ho-stats">
    <div><dt>Model calls per session</dt><dd><span class="was">${fixed(adv.mean_requests_alone, 1)}</span><span class="arrow" aria-hidden="true">→</span><span class="now">${fixed(adv.mean_requests, 1)}</span><small>alone → with Adage, over ${adv.blocks ?? "?"} blocks</small></dd></div>
    <div><dt>Blocks with fewer calls</dt><dd><span class="now">${of(adv.blocks_fewer, adv.blocks)}</span><small>same task, same repetition</small></dd></div>
    <div><dt>Runs passing hidden tests</dt><dd><span class="now">${of(adv.hidden_test_passes, adv.of)}</span><small>with Adage · ${of(adv.hidden_test_passes_alone, adv.of)} alone</small></dd></div>
    <div><dt>Cost per session</dt><dd><span class="was">${usd(adv.cost_per_session_alone_usd)}</span><span class="arrow" aria-hidden="true">→</span><span class="now">${usd(adv.cost_per_session_usd)}</span><small>API list price${adv.cost_change_percent === null ? "" : ` · ${fixed(Math.abs(adv.cost_change_percent), 1)}% ${adv.cost_change_percent < 0 ? "lower" : "higher"}`}</small></dd></div>
    ${tokens}
    <div><dt>Safety controls</dt><dd><span class="now">${controlsKept}/${s.controls.length}</span><small>kept every necessary check</small></dd></div>
  </dl>
  <div class="ev-exec" data-status="${esc(ex.status)}">
    <p class="ev-exec-flag"><span>Execution mode</span>${esc(x.flag)}</p>
    <p class="ev-exec-what">Opt-in: Adage also runs the person-approved tests itself, on your CPU, and checks the finish line before the agent stops.</p>
    <p class="ev-exec-line">${esc(x.line)}${esc(shared)} It stays opt-in.</p>
  </div>
  ${detail(r, h)}
  ${sourceLine(h)}
</div>`;
}

/** The evidence page's headline: the advisory result, or a plain line before there is one. */
export function renderHeldoutHeadline(h: Heldout): string {
  const r = h.result;
  const m = h.status === "result" && r && decided(r) ? r.modes!.advisory! : null;
  if (!m || m.status === "no-clear-saving") return `Fewer model calls, the same correct code. <span class="soft">Here is exactly how we test it.</span>`;
  const cost = m.cost_change_percent !== null && m.cost_change_percent < 0 ? ` ${cap(share(m.cost_change_percent))} lower cost.` : "";
  const same = m.hidden_test_passes !== null && m.hidden_test_passes_alone !== null && m.hidden_test_passes >= m.hidden_test_passes_alone;
  return `About ${m.fewer_requests_percent}% fewer model calls.${esc(cost)} <span class="soft">${same ? "The same correct code." : "Here is exactly what we found."}</span>`;
}

/** The running example: one dot per model call, the agent alone against advisory mode. */
export function renderHeldoutExample(h: Heldout): string {
  const r = h.result;
  if (h.status !== "result" || !r || !decided(r)) return "";
  const m = r.modes!.advisory!;
  return `<div class="ev-runs">
  <div class="ev-run">
    <p class="ev-run-h"><b>The agent alone</b><span><b>${fixed(m.mean_requests_alone, 1)}</b> calls per session</span></p>
    <p class="ev-dots" data-dots="${m.mean_requests_alone ?? ""}" aria-hidden="true"></p>
    <p class="ev-run-note">It looks around the repository to find where things go, runs the tests, reads the output, and often runs them again to be sure.</p>
  </div>
  <div class="ev-run adage">
    <p class="ev-run-h"><b>With Adage</b><span><b>${fixed(m.mean_requests, 1)}</b> calls per session</span></p>
    <p class="ev-dots" data-dots="${m.mean_requests ?? ""}" aria-hidden="true"></p>
    <p class="ev-run-note">A learned card says which files to touch and where each one stands right now, and commands go to the project's own environment the first time. The agent goes straight to the work.</p>
  </div>
</div>
<p class="ev-ex-foot">Averages over ${m.blocks ?? "?"} paired runs on ${h.study.tasks.length} held-out tasks, advisory mode. Hidden tests passed ${of(m.hidden_test_passes, m.of)} with Adage and ${of(m.hidden_test_passes_alone, m.of)} alone.</p>`;
}

/** One line: the held-out result, for /how and the evidence page's first screen. */
export function renderHeldoutBrief(h: Heldout): string {
  const s = h.study, r = h.result;
  const more = `<a class="ev-brief-link" href="/evidence#results">How we tested it →</a>`;
  if (h.status !== "result" || !r) return `<p class="ev-brief" data-state="pending"><span class="ev-pulse" aria-hidden="true"></span><span>${esc(pendingLine(s))}</span> ${more}</p>`;
  if (!decided(r)) return `<p class="ev-brief" data-state="none"><b>Held-out result: no verdict.</b> <span>${esc(r.headline)}</span> ${more}</p>`;
  const a = r.modes!.advisory!, e = r.modes!.execution!;
  const exec = e.status === "passed" ? `Execution mode (opt-in) passed its registered test too, at ${times(e.ratio)}.` : `Execution mode (opt-in) is experimental: ${times(e.ratio)}, but not yet proven to keep quality.`;
  return `<p class="ev-brief" data-state="${esc(a.status)}"><b>Held-out tasks, advisory mode:</b> <span>${fixed(a.mean_requests_alone, 1)} → ${fixed(a.mean_requests, 1)} model calls per session (${times(a.ratio)}), hidden tests ${of(a.hidden_test_passes, a.of)} against ${of(a.hidden_test_passes_alone, a.of)}. ${esc(exec)}</span> ${more}</p>`;
}

/** The evidence page's first look: advisory mode's held-out result in a few numbers, beside the mod's. */
export function renderHeldoutGlance(h: Heldout): string {
  const s = h.study, r = h.result;
  const head = `<p class="ev-gl-tag">Registered test · real open-source repository</p>
  <h3 class="ev-gl-h" id="gl-adv-h">Advisory mode, the default</h3>
  <p class="ev-gl-what">Adage tells the agent what it would otherwise go looking for. Nothing runs on its behalf.</p>`;
  const foot = `<p class="ev-gl-setting">${esc(s.repository)} · ${esc(s.model)} · ${s.tasks.length} held-out tasks × ${s.repetitions}</p>
  <a class="ev-gl-more" href="#results">The full result ↓</a>`;
  if (h.status !== "result" || !r) return `${head}<p class="ev-gl-plain"><span class="ev-pulse" aria-hidden="true"></span>${esc(pendingLine(s))}</p>${foot}`;
  if (!decided(r)) return `${head}<p class="ev-gl-plain">${esc(r.headline)}</p>${foot}`;
  const a = r.modes!.advisory!, e = r.modes!.execution!;
  const big = a.status === "no-clear-saving" || a.fewer_requests_percent === null
    ? `<p class="ev-gl-big"><b>${times(a.ratio)}</b> the agent alone's model calls</p>`
    : `<p class="ev-gl-big"><b>${a.fewer_requests_percent}%</b> fewer model calls</p>`;
  const cost = a.cost_change_percent === null ? "n/a" : `${Math.round(Math.abs(a.cost_change_percent))}% ${a.cost_change_percent < 0 ? "lower" : "higher"}`;
  return `${head}
  ${big}
  <dl class="ev-gl-stats">
    <div><dt>Model calls per session</dt><dd>${fixed(a.mean_requests_alone, 1)} → ${fixed(a.mean_requests, 1)}</dd></div>
    <div><dt>Cost per session</dt><dd>${cost}</dd></div>
    <div><dt>Runs passing hidden tests</dt><dd>${of(a.hidden_test_passes, a.of)}<small>${of(a.hidden_test_passes_alone, a.of)} alone</small></dd></div>
  </dl>
  <p class="ev-gl-exec"><b>Execution mode, opt-in:</b> ${esc(executionStatus(e).flag.toLowerCase())}.</p>
  ${foot}`;
}

/** /how's summary step: one session's model calls, both modes, from the held-out study. */
export function renderHeldoutPlug(h: Heldout): string {
  const r = h.result;
  if (h.status !== "result" || !r || !decided(r)) return renderHeldoutBrief(h);
  const a = r.modes!.advisory!, e = r.modes!.execution!;
  const x = executionStatus(e);
  return `<p class="plug-result"><b>${fixed(a.mean_requests_alone, 1)} → ${fixed(a.mean_requests, 1)}</b><span>model calls per session on ${h.study.tasks.length} held-out tasks, in advisory mode (the default): ${esc(advisorySentence(a).replace(/^About/, "about").replace(/ on held-out tasks/, ""))} Hidden tests passed ${of(a.hidden_test_passes, a.of)} with Adage and ${of(a.hidden_test_passes_alone, a.of)} alone.</span></p>
<p class="plug-exec"><b>Execution mode, ${esc(x.flag.toLowerCase())}:</b> ${fixed(e.mean_requests, 1)} calls per session, with hidden tests ${of(e.hidden_test_passes, e.of)}.</p>`;
}

/** Every held-out component, by the name pages use for it (`{{heldout:<kind>}}`, data-heldout). */
export const HELDOUT_KINDS: Record<string, (h: Heldout) => string> = {
  full: renderHeldout,
  brief: renderHeldoutBrief,
  glance: renderHeldoutGlance,
  headline: renderHeldoutHeadline,
  example: renderHeldoutExample,
  plug: renderHeldoutPlug,
};
