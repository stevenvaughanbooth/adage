// The held-out result as the pages show it, from public/evidence/heldout.json (written by
// scripts/heldout-evidence.mjs from the report `heldout_cohort.py report` produces). Pure string
// rendering, so the build can put the same markup in the page (vite.config.ts) that the page
// redraws when it reads the file (src/evidence/load.ts): it reads without JavaScript, and a new
// result drops in from the data alone.

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
  source: { report: string; report_sha256: string; decision_record: string | null; converted_at: string; converter: string } | null;
}

export const REPO_URL = "https://github.com/stevenvaughanbooth/adage";
const blob = (path: string) => `${REPO_URL}/blob/main/${path}`;

const esc = (s: unknown) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
const fixed = (v: number | null | undefined, d: number) => (typeof v === "number" ? v.toFixed(d) : "n/a");
const times = (v: number | null | undefined) => (typeof v === "number" ? `${v.toFixed(2)}×` : "n/a");
const usd = (v: number | null | undefined) => (typeof v === "number" ? `$${v.toFixed(3)}` : "n/a");
const kilo = (v: number | null | undefined) => (typeof v === "number" ? (v >= 1000 ? `${Math.round(v / 1000)}k` : String(Math.round(v))) : "n/a");
const of = (n: number | null | undefined, d: number | null | undefined) => `${n ?? "?"}/${d ?? "?"}`;
const CONTROL_WORDS: Record<string, string> = {
  failed: "a failing test",
  "later-edit": "an edit after the check",
  "config-change": "a changed config",
  broader: "a broader change",
  rerun: "an explicit request to rerun",
};

/** What each registered G criterion missing means, in plain words (heldout_analysis.GO_CRITERIA). */
const CRITERIA: Record<string, string> = {
  G1: "the learned route was not in place",
  G2: "the learned check did not run often enough, or did not pass often enough",
  G5: "Adage had more hidden-test failures than an arm without it",
  G6: "a safety rule was broken",
  G7: "too many of Adage's runs were unnecessary",
  G8: "it did no better than Adage's hooks without the approved check",
};

/** A verdict on the study's own terms: decided (GO or NO-GO) or not (no verdict). */
export const decided = (r: Result) => r.valid && (r.outcome === "pass" || r.outcome === "fail");

/** The pending line, word for word the same on every page. */
export function pendingLine(s: Study) {
  return `Held-out comparison running: ${s.tasks.length} new tasks, ${s.blocks} paired blocks; pass rule: ${Math.round(s.pass_rule.confidence * 100)}% upper bound ≤ ${s.pass_rule.threshold.toFixed(2)}× requests with no quality loss.`;
}

/** The plain sentence under a decided result. */
function plain(r: Result, s: Study) {
  const p = r.primary!, q = r.quality;
  const per100 = typeof p.ratio === "number" ? Math.round(p.ratio * 100) : null;
  const quality = q ? `Hidden tests passed: ${of(q.best, q.of)} with Adage, ${of(q.a0, q.of)} for the agent alone.` : "";
  if (r.outcome === "pass") {
    return `On ${s.tasks.length} tasks it had never seen, the agent with Adage needed ${times(p.ratio)} the model requests of the agent alone (about ${per100} for every 100), and even the cautious estimate, ${times(p.upper)}, clears the ${times(p.threshold)} line. ${quality}`;
  }
  const failed = r.criteria.filter((c) => c.applies && c.passed === false).map((c) => `${CRITERIA[c.id] ?? "a registered criterion did not hold"} (${c.id})`);
  const why = [
    typeof p.upper === "number" && typeof p.threshold === "number" && p.upper > p.threshold ? `the cautious estimate, ${times(p.upper)}, does not clear the ${times(p.threshold)} line` : "",
    q && !q.passed ? "quality fell by more than the allowed margin" : "",
    ...failed,
  ].filter(Boolean);
  return `On ${s.tasks.length} tasks it had never seen, the agent with Adage needed ${times(p.ratio)} the model requests of the agent alone, but ${why.join("; ") || "a registered criterion did not hold"}. ${quality} We publish it as it came out.`;
}

