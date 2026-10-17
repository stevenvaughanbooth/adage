import { mkdirSync, readFileSync } from "node:fs";
import AxeBuilder from "@axe-core/playwright";
import { type Page, expect, test } from "@playwright/test";
import { DEMO_PORT } from "./ports";
const BASE = `http://127.0.0.1:${DEMO_PORT}`;
const revision = "a".repeat(64);
async function screenshot(page: Page, name: string) {
  const dir = process.env.ADAGE_SCREENSHOT_DIR;
  if (!dir) return;
  mkdirSync(dir, { recursive: true });
  await page.screenshot({ path: `${dir}/${name}.png` });
}

async function fixture(page: Page, options: { enabled?: boolean; stale?: boolean; instinct?: boolean; failRefresh?: boolean } = {}) {
  const g = JSON.parse(readFileSync(new URL("./fixtures/busy.json", import.meta.url), "utf8"));
  const route = g.routes.find((r: { id: string }) => r.id === "start-with-make-test");
  Object.assign(route, { instinct: options.instinct ?? false, last_review: null, state: "forming", strength: "prime", ceiling: options.instinct ? "prime" : "fire" });
  route.review.verbs = options.instinct ? ["approve", "extinguish", "pin"] : ["approve", "extinguish", "strengthen", "pin"];
  route.review.reasons = [];
  const originalVersion = route.version;
  const decisions: unknown[] = [];
  let previews = 0;
  await page.route("**/api/v1/graph", r => options.failRefresh && decisions.length ? r.fulfill({ status: 503, body: "Temporarily unavailable" }) : r.fulfill({ json: g }));
  await page.route("**/api/v1/events", r => r.abort());
  await page.route("**/api/v1/review**", async r => {
    if (r.request().method() === "POST") {
      decisions.push(r.request().postDataJSON());
      expect(r.request().headers()["x-adage-csrf"]).toBe("capability");
      if (options.stale && decisions.length === 1) return r.fulfill({ status: 409, body: "This reflex changed. Nothing was written. Reload and inspect the new proposal." });
      route.version = "0.3.1";
      route.last_review = { decision: "approved", on: "2026-10-17", current: true };
      if (r.request().postDataJSON().verb === "strengthen") route.strength = "fire";
      return r.fulfill({ json: { saved: true, verb: r.request().postDataJSON().verb, reflex: route.id, reference: route.reference, repo: route.repo, warnings: [], state: route.state, strength: route.strength, recorded: "2026-10-17", proposal_applied: false, already: false, pack_approved: [] } });
    }
    if (new URL(r.request().url()).search) {
      previews++;
      return r.fulfill({ json: { revision: previews === 1 ? revision : "b".repeat(64), current_directive: "Run make test before finishing.", verbs: route.review.verbs, reasons: [], proposal: null } });
    }
    return r.fulfill({ json: { enabled: options.enabled ?? true, token: options.enabled === false ? null : "capability", reason: options.enabled === false ? "This map is read-only. Run adage map from your own terminal to enable review." : null } });
  });
  await page.goto(`${BASE}/?reflex=${encodeURIComponent(`${route.reference}::${route.id}`)}`);
  await expect(page.getByRole("region", { name: "Review this reflex" })).toBeVisible();
  return { decisions, route, g, originalVersion };
}

test("approval inspects one version, persists visibly, and keeps an instinct prime", async ({ page }) => {
  const { decisions, route } = await fixture(page, { instinct: true });
  const review = page.getByRole("region", { name: "Review this reflex" });
  await expect(review.getByRole("button", { name: "Strengthen advice" })).toHaveCount(0);
  await review.getByRole("button", { name: "Approve reflex" }).click();
  const dialog = page.getByRole("dialog", { name: /Approve reflex/ });
  await expect(dialog.getByText("Run make test before finishing.")).toBeVisible();
  await screenshot(page, "review-desktop-confirm");
  expect((await new AxeBuilder({ page }).analyze()).violations.filter(v => v.impact === "serious" || v.impact === "critical")).toEqual([]);
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  expect(decisions).toHaveLength(0);
  await expect(review.getByRole("button", { name: "Approve reflex" })).toBeFocused();
  await review.getByRole("button", { name: "Approve reflex" }).click();
  await dialog.getByRole("button", { name: "Confirm decision" }).click();
  await expect(dialog.getByText("Decision saved.")).toBeVisible();
  await expect(dialog.getByText(/This instinct remains prime after approval/)).toBeVisible();
  expect(decisions).toEqual([{ reference: route.reference, reflex: route.id, verb: "approve", revision: "b".repeat(64) }]);
  await dialog.getByRole("button", { name: "Done", exact: true }).click();
  const readiness = page.getByRole("region", { name: "Readiness and strength" });
  await expect(readiness).toContainText("Review: Approved");
  await expect(readiness).toContainText("Active advice");
  await expect(readiness).toContainText("prime · suggests the step");
  await screenshot(page, "review-desktop-saved");
  expect(new URL(page.url()).searchParams.get("reflex")).toBe(JSON.stringify([route.repo, route.reference, route.id]));
  await page.reload();
  await expect(page.getByRole("region", { name: "Readiness and strength" })).toContainText("Review: Approved");
});

