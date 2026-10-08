import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
// The converter that turns `heldout_cohort.py report` output into public/evidence/heldout.json.
import { convert, pending } from "../scripts/heldout-evidence.mjs";

const here = (p: string) => fileURLToPath(new URL(p, import.meta.url));
const json = (p: string) => JSON.parse(readFileSync(here(p), "utf8"));
type Fixture = "pass" | "fail" | "fail-savings" | "unverified";
// Synthetic reports from the registered analysis (e2e/fixtures/heldout/make.py): not study results.
const fixture = (name: Fixture) => convert(json(`./fixtures/heldout/${name}.json`), { source: { report: `${name}.json`, report_sha256: "0".repeat(64), decision_record: null, converted_at: "2026-10-01T00:00:00Z", converter: "test" } });
// What the site ships: the converted held-out report.
const shipped = json("../public/evidence/heldout.json");
const adv = shipped.result.modes.advisory, exe = shipped.result.modes.execution;

async function heldout(page: Page, value: unknown) {
  await page.route("**/evidence/heldout.json", (r) => r.fulfill({ status: 200, json: value }));
}

test.describe("/evidence", () => {
  test("leads with the advisory result, from the shipped data", async ({ page }) => {
    await page.goto("/evidence");
    await expect(page.getByRole("heading", { level: 1 })).toContainText(`About ${adv.fewer_requests_percent}% fewer model calls.`);
    await expect(page.locator(".ev-q")).toHaveText("Can Adage handle predictable steps on your CPU so the agent needs fewer model calls, without hurting correctness?");
    await expect(page.locator("#question .lede")).toContainText("Adage saves model calls by handling learned reflexes and instincts on your CPU");
    // The running example: one dot per model call, the agent alone against advisory mode.
    const example = page.locator("#question");
    await expect(example.locator(".ev-run").first()).toContainText(`${adv.mean_requests_alone.toFixed(1)} calls per session`);
    await expect(example.locator(".ev-run.adage")).toContainText(`${adv.mean_requests.toFixed(1)} calls per session`);
    await expect(example.locator(".ev-run.adage .ev-dots i")).toHaveCount(Math.round(adv.mean_requests));
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-advisory", "clear");
    await expect(ho.locator(".ev-ho-h")).toHaveText(`About ${adv.fewer_requests_percent}% fewer model calls and a third lower cost on held-out tasks, with the same correctness.`);
    await expect(ho.locator(".ev-ho-flag")).toContainText("Pre-registered comparison");
    await expect(ho.locator(".ev-ho-stats")).toContainText(`${adv.hidden_test_passes}/${adv.of}`);
    await expect(ho.locator(".ev-ho-stats")).toContainText(`${adv.blocks_fewer}/${adv.blocks}`);
    // Execution mode: experimental, the quality bar not met, said plainly and not as the headline.
    await expect(ho.locator(".ev-exec")).toHaveAttribute("data-status", "experimental-quality");
    await expect(ho.locator(".ev-exec-flag")).toContainText("Experimental · quality bar not yet met");
    await expect(ho.locator(".ev-exec-line")).toContainText(`hidden tests ${exe.hidden_test_passes}/${exe.of} against ${exe.hidden_test_passes_alone}/${exe.of}`);
    await expect(ho.locator(".ev-exec-line")).toContainText("did not pass");
    await expect(ho.locator(".ev-exec-line")).toContainText("One of its failures came in a block where all three arms failed.");
    // The study history is in the repository, not on this page.
    await expect(page.locator(".ev-timeline")).toHaveCount(0);
    await expect(page.getByRole("link", { name: "heldout-report.json" })).toHaveAttribute("href", "/evidence/heldout-report.json");
    expect(await page.content()).not.toContain("{{");
  });

  test("the shipped report and record are the files behind the numbers", async ({ request }) => {
    const r = await request.get("/evidence/heldout-report.json");
    expect(r.ok()).toBe(true);
    const report = await r.json();
    expect(report.report_only.noexec_vs_a0.ratio).toBe(adv.ratio);
    expect(report.primary.ratio).toBe(exe.ratio);
    expect((await request.get("/evidence/heldout-record.md")).ok()).toBe(true);
  });

  test("before a result, the comparison shows as running, with its pass rule", async ({ page }) => {
    await heldout(page, pending());
    await page.goto("/evidence#results");
    const line = "Held-out comparison running: 4 new tasks, 20 paired blocks; pass rule: 95% upper bound ≤ 0.90× requests with no quality loss.";
    await expect(page.locator("[data-heldout='full'] .ev-ho")).toHaveAttribute("data-state", "pending");
    await expect(page.locator("[data-heldout='full'] .ev-ho-h")).toHaveText(line);
    await expect(page.getByRole("heading", { level: 1 })).toContainText("Here is exactly how we test it.");
  });

  test("when both modes pass, execution mode says so", async ({ page }) => {
    const h = fixture("pass");
    await heldout(page, h);
    await page.goto("/evidence#results");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-advisory", "clear");
    await expect(ho.locator(".ev-exec")).toHaveAttribute("data-status", "passed");
    await expect(ho.locator(".ev-exec-flag")).toContainText("Passed its registered test");
    await expect(ho.locator(".ev-scale-track")).toHaveAttribute("aria-label", new RegExp(`request ratio ${h.result!.modes!.advisory!.ratio!.toFixed(2)}×`));
    // Held sections can sit under the next scene; open the detail directly, as a reader's click would.
    await ho.locator(".ev-more").evaluate((d) => ((d as HTMLDetailsElement).open = true));
    await expect(ho.locator(".ev-more")).toContainText("The study's registered outcome for execution mode: pass");
    await expect(ho.locator(".ev-more tbody th", { hasText: /^G9$/ })).toBeVisible();
  });

  test("a mixed result: advisory leads, execution is experimental, the verdict stays in the detail", async ({ page }) => {
    await heldout(page, fixture("fail"));
    await page.goto("/evidence#results");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-advisory", "clear");
    await expect(ho.locator(".ev-ho-h")).toContainText("with the same correctness");
    await expect(ho.locator(".ev-exec")).toHaveAttribute("data-status", "experimental-quality");
    // Held sections can sit under the next scene; open the detail directly, as a reader's click would.
    await ho.locator(".ev-more").evaluate((d) => ((d as HTMLDetailsElement).open = true));
    await expect(ho.locator(".ev-more")).toContainText("The study's registered outcome for execution mode: did not pass (criteria not met: G5)");
  });

  test("when neither mode saves enough, the page says there is no clear saving", async ({ page }) => {
    await heldout(page, fixture("fail-savings"));
    await page.goto("/evidence#results");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-advisory", "no-clear-saving");
    await expect(ho.locator(".ev-ho-h")).toContainText("Advisory mode: no clear saving in model calls");
    await expect(ho.locator(".ev-ho-plain")).toContainText("does not clear the 0.90× line");
    await expect(ho.locator(".ev-scale-ci")).toHaveClass(/\bmiss\b/);
    await expect(ho.locator(".ev-exec")).toHaveAttribute("data-status", "experimental-savings");
    await expect(page.getByRole("heading", { level: 1 })).toContainText("Here is exactly how we test it.");
  });

  test("an inconclusive result shows no verdict and no headline number", async ({ page }) => {
    await heldout(page, fixture("unverified"));
    await page.goto("/evidence#results");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-state", "none");
    await expect(ho.locator(".ev-verdict")).toHaveText("NO VERDICT");
    await expect(ho.locator(".ev-scale")).toHaveCount(0);
    await expect(ho.locator(".ev-ho-stats")).toHaveCount(0);
    await expect(ho.locator(".ev-ho-warn")).toContainText("3 of 20 blocks have an unverified request count");
    await expect(page.locator("[data-heldout='example']")).toBeEmpty();
  });

  test("an invalid report (a lapsed approval) is never shown as a result", async ({ page }) => {
    const report = { ...json("./fixtures/heldout/pass.json"), valid: false, lapses: [{ order: 12, arm: "BEST", condition: "positive", problem: "withheld: approval" }] };
    await heldout(page, convert(report));
    await page.goto("/evidence#results");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-state", "none");
    await expect(ho.locator(".ev-ho-h")).toContainText("not evidence");
  });

  test("dotted words show their definition, in the day theme too", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await page.goto("/evidence#tested");
    await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
    const term = page.locator("#tested .ev-callout .gloss", { hasText: /^held-out$/i }).first();
    await term.scrollIntoViewIfNeeded();
    await term.hover();
    const tip = page.locator("#termtip");
    await expect(tip).toBeVisible();
    await expect(tip).toContainText("never used to tune Adage");
    await expect(page.locator("#tested .ev-rules .gloss", { hasText: /^paired comparison$/ })).toBeAttached();
    // Never a dark box on a light page: the word keeps a transparent background, the tip a light one.
    expect(await term.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
    expect(await tip.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgb(255, 255, 255)");
  });

  for (const scheme of ["dark", "light"] as const) {
    test(`no axe violations (${scheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
      await page.goto("/evidence");
      await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
      await page.evaluate(() => document.querySelectorAll("details").forEach((d) => (d.open = true)));
      const r = await new AxeBuilder({ page }).analyze();
      expect(r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
    });
  }

  test("every outcome passes axe, in both themes", async ({ page }) => {
    for (const name of ["pass", "fail-savings", "unverified"] as const) {
      await page.unrouteAll();
      await heldout(page, fixture(name));
      for (const scheme of ["dark", "light"] as const) {
        await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
        await page.goto("/evidence#results");
        await expect(page.locator(".ev-ho")).toBeVisible();
        await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
        await page.evaluate(() => document.querySelectorAll("details").forEach((d) => (d.open = true)));
        const r = await new AxeBuilder({ page }).include("#results").analyze();
        expect(r.violations.map((v) => `${name} ${scheme} ${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
      }
    }
  });

  test("a phone never scrolls sideways", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/evidence");
    await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
    await page.evaluate(() => document.querySelectorAll("details").forEach((d) => (d.open = true)));
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(0);
  });

  test("sections hold as scenes and nothing takes the scroll over", async ({ page }) => {
    await page.goto("/evidence");
    await expect(page.locator("html")).toHaveClass(/\bscenes\b/);
    // A wheel scroll moves the page by the browser's own amount, and it stays where it was left.
    await page.mouse.move(700, 500);
    await page.mouse.wheel(0, 400);
    await expect.poll(() => page.evaluate(() => Math.round(scrollY))).toBe(400);
    await page.waitForTimeout(400);
    expect(await page.evaluate(() => Math.round(scrollY))).toBe(400);
  });
});

