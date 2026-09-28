// The map as `adage map` serves it (playwright.config.ts starts the binary): every view loads,
// passes axe, works from the keyboard and fits a phone.

import AxeBuilder from "@axe-core/playwright";
import { type Page, expect, test } from "@playwright/test";
import { DEMO_PORT, EMPTY_PORT } from "./ports";

const DEMO = `http://127.0.0.1:${DEMO_PORT}`;
const EMPTY = `http://127.0.0.1:${EMPTY_PORT}`;
const VIEWS = ["map", "traces", "evidence", "health"] as const;

async function axe(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
}

async function ready(page: Page) {
  await expect(page.getByText("Loading the map…")).toHaveCount(0);
  await page.waitForTimeout(400);
}

for (const colorScheme of ["light", "dark"] as const) {
  test.describe(`${colorScheme} theme`, () => {
    test.use({ colorScheme });
    for (const view of VIEWS) {
      test(`${view} has no serious axe violations`, async ({ page }) => {
        await page.goto(`${DEMO}/?view=${view}`);
        await ready(page);
        await expect(page.getByRole("heading", { level: 1 })).toHaveCount(1);
        expect(await axe(page)).toEqual([]);
      });
    }
    test("a selected reflex passes axe", async ({ page }) => {
      await page.goto(`${DEMO}/`);
      await ready(page);
      await page.getByRole("navigation", { name: "Reflex tree" }).getByRole("button").first().click();
      await expect(page.getByRole("complementary", { name: "Inspector" }).getByText("What the agent sees")).toBeVisible();
      expect(await axe(page)).toEqual([]);
    });
  });
}

test("the palette takes typing at once, keeps focus, and closes on Escape", async ({ page }) => {
  await page.goto(`${DEMO}/`);
  await ready(page);
  await page.keyboard.press("ControlOrMeta+k");
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await expect(dialog).toBeVisible();
  await page.keyboard.type("lookup");
  await expect(dialog.getByRole("combobox")).toHaveValue("lookup");
  for (let i = 0; i < 5; i++) await page.keyboard.press("Tab");
  expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("Space presses a focused button instead of playing the replay", async ({ page }) => {
  await page.goto(`${DEMO}/`);
  await ready(page);
  const expanded = page.getByRole("button", { name: "Expanded" });
  await expanded.focus();
  await page.keyboard.press(" ");
  await expect(expanded).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: "Replay how the map formed" })).toBeVisible();
});

test("the inspector measures evidence against the bar the home conditions with", async ({ page }) => {
  const g = await (await page.request.get(`${DEMO}/api/v1/graph`)).json();
  await page.goto(`${DEMO}/`);
  await ready(page);
  await page.getByRole("navigation", { name: "Reflex tree" }).getByRole("button").first().click();
  await expect(page.getByText(`fire bar ${g.bars.fire_lower_bound.toFixed(2)}`)).toBeVisible();
});

test("a dropped stream says offline, not connecting", async ({ page }) => {
  await page.route("**/api/v1/events", (r) => r.abort());
  await page.goto(`${DEMO}/`);
  await ready(page);
  await expect(page.getByTestId("live-pill")).toContainText("Offline");
});

test("first run says exactly what to run", async ({ page }) => {
  await page.goto(`${EMPTY}/`);
  await expect(page.getByRole("heading", { name: /Your first reflex forms/ })).toBeVisible();
  await expect(page.getByRole("button", { name: "Copy adage init" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Copy adage condition" })).toBeVisible();
  expect(await axe(page)).toEqual([]);
});

test.describe("phone", () => {
  test.use({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  for (const [base, view] of [...VIEWS.map((v) => [DEMO, v] as const), [EMPTY, "map"] as const]) {
    test(`${base === EMPTY ? "first run" : view} fits without sideways scrolling`, async ({ page }) => {
      await page.goto(`${base}/?view=${view}`);
      await ready(page);
      const [scroll, width] = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
      expect(scroll).toBeLessThanOrEqual(width);
      await expect(page.getByRole("navigation", { name: "Views" })).toBeVisible();
    });
  }
});