/** The ratio on a scale: the estimate, its 95% interval, the pass line and "the same as without". */
function scale(p: NonNullable<Result["primary"]>) {
  const lo = 0.3, hi = 1.3;
  const at = (v: number) => `${(((Math.min(hi, Math.max(lo, v)) - lo) / (hi - lo)) * 100).toFixed(2)}%`;
  const ratio = p.ratio ?? 1, l = p.lower ?? ratio, u = p.upper ?? ratio, t = p.threshold ?? 0.9;
  const label = `Request ratio ${times(p.ratio)}, 95% interval ${times(p.lower)} to ${times(p.upper)}; the pass line is ${times(t)}, and 1.00× is the same as without Adage.`;
  return `<div class="ev-scale">
  <div class="ev-scale-track" role="img" aria-label="${esc(label)}">
    <span class="ev-scale-same" style="left:${at(1)}"><i>1.00× same as alone</i></span>
    <span class="ev-scale-line" style="left:${at(t)}"><i>pass line ${times(t)}</i></span>
    <span class="ev-scale-ci${u <= t ? " ok" : " miss"}" style="left:${at(l)};width:calc(${at(u)} - ${at(l)})"></span>
    <span class="ev-scale-dot" style="left:${at(ratio)}"><b>${times(p.ratio)}</b></span>
  </div>
  <p class="ev-scale-cap">The dot is Adage's requests per 100 of the agent alone's; the bar is where the true value likely sits (95%). It passes only if the whole bar is left of the line.</p>
</div>`;
}

const yes = (v: boolean | null) => (v === true ? "yes" : v === false ? "no" : "n/a");

function contrastRow(name: string, c: Contrast | null) {
  if (!c) return "";
  return `<tr><th scope="row">${esc(name)}</th><td>${times(c.ratio)}</td><td>${times(c.lower)} to ${times(c.upper)}</td><td>${of(c.blocks_fewer, c.blocks)}</td><td>${fixed(c.sign_flip_p, 4)}</td><td>${yes(c.passed)}</td></tr>`;
}