test("a stale decision needs a fresh inspection before it can be confirmed again", async ({ page }) => {
  const { decisions } = await fixture(page, { stale: true });
  await page.getByRole("button", { name: "Approve reflex", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: /Approve reflex/ });
  await dialog.getByRole("button", { name: "Confirm decision" }).click();
  await expect(dialog.getByRole("alert")).toContainText("Nothing was written");
  await expect(dialog.getByRole("button", { name: "Confirm decision" })).toHaveCount(0);
  expect(decisions).toHaveLength(1);
  await dialog.getByRole("button", { name: "Reload review" }).click();
  await dialog.getByRole("button", { name: "Confirm decision" }).click();
  await expect(dialog.getByText("Decision saved.")).toBeVisible();
  expect(decisions[1]).toMatchObject({ revision: "b".repeat(64) });
});

test("strengthening is a separate choice with an explicit advice-only result", async ({ page }) => {
  const { decisions } = await fixture(page);
  await page.getByRole("button", { name: "Strengthen advice", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: /Strengthen advice/ });
  await expect(dialog).toContainText("This does not allow automatic execution");
  await dialog.getByRole("button", { name: "Confirm decision" }).click();
  await expect(dialog).toContainText("Advice strength: fire");
  await expect(dialog).toContainText("Automatic execution permission was not changed");
  expect(decisions[0]).toMatchObject({ verb: "strengthen" });
});

test("an agent-launched map explains its read-only fallback", async ({ page }) => {
  const { decisions } = await fixture(page, { enabled: false });
  const review = page.getByRole("region", { name: "Review this reflex" });
  await expect(review).toContainText("This map is read-only");
  await expect(review.getByRole("button", { name: "Approve reflex", exact: true })).toHaveCount(0);
  await review.getByText("Review at your terminal", { exact: true }).click();
  await expect(review).toContainText("adage review approve");
  expect(decisions).toHaveLength(0);
});

test("learning inputs remain separate from recorded activity counts", async ({ page }) => {
  await fixture(page);
  await page.goto(`${BASE}/?view=traces`);
  await page.getByText(/^Last conditioning ·/).click();
  const learning = page.getByRole("region", { name: "Learning history" });
  await expect(learning).toContainText("120 sessions used · 144 files read");
  await expect(learning).toContainText("not automatically recorded hook sessions");
  await expect(learning).toContainText("not a lifetime total");
});

test.describe("phone review", () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  test("the decision modal fits and its controls remain reachable", async ({ page }) => {
    await fixture(page);
    await page.getByRole("button", { name: "Approve reflex", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: /Approve reflex/ });
    await expect(dialog.getByRole("button", { name: "Confirm decision" })).toBeVisible();
    await screenshot(page, "review-phone-confirm");
    const [scroll, width] = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
    expect(scroll).toBeLessThanOrEqual(width);
    expect((await new AxeBuilder({ page }).analyze()).violations.filter(v => v.impact === "serious" || v.impact === "critical")).toEqual([]);
  });
});

test("a failed graph refresh does not misreport or repeat a saved decision", async ({ page }) => {
  const { decisions } = await fixture(page, { failRefresh: true });
  await page.getByRole("button", { name: "Approve reflex", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: /Approve reflex/ });
  await dialog.getByRole("button", { name: "Confirm decision" }).click();
  await expect(dialog.getByText("Decision saved.")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Confirm decision" })).toHaveCount(0);
  await expect(dialog.getByRole("button", { name: "Reload review" })).toHaveCount(0);
  expect(decisions).toHaveLength(1);
});


test("a first-run home can inspect and review its built-in instincts", async ({ page }) => {
  const { g, route } = await fixture(page, { instinct: true });
  g.routes = [route]; g.sessions = { total: 0, treated: 0, control: 0 }; g.events = [];
  await page.route("**/api/v1/health", r => r.fulfill({ path: new URL("./fixtures/health.json", import.meta.url).pathname, contentType: "application/json" }));
  await page.goto(BASE);
  await page.getByRole("button", { name: new RegExp(route.id) }).click();
  await expect(page.getByRole("region", { name: "Review this reflex" }).getByRole("button", { name: "Approve reflex", exact: true })).toBeVisible();
});
