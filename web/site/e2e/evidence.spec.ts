import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
// The converter that turns `heldout_cohort.py report` output into public/evidence/heldout.json.
import { convert, pending } from "../scripts/heldout-evidence.mjs";

const here = (p: string) => fileURLToPath(new URL(p, import.meta.url));
const json = (p: string) => JSON.parse(readFileSync(here(p), "utf8"));
// Synthetic reports from the registered analysis (e2e/fixtures/heldout/make.py): not study results.
const fixture = (name: "pass" | "fail" | "unverified") => convert(json(`./fixtures/heldout/${name}.json`), { source: { report: `${name}.json`, report_sha256: "0".repeat(64), decision_record: null, converted_at: "2026-10-01T00:00:00Z", converter: "test" } });
const dev = json("../public/evidence/dev-v9b.json");
const cold = json("../public/evidence/cold-start.json");

async function heldout(page: Page, value: unknown) {
  await page.route("**/evidence/heldout.json", (r) => r.fulfill({ status: 200, json: value }));
}
const films = (page: Page) => page.route("**/*.mp4", (r) => r.abort());

test.describe("/how-we-tested", () => {
  test("states the question and the claim, with numbers from the data files", async ({ page }) => {
    await page.goto("/how-we-tested");
    await expect(page.getByRole("heading", { level: 1 })).toContainText("Fewer model calls");
    await expect(page.locator(".ev-q")).toHaveText("Can Adage handle predictable steps on your CPU so the agent needs fewer model calls, without hurting correctness?");
    const a0 = dev.arms.A0.mean_requests.toFixed(1), a4 = dev.arms.A4.mean_requests.toFixed(1);
    await expect(page.locator(".ev-run").first()).toContainText(`${a0} calls per session`);
    await expect(page.locator(".ev-run.adage")).toContainText(`${a4} calls per session`);
    // One dot per model call, rounded.
    await expect(page.locator(".ev-run.adage .ev-dots i")).toHaveCount(Math.round(dev.arms.A4.mean_requests));
    await expect(page.locator("#journey")).toContainText(`${cold.cost_reduction_percent.toFixed(1)}% lower cost`);
    await expect(page.locator("#journey")).toContainText(`${dev.comparison.A4.ratio.toFixed(2)}×`);
    await expect(page.locator("#journey")).toContainText("0 of 4");
    await expect(page.locator("#journey")).toContainText("Invalid");
    // No unfilled data token reaches the page.
    expect(await page.content()).not.toContain("{{");
  });

  test("ships the held-out comparison as running, with its pass rule", async ({ page }) => {
    await page.goto("/how-we-tested");
    const line = "Held-out comparison running: 4 new tasks, 20 paired blocks; pass rule: 95% upper bound ≤ 0.90× requests with no quality loss.";
    await expect(page.locator("[data-heldout='full'] .ev-ho")).toHaveAttribute("data-state", "pending");
    await expect(page.locator("[data-heldout='full'] .ev-ho-h")).toHaveText(line);
    await expect(page.locator("[data-heldout='brief']")).toContainText(line);
  });

  test("a passing result drops in from the data: GO, the ratio on its scale, quality and controls", async ({ page }) => {
    const h = fixture("pass");
    await heldout(page, h);
    await page.goto("/how-we-tested#heldout");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-state", "pass");
    await expect(ho.locator(".ev-verdict")).toHaveText("GO");
    await expect(ho.locator(".ev-ho-h")).toHaveText("Adage passed the held-out comparison.");
    await expect(ho.locator(".ev-scale-track")).toHaveAttribute("aria-label", new RegExp(`^Request ratio ${h.result!.primary!.ratio!.toFixed(2)}×`));
    await expect(ho.locator(".ev-ho-stats")).toContainText(`${h.result!.quality!.best}/${h.result!.quality!.of}`);
    await expect(ho.locator(".ev-ho-stats")).toContainText("5/5");
    await expect(ho.locator(".ev-ho-stats")).toContainText("not converted");
    await ho.locator(".ev-more > summary").click();
    await expect(ho.locator(".ev-more")).toContainText("Without the has-keys task (sensitivity)");
    await expect(ho.locator(".ev-more tbody th", { hasText: /^G9$/ })).toBeVisible();
    await expect(page.locator("[data-heldout='brief']")).toContainText("Held-out result: passed.");
  });

  test("a failing result says so plainly: NO-GO, which rule missed, the same numbers", async ({ page }) => {
    const h = fixture("fail");
    await heldout(page, h);
    await page.goto("/how-we-tested#heldout");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-state", "fail");
    await expect(ho.locator(".ev-verdict")).toHaveText("NO-GO");
    await expect(ho.locator(".ev-ho-h")).toHaveText("Adage did not pass the held-out comparison.");
    await expect(ho.locator(".ev-ho-plain")).toContainText(`the cautious estimate, ${h.result!.primary!.upper!.toFixed(2)}×, does not clear the 0.90× line`);
    await expect(ho.locator(".ev-ho-plain")).toContainText("quality fell");
    await expect(ho.locator(".ev-scale-ci")).toHaveClass(/\bmiss\b/);
    await expect(page.locator("[data-heldout='brief']")).toContainText("Held-out result: did not pass.");
  });

  test("an inconclusive result shows no verdict and no headline ratio", async ({ page }) => {
    await heldout(page, fixture("unverified"));
    await page.goto("/how-we-tested#heldout");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-state", "none");
    await expect(ho.locator(".ev-verdict")).toHaveText("NO VERDICT");
    await expect(ho.locator(".ev-ho-h")).toContainText("No verdict");
    await expect(ho.locator(".ev-scale")).toHaveCount(0);
    await expect(ho.locator(".ev-ho-stats")).toHaveCount(0);
    await expect(ho.locator(".ev-ho-warn")).toContainText("3 of 20 blocks have an unverified request count");
    await expect(page.locator("[data-heldout='brief']")).toContainText("Held-out result: no verdict.");
  });

  test("an invalid report (a lapsed approval) is never shown as a result", async ({ page }) => {
    const report = { ...json("./fixtures/heldout/pass.json"), valid: false, lapses: [{ order: 12, arm: "BEST", condition: "positive", problem: "withheld: approval" }] };
    await heldout(page, convert(report));
    await page.goto("/how-we-tested#heldout");
    const ho = page.locator("[data-heldout='full'] .ev-ho");
    await expect(ho).toHaveAttribute("data-state", "none");
    await expect(ho.locator(".ev-ho-h")).toContainText("not evidence");
  });

  test("the pending file the converter writes is the one the site ships", async () => {
    expect(json("../public/evidence/heldout.json")).toEqual(pending());
  });

  test("dotted words show their definition, in the day theme too", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
    await page.goto("/how-we-tested#fair");
    await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
    const term = page.locator("#fair .ev-callout .gloss", { hasText: /^held-out$/i }).first();
    await term.scrollIntoViewIfNeeded();
    await term.hover();
    const tip = page.locator("#termtip");
    await expect(tip).toBeVisible();
    await expect(tip).toContainText("never used to tune Adage");
    await expect(page.locator("#fair .ev-rules .gloss", { hasText: /^paired comparison$/ })).toBeAttached();
    // Never a dark box on a light page: the word keeps a transparent background, the tip a light one.
    expect(await term.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgba(0, 0, 0, 0)");
    expect(await tip.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgb(255, 255, 255)");
  });

  for (const scheme of ["dark", "light"] as const) {
    test(`no axe violations (${scheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
      await page.goto("/how-we-tested");
      await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
      await page.evaluate(() => document.querySelectorAll("details").forEach((d) => (d.open = true)));
      const r = await new AxeBuilder({ page }).analyze();
      expect(r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
    });
  }

  test("a decided result passes axe too, in both themes", async ({ page }) => {
    await heldout(page, fixture("fail"));
    for (const scheme of ["dark", "light"] as const) {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
      await page.goto("/how-we-tested#heldout");
      await expect(page.locator(".ev-ho")).toHaveAttribute("data-state", "fail");
      await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
      await page.evaluate(() => document.querySelectorAll("details").forEach((d) => (d.open = true)));
      const r = await new AxeBuilder({ page }).include("#heldout").analyze();
      expect(r.violations.map((v) => `${scheme} ${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
    }
  });

  test("a phone never scrolls sideways", async ({ page }) => {
    await heldout(page, fixture("pass"));
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/how-we-tested");
    await expect(page.locator(".ev-ho")).toHaveAttribute("data-state", "pass");
    await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
    await page.evaluate(() => document.querySelectorAll("details").forEach((d) => (d.open = true)));
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(0);
  });

  test("sections hold as scenes and nothing takes the scroll over", async ({ page }) => {
    await page.goto("/how-we-tested");
    await expect(page.locator("html")).toHaveClass(/\bscenes\b/);
    // A wheel scroll moves the page by the browser's own amount, and it stays where it was left.
    await page.mouse.move(700, 500);
    await page.mouse.wheel(0, 400);
    await expect.poll(() => page.evaluate(() => Math.round(scrollY))).toBe(400);
    await page.waitForTimeout(400);
    expect(await page.evaluate(() => Math.round(scrollY))).toBe(400);
  });
});

test.describe("the home page's claim", () => {
  test("links to how we tested, and shows the held-out status from its data file", async ({ page }) => {
    await films(page);
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/#evidence");
    const card = page.locator(".launch-proof");
    await expect(card).toContainText(`${dev.arms.A0.mean_requests.toFixed(1)}`);
    await expect(card).toContainText(`${dev.arms.A4.mean_requests.toFixed(1)}`);
    await expect(card.locator("[data-heldout='brief']")).toContainText("Held-out comparison running");
    await expect(page.locator("#evidence .lede")).toContainText("Adage saves model calls by handling learned reflexes and instincts on your CPU");
    await expect(page.locator("#evidence .lede")).toContainText("GPU use and energy are not measured");
    await expect(card.getByRole("link", { name: "How we tested, misses included" })).toHaveAttribute("href", "/how-we-tested");
    expect(await page.content()).not.toContain("{{");
  });

  test("a held-out result reaches the home page without a rebuild", async ({ page }) => {
    await films(page);
    await heldout(page, fixture("fail"));
    await page.goto("/#evidence");
    await expect(page.locator(".launch-proof [data-heldout='brief']")).toContainText("Held-out result: did not pass.");
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
    await expect(fig.locator(".cap.on")).toHaveText("fewer model calls, the same hidden tests passing");
    // Every hook the plugin registers is named, and the PostToolBatch step says it comes before the next model call.
    await expect(page.locator("#plug-post .plug-when")).toContainText("before the next model call");
    await expect(page.locator("#plug-sum")).toContainText("No hook skips a model call");
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