// The Claude Code mod's comparison, as the site ships it.
const mod = json("../public/evidence/claude-mod.json");

test.describe("the results at a glance", () => {
  test("two tests, the same few numbers each, from their data files", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/evidence#glance");
    const [a, m] = [page.locator(".ev-gl").first(), page.locator(".ev-gl.mod")];
    await expect(page.locator(".ev-gl")).toHaveCount(2);
    // Advisory mode, the registered held-out test.
    await expect(a.locator(".ev-gl-h")).toHaveText("Advisory mode, the default");
    await expect(a.locator(".ev-gl-big")).toHaveText(`${adv.fewer_requests_percent}% fewer model calls`);
    await expect(a.locator(".ev-gl-stats")).toContainText(`${adv.mean_requests_alone.toFixed(1)} → ${adv.mean_requests.toFixed(1)}`);
    await expect(a.locator(".ev-gl-stats")).toContainText(`${Math.round(Math.abs(adv.cost_change_percent))}% lower`);
    await expect(a.locator(".ev-gl-stats")).toContainText(`${adv.hidden_test_passes}/${adv.of}`);
    await expect(a.locator(".ev-gl-exec")).toContainText("experimental · quality bar not yet met");
    await expect(a.getByRole("link", { name: "The full result ↓" })).toHaveAttribute("href", "#results");
    // The mod, a development test, said as one.
    await expect(m.locator(".ev-gl-tag")).toContainText("Development test");
    await expect(m.locator(".ev-gl-big")).toHaveText(`${Math.round(mod.mod_vs_alone.fewer_requests_percent)}% fewer model calls`);
    await expect(m.locator(".ev-gl-stats")).toContainText(`${mod.arms.a0.requests_mean.toFixed(1)} → ${mod.arms.mod.requests_mean.toFixed(1)}`);
    await expect(m.locator(".ev-gl-stats")).toContainText(`${Math.round(mod.mod_vs_alone.cost_lower_percent)}% lower`);
    await expect(m.locator(".ev-gl-stats")).toContainText(`${mod.arms.mod.hidden_passed}/${mod.arms.mod.of}`);
    await expect(m.getByRole("link", { name: "Why the mod helps ↓" })).toHaveAttribute("href", "#mod");
    // The old evidence section's cards are gone: one simple story.
    await expect(page.getByText("Measured against a control")).toHaveCount(0);
    await expect(page.locator(".trial")).toHaveCount(0);
    expect(await page.content()).not.toContain("{{");
  });

  test("a new held-out result reaches the glance without a rebuild", async ({ page }) => {
    await heldout(page, fixture("fail-savings"));
    await page.goto("/evidence#glance");
    await expect(page.locator(".ev-gl").first().locator(".ev-gl-big")).toContainText("the agent alone's model calls");
  });

  test("the mod: why it helps, what it did, and what it doesn't show yet", async ({ page, request }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/evidence#mod");
    const sec = page.locator("#mod");
    await expect(sec.getByRole("heading", { level: 2 })).toHaveText("A hook can only suggest the next step. A mod can take it.");
    // One dot per model call in each arm, and a ring per step Adage took itself.
    const runs = sec.locator(".ev-run");
    await expect(runs).toHaveCount(3);
    await expect(runs.nth(0).locator(".ev-dots:not(.made) i")).toHaveCount(Math.round(mod.arms.a0.requests_mean));
    await expect(runs.nth(1).locator(".ev-dots:not(.made) i")).toHaveCount(Math.round(mod.arms.adage.requests_mean));
    await expect(runs.nth(2).locator(".ev-dots:not(.made) i")).toHaveCount(Math.round(mod.arms.mod.requests_mean));
    await expect(runs.nth(2).locator(".ev-dots.made i")).toHaveCount(Math.round(mod.arms.mod.steps_made_per_session));
    await expect(sec.locator(".ev-steps li")).toHaveCount(4);
    await expect(sec.locator(".ev-callout")).toContainText("A person approved the three steps beforehand");
    // The section may sit under the next one as it slides over: open the detail as a reader would reach it.
    await sec.locator("details.ev-more").evaluate((d: HTMLDetailsElement) => (d.open = true));
    await expect(sec.locator(".ev-table")).toContainText(mod.arms.mod.cost_usd_mean.toFixed(4));
    await expect(sec.locator(".ev-more-body")).toContainText("development evidence, not a registered test");
    await expect(sec.getByRole("link", { name: "/evidence/claude-mod.json" })).toHaveAttribute("href", "/evidence/claude-mod.json");
    // The file behind the numbers is served as shipped.
    const r = await request.get("/evidence/claude-mod.json");
    expect(r.ok()).toBe(true);
    expect((await r.json()).arms.mod.requests_mean).toBe(mod.arms.mod.requests_mean);
  });
});