function detail(r: Result, h: Heldout) {
  const s = h.study;
  const arms = (["A0", "NOEXEC", "BEST"] as const).filter((a) => r.arms[a]);
  const criteria = r.criteria.map((c) => `<tr><th scope="row">${esc(c.id)}</th><td>${c.applies ? "applies" : "not applicable"}</td><td>${c.applies ? yes(c.passed) : "–"}</td><td>${esc(c.rule)}</td></tr>`).join("");
  const armRows = arms.map((a) => {
    const x = r.arms[a]!;
    return `<tr><th scope="row">${esc(x.label)}</th><td>${fixed(x.mean_requests, 2)}</td><td>${x.requests_included ?? "n/a"}</td><td>${of(x.hidden_test_passes, x.sessions)}</td><td>${x.censored ?? "n/a"}</td><td>${x.verification_requests ?? "n/a"}</td><td>${usd(x.cost_per_session_usd)}</td><td>${x.tokens ? kilo(x.tokens.per_session) : "not converted"}</td></tr>`;
  }).join("");
  const controls = r.controls.map((c) => `<li class="${c.realized && !c.necessary_work_skipped ? "ok" : "bad"}"><b>${esc(CONTROL_WORDS[c.condition] ?? c.condition)}</b><span>${c.realized ? "ran as designed" : "did not run as designed"} · ${c.necessary_work_skipped ? "necessary check skipped" : "necessary check kept"}</span></li>`).join("");
  return `<details class="ev-more">
  <summary>More detail: every registered number</summary>
  <div class="ev-more-body">
    <h4>The registered test</h4>
    <div class="ev-table" role="region" aria-label="Primary test and sensitivity analyses" tabindex="0"><table>
      <thead><tr><th scope="col">Comparison</th><th scope="col">Ratio</th><th scope="col">95% range</th><th scope="col">Blocks fewer</th><th scope="col">Sign-flip p</th><th scope="col">Under the line</th></tr></thead>
      <tbody>${contrastRow("Adage / alone (primary)", r.primary)}${contrastRow("Without censored blocks (sensitivity)", r.sensitivity.without_censored_blocks)}${contrastRow("Without the has-keys task (sensitivity)", r.sensitivity.without_has_keys)}${contrastRow("Hooks only / alone (report only)", r.report_only.noexec_vs_a0)}${contrastRow("Adage / hooks only (report only)", r.report_only.best_vs_noexec)}</tbody>
    </table></div>
    <h4>Each arm</h4>
    <div class="ev-table" role="region" aria-label="Each arm" tabindex="0"><table>
      <thead><tr><th scope="col">Arm</th><th scope="col">Mean requests</th><th scope="col">Requests counted</th><th scope="col">Hidden tests</th><th scope="col">Censored</th><th scope="col">Test-run requests</th><th scope="col">Cost / session</th><th scope="col">Tokens / session</th></tr></thead>
      <tbody>${armRows}</tbody>
    </table></div>
    <h4>Safety controls, run first</h4>
    <ul class="ev-controls">${controls}</ul>
    <h4>Every G criterion</h4>
    <div class="ev-table" role="region" aria-label="G criteria" tabindex="0"><table>
      <thead><tr><th scope="col">Criterion</th><th scope="col">Applies</th><th scope="col">Held</th><th scope="col">Rule, or why it does not apply</th></tr></thead>
      <tbody>${criteria}</tbody>
    </table></div>
    <p class="ev-fine">Blocks left out for an unverified request count: ${r.excluded_blocks.length ? esc(r.excluded_blocks.join(", ")) : "none"} (at most 2 allowed). A session that stopped early counts as a quality failure in every arm, and only Adage's is raised to ${60} requests. Assignments ${r.assignments ?? "?"}, collected ${r.collected ?? "?"}. ${r.problems.map((p) => esc(p)).join(" ")}</p>
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
    const link = (p: string) => (p.includes("/") ? `<a href="${blob(p)}">` : "<span>");
    const close = (p: string) => (p.includes("/") ? "</a>" : "</span>");
    if (src.decision_record) parts.push(`${link(src.decision_record)}the decision record${close(src.decision_record)}`);
    parts.push(`${link(src.report)}the report${close(src.report)} (sha256 <code>${esc(src.report_sha256.slice(0, 12))}…</code>)`);
  }
  return `<p class="ev-src">${parts.join(" · ")}</p>`;
}

/** The whole held-out component (the evidence page). */
export function renderHeldout(h: Heldout): string {
  const s = h.study, r = h.result;
  if (h.status !== "result" || !r) {
    return `<div class="ev-ho" data-state="pending">
  <p class="ev-ho-flag"><span class="ev-pulse" aria-hidden="true"></span>Running now</p>
  <h3 class="ev-ho-h">${esc(pendingLine(s))}</h3>
  <ul class="ev-ho-facts">
    <li><b>${s.tasks.length}</b><span>new tasks, frozen before any result and never used for tuning</span></li>
    <li><b>${s.blocks}</b><span>paired blocks: each task ${s.repetitions} times, in every arm</span></li>
    <li><b>${s.controls.length}</b><span>safety controls, run first; one failure stops the study</span></li>
    <li><b>${s.sessions}</b><span>sessions in all, every one kept</span></li>
  </ul>
  <p class="ev-ho-plain">${esc(s.frozen)} The result lands here as it comes out, pass or fail, with its decision record.</p>
  ${sourceLine(h)}
</div>`;
  }
  const state = !decided(r) ? "none" : r.outcome === "pass" ? "pass" : "fail";
  const p = r.primary, q = r.quality, a0 = r.arms.A0, best = r.arms.BEST;
  const warnings = r.warnings.length ? `<ul class="ev-ho-warn">${r.warnings.map((w) => `<li>${esc(w)}</li>`).join("")}</ul>` : "";
  if (state === "none") {
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
  const tokens = r.tokens_converted && a0?.tokens && best?.tokens
    ? `<div><dt>Tokens per session</dt><dd><span class="was">${kilo(a0.tokens.per_session)}</span><span class="arrow" aria-hidden="true">→</span><span class="now">${kilo(best.tokens.per_session)}</span><small>alone → with Adage, every token counted</small></dd></div>`
    : `<div><dt>Tokens per session</dt><dd><span class="now ev-na">n/a</span><small>not converted with this report; the archived sessions have them</small></dd></div>`;
  return `<div class="ev-ho" data-state="${state}">
  <p class="ev-ho-flag"><span class="ev-verdict">${esc(r.verdict)}</span>Held-out result · ${esc(r.label)}</p>
  <h3 class="ev-ho-h">${esc(r.headline)}</h3>
  <p class="ev-ho-plain">${esc(plain(r, s))}</p>
  ${warnings}
  ${p ? scale(p) : ""}
  <dl class="ev-ho-stats">
    <div><dt>Model requests per session</dt><dd><span class="was">${fixed(a0?.mean_requests, 1)}</span><span class="arrow" aria-hidden="true">→</span><span class="now">${fixed(best?.mean_requests, 1)}</span><small>alone → with Adage, over ${p?.included_blocks ?? "?"} blocks</small></dd></div>
    <div><dt>Blocks where Adage needed fewer</dt><dd><span class="now">${of(p?.blocks_fewer, p?.blocks)}</span><small>same task, same repetition</small></dd></div>
    <div><dt>Hidden tests passed</dt><dd><span class="now">${of(q?.best, q?.of)}</span><small>with Adage · ${of(q?.a0, q?.of)} alone</small></dd></div>
    <div><dt>Cost per session</dt><dd><span class="was">${usd(a0?.cost_per_session_usd)}</span><span class="arrow" aria-hidden="true">→</span><span class="now">${usd(best?.cost_per_session_usd)}</span><small>API list price</small></dd></div>
    ${tokens}
    <div><dt>Safety controls</dt><dd><span class="now">${r.controls.filter((c) => c.realized && !c.necessary_work_skipped).length}/${s.controls.length}</span><small>kept every necessary check</small></dd></div>
  </dl>
  ${detail(r, h)}
  ${sourceLine(h)}
</div>`;
}

/** One paragraph for the home page, beside the claim. */
export function renderHeldoutBrief(h: Heldout): string {
  const s = h.study, r = h.result;
  const more = `<a class="ev-brief-link" href="/how-we-tested#heldout">How the held-out test works →</a>`;
  if (h.status !== "result" || !r) return `<p class="ev-brief" data-state="pending"><span class="ev-pulse" aria-hidden="true"></span><span>${esc(pendingLine(s))}</span> ${more}</p>`;
  const p = r.primary;
  if (!decided(r)) return `<p class="ev-brief" data-state="none"><b>Held-out result: no verdict.</b> <span>${esc(r.headline)}</span> ${more}</p>`;
  const q = r.quality;
  const body = r.outcome === "pass"
    ? `${times(p?.ratio)} the model requests of the agent alone on ${s.tasks.length} new tasks (95% upper bound ${times(p?.upper)}, pass line ${times(p?.threshold)}); hidden tests ${of(q?.best, q?.of)} with Adage, ${of(q?.a0, q?.of)} alone.`
    : `${times(p?.ratio)} the model requests of the agent alone on ${s.tasks.length} new tasks; the 95% upper bound, ${times(p?.upper)}, missed the ${times(p?.threshold)} pass line. Hidden tests ${of(q?.best, q?.of)} with Adage, ${of(q?.a0, q?.of)} alone.`;
  return `<p class="ev-brief" data-state="${r.outcome}"><b>Held-out result: ${r.outcome === "pass" ? "passed" : "did not pass"}.</b> <span>${esc(body)}</span> ${more.replace("How the held-out test works", "The full result")}</p>`;
}
