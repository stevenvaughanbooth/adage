// The map as `adage map` serves it (playwright.config.ts starts the binary): every view loads,
// passes axe, works from the keyboard and fits a phone.

import AxeBuilder from "@axe-core/playwright";
import { type Page, expect, test } from "@playwright/test";
import { DEMO_PORT, EMPTY_PORT } from "./ports";

const DEMO = `http://127.0.0.1:${DEMO_PORT}`;
const EMPTY = `http://127.0.0.1:${EMPTY_PORT}`;
const VIEWS = ["map", "traces", "evidence", "health", "how"] as const;

async function axe(page: Page) {
  const r = await new AxeBuilder({ page }).analyze();
  return r.violations.filter((v) => v.impact === "serious" || v.impact === "critical").map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`);
}

async function ready(page: Page) {
  // The app has mounted (the top bar is there) before "not loading" means the data arrived.
  await expect(page.getByRole("navigation", { name: "Views" })).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText("Loading the map…")).toHaveCount(0, { timeout: 15_000 });
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

test("the inspector says where each number comes from", async ({ page }) => {
  await page.goto(`${DEMO}/`);
  await ready(page);
  await page.getByRole("navigation", { name: "Reflex tree" }).getByRole("button").first().click();
  const inspector = page.getByRole("complementary", { name: "Inspector" });
  await expect(inspector.getByRole("region", { name: /^(From conditioning|From the pack|Shipped with Adage) · Evidence$/ })).toBeVisible();
  const live = inspector.getByRole("region", { name: /^Live since installed · vs control$/ });
  await expect(live).toBeVisible();
  await expect(live.getByText(/^Control: traces where Adage withheld the reflex on purpose/)).toBeVisible();
});

test("the Evidence table separates conditioning from live counts", async ({ page }) => {
  await page.goto(`${DEMO}/?view=evidence`);
  await ready(page);
  const table = page.getByRole("region", { name: "Reflexes by turns saved" });
  await expect(table.getByRole("columnheader", { name: "From conditioning" })).toBeVisible();
  await expect(table.getByRole("columnheader", { name: "Live since installed" })).toBeVisible();
  // Every live cell carries its denominators, even before anything applied.
  const cells = await table.locator("tbody tr td:last-child").allInnerTexts();
  expect(cells.length).toBeGreaterThan(0);
  for (const c of cells) expect(c).toMatch(/^\d+\/\d+ · \d+\/\d+$/);
});

for (const width of [390, 820]) {
  test.describe(`filter drawer at ${width} px`, () => {
    test.use({ viewport: { width, height: 844 }, hasTouch: true });
    test("opens from the map, filters, traps focus and closes", async ({ page }) => {
      await page.goto(`${DEMO}/`);
      await ready(page);
      await expect(page.getByRole("navigation", { name: "Reflex tree" })).toBeHidden();
      const open = page.getByRole("button", { name: /^Filter/ });
      await open.click();
      const drawer = page.getByRole("dialog", { name: "Filter and reflex tree" });
      await expect(drawer).toBeVisible();
      expect(await drawer.evaluate((d) => d.contains(document.activeElement))).toBe(true);
      expect(await axe(page)).toEqual([]);

      // The strength filters work from here and show on the button.
      const fire = drawer.getByRole("group", { name: "Show on the map" }).getByRole("button", { name: /^fire/ });
      await fire.click();
      await expect(fire).toHaveAttribute("aria-pressed", "false");
      await expect(page).toHaveURL(/strength=prime%2Cinhibit|strength=prime,inhibit/);

      // Tab stays inside.
      for (let i = 0; i < 12; i++) await page.keyboard.press("Tab");
      expect(await drawer.evaluate((d) => d.contains(document.activeElement))).toBe(true);

      // Escape closes and focus returns to the button.
      await page.keyboard.press("Escape");
      await expect(drawer).toHaveCount(0);
      await expect(open).toBeFocused();
      await expect(open).toContainText("1");

      // The visible close button closes too; picking a reflex closes it and opens the inspector.
      await open.click();
      await drawer.getByRole("button", { name: "Close" }).click();
      await expect(drawer).toHaveCount(0);
      await open.click();
      await drawer.getByRole("navigation", { name: "Reflex tree" }).getByRole("button").first().click();
      await expect(drawer).toHaveCount(0);
      await expect(page.getByRole("complementary", { name: "Inspector" }).getByText("What the agent sees")).toBeVisible();
      const [scroll, w] = await page.evaluate(() => [document.documentElement.scrollWidth, window.innerWidth]);
      expect(scroll).toBeLessThanOrEqual(w);
    });
  });
}

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

test("how it works steps through every chapter, from the keyboard", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`${DEMO}/?view=how`);
  await ready(page);
  const caption = page.locator("[aria-live='polite']").filter({ has: page.getByRole("heading", { level: 2 }) });
  await expect(caption.getByRole("heading", { level: 2 })).toHaveText("Transcript lines stream in");
  // The stage draws (WebGL), or says why it cannot.
  await expect(page.locator("main canvas").first().or(page.getByText("This view draws in 3D with WebGL"))).toBeVisible();
  const chapters = page.getByRole("navigation", { name: "Chapters" }).getByRole("listitem");
  const n = await chapters.count();
  expect(n).toBe(9);
  // → walks every step of every chapter; the URL follows, so each step is shareable.
  const seen = new Set<string>();
  const heading = caption.getByRole("heading", { level: 2 });
  for (let i = 0; i < 150; i++) {
    const before = (await heading.textContent()) ?? "";
    seen.add(before);
    await page.keyboard.press("ArrowRight");
    await page.waitForTimeout(60);
    // At the last step of the last chapter, → does nothing.
    if (new URL(page.url()).searchParams.get("ch") === "measure" && (await heading.textContent()) === before) break;
  }
  expect(new URL(page.url()).searchParams.get("ch")).toBe("measure");
  expect(seen.size).toBeGreaterThan(30);
  // ← goes back across a chapter boundary to the previous chapter's last step.
  await page.goto(`${DEMO}/?view=how&ch=subgraphs&step=0`);
  await ready(page);
  await page.keyboard.press("ArrowLeft");
  await expect(page).toHaveURL(/ch=intent/);
  // The scoring step computes its numbers and draws its figure.
  await page.goto(`${DEMO}/?view=how&ch=subgraphs&step=9`);
  await ready(page);
  await expect(caption.getByText("lower bound 0.59 ≥ 0.50")).toBeVisible();
  await expect(caption.getByRole("img", { name: /Figure/ })).toBeVisible();
  expect(errors).toEqual([]);
});