test.describe("/how opens on how Adage plugs into Claude Code", () => {
  test("the diagram follows the step in the middle of the screen", async ({ page }) => {
    await page.goto("/how");
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("How Adage plugs into Claude Code.");
    const fig = page.locator("#plug-fig");
    await expect(fig).toHaveAttribute("data-step", "loop");
    for (const step of ["start", "prompt", "pre", "post", "stop", "learn", "sum"]) {
      await page.locator(`#plug-${step}`).evaluate((el) => el.scrollIntoView({ block: "center", behavior: "instant" }));
      await expect(fig).toHaveAttribute("data-step", step);
      await expect(page.locator(`#plug-${step}`)).toHaveClass(/\bon\b/);
    }
    await expect(fig.locator(".cap.on")).toHaveText("advisory mode: fewer model calls, the same hidden tests passing");
    await expect(page.locator("#plug-post .plug-when")).toContainText("before the next model call");
    await expect(page.locator("#plug-start .plug-mode")).toHaveText("Execution mode");
    await expect(page.locator("#plug-sum")).toContainText("No hook skips a model call");
    // The running example is the held-out study's, labelled by mode.
    await expect(page.locator("#plug-sum .plug-result b")).toHaveText(`${adv.mean_requests_alone.toFixed(1)} → ${adv.mean_requests.toFixed(1)}`);
    await expect(page.locator("#plug-sum .plug-exec")).toContainText("Execution mode, experimental · quality bar not yet met");
  });

  test("its dotted words and technical detail work, in the day theme", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await page.goto("/how");
    await page.locator("#plug-post").scrollIntoViewIfNeeded();
    const term = page.locator("#plug-post .gloss", { hasText: /^receipt$/ }).first();
    await term.hover();
    await expect(page.locator("#termtip")).toContainText("Adage's short report");
    expect(await term.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
    await page.locator("#plug-post .plug-more summary").click();
    await expect(page.locator("#plug-post .plug-more")).toContainText("OS sandbox with no network");
  });
});
