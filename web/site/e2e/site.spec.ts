import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// Films: "hang" keeps the overlay up (the request never answers), "fail" makes the video error out.
async function films(page: Page, mode: "hang" | "fail") {
  await page.route("**/*.mp4", (r) => (mode === "fail" ? r.abort() : undefined));
}

async function skipFilm(page: Page) {
  await page.locator("#film").hover();
  await page.getByRole("button", { name: "Skip intro" }).click();
  await expect(page.locator("#film")).toBeHidden();
}

test("the page opens on the film; skipping lands on the hero", async ({ page }) => {
  await films(page, "hang");
  await page.goto("/");
  const film = page.getByRole("dialog", { name: "Adage teaser film" });
  await expect(film).toBeVisible();
  await expect(film).toBeFocused();
  await expect(page.locator("main")).toHaveJSProperty("inert", true);
  // The controls stay hidden until the pointer moves.
  await expect(page.locator(".film-bar")).toHaveCSS("opacity", "0");
  await film.hover();
  await expect(page.locator(".film-bar")).toHaveCSS("opacity", "1");
  await skipFilm(page);
  await expect(page.locator("#h")).toBeFocused();
  await expect(page.locator("main")).toHaveJSProperty("inert", false);
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Agent traces become reflexes.");
  await expect(page.getByRole("tab")).toHaveCount(5);
});

test("Escape and the close button dismiss it; Watch the teaser brings it back with sound", async ({ page }) => {
  await films(page, "hang");
  await page.goto("/");
  await page.keyboard.press("Escape");
  await expect(page.locator("#film")).toBeHidden();
  await page.getByRole("link", { name: "Watch the teaser" }).click();
  await expect(page.locator("#film")).toBeVisible();
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", false);
  await page.locator("#film").hover();
  await page.getByRole("button", { name: "Close the teaser" }).click();
  await expect(page.locator("#film")).toBeHidden();
  await expect(page.getByRole("link", { name: "Watch the teaser" })).toBeFocused();
});

test("a film that cannot load leaves the page at the hero", async ({ page }) => {
  await films(page, "fail");
  await page.goto("/");
  await expect(page.locator("#film")).toBeHidden();
  await expect(page.locator("main")).toHaveJSProperty("inert", false);
});

test("the film follows the theme, and the choice is remembered", async ({ page }) => {
  await films(page, "hang");
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/");
  await expect(page.locator("#film-video")).toHaveAttribute("data-src", /teaser-dark\.mp4$/);
  await skipFilm(page);
  await page.getByRole("button", { name: "Switch to the day theme" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  // Closed, the film fetches nothing; reopened, it plays the day cut.
  await expect(page.locator("#film-video")).toHaveAttribute("data-src", /teaser-dark\.mp4$/);
  await page.getByRole("link", { name: "Watch the teaser" }).click();
  await expect(page.locator("#film-video")).toHaveAttribute("data-src", /teaser-day\.mp4$/);
  await page.keyboard.press("Escape");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
});

test("the hero draws its scene", async ({ page }) => {
  await films(page, "fail");
  await page.goto("/");
  await expect(page.locator("#gl")).toBeVisible();
  await expect(page.locator(".nogl")).toHaveCount(0);
  await page.getByRole("tab", { name: "Reflexes" }).click();
  await expect(page.locator("#p-title")).toHaveText("Reflexes are learned from traces");
  await expect(page.locator(".lbl", { hasText: /fire.*make gen$/ })).toHaveCSS("opacity", "1", { timeout: 15_000 });
});

test("early access: success, a closed list, and a bad address", async ({ page }) => {
  await films(page, "fail");
  let open = true;
  await page.route("**/api/waitlist", (r) => r.fulfill(open ? { status: 200, json: { ok: true } } : { status: 503, json: { error: "closed" } }));
  await page.goto("/#access");
  const email = page.getByRole("textbox", { name: "Email address" });
  const status = page.locator("#signup-status");
  await email.fill("not-an-email");
  await page.getByRole("button", { name: "Join the list" }).click();
  await expect(status).toContainText("Enter an email address");
  await email.fill("dev@example.com");
  await page.getByRole("button", { name: "Join the list" }).click();
  await expect(status).toContainText("You're on the list");
  open = false;
  await email.fill("dev@example.com");
  await page.getByRole("button", { name: "Join the list" }).click();
  await expect(status).toContainText("aren't open yet");
});

for (const scheme of ["dark", "light"] as const) {
  test(`no axe violations (${scheme})`, async ({ page }) => {
    await films(page, "fail");
    await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
    await page.goto("/");
    await expect(page.locator("#film")).toBeHidden();
    const r = await new AxeBuilder({ page }).exclude("#gl").analyze();
    expect(r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
  });
}

test("unknown paths get the 404 page", async ({ page }) => {
  const r = await page.goto("/no-such-page");
  // vite preview answers with index.html for unknown paths; Vercel serves 404.html. Check the page itself.
  if (r && r.status() === 404) await expect(page.getByRole("heading", { level: 1 })).toHaveText("No reflex for this path.");
  await page.goto("/404.html");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("No reflex for this path.");
});

test("a phone never scrolls sideways", async ({ page }) => {
  await films(page, "fail");
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.locator("#film")).toBeHidden();
  await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBe(0);
});

test("Escape still closes the film after Play is pressed", async ({ page }) => {
  await films(page, "hang");
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/");
  await page.getByRole("button", { name: /Play the teaser/ }).click();
  await page.keyboard.press("Escape");
  await expect(page.locator("#film")).toBeHidden();
});
