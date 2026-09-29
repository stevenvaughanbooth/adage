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

test.describe("/how", () => {
  test("steps through chapters with the buttons, the keys and the URL", async ({ page }) => {
    await page.goto("/how?ch=miners&step=2");
    await expect(page.getByRole("heading", { level: 1 })).toContainText("From a transcript to a reflex");
    await expect(page.locator("#capmeta")).toContainText("02 Condition with miners");
    await expect(page.locator("#count")).toHaveText(/^3 \/ \d+$/);
    await page.getByRole("button", { name: "Next step" }).click();
    await expect(page.locator("#count")).toHaveText(/^4 \//);
    await expect(page).toHaveURL(/step=3/);
    await page.locator("#walk").click({ position: { x: 5, y: 5 } });
    await page.keyboard.press("ArrowLeft");
    await expect(page.locator("#count")).toHaveText(/^3 \//);
    await page.getByRole("button", { name: /Measure against control/ }).click();
    await expect(page.locator("#captitle")).toHaveText("Two arms, task by task");
    await expect(page.locator("#howgl")).toBeVisible();
  });

  test("every chapter builds and every step has words", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto("/how");
    const chapters = await page.locator("#chaplist .chap").count();
    expect(chapters).toBe(9);
    for (let c = 0; c < chapters; c++) {
      await page.locator("#chaplist .chap").nth(c).click();
      const steps = await page.locator("#ticks button").count();
      expect(steps).toBeGreaterThan(2);
      for (let s = 0; s < steps; s++) {
        await page.locator("#ticks button").nth(s).click();
        await expect(page.locator("#captitle")).not.toHaveText("");
        await expect(page.locator("#capbody")).not.toHaveText("");
      }
    }
    expect(errors).toEqual([]);
  });

  test("the Wilson calculator agrees with the Rust bound", async ({ page }) => {
    await page.goto("/how#m-evidence");
    await page.locator("#wn").fill("8");
    await page.locator("#wk").fill("7");
    // wilson_lower(7, 8) = 0.529 (crates/adage-tracks/src/model.rs)
    await expect(page.locator("#w-say")).toContainText("lower bound 0.53");
    await page.locator("#wn").fill("20");
    await page.locator("#wk").fill("20");
    await expect(page.locator("#w-say")).toContainText("it may fire");
  });

  for (const scheme of ["dark", "light"] as const) {
    test(`no axe violations (${scheme})`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
      await page.goto("/how");
      await expect(page.locator("#captitle")).not.toHaveText("");
      const r = await new AxeBuilder({ page }).exclude("#howgl").exclude("#howlayer").analyze();
      expect(r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
    });
  }

  test("a phone never scrolls sideways", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/how");
    await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(0);
  });
});

test.describe("the film plays once, on a fresh visit", () => {
  test("not on a link to a section", async ({ page }) => {
    await films(page, "hang");
    await page.goto("/#evidence");
    await expect(page.locator("#film")).toBeHidden();
    await expect(page.locator("main")).toHaveJSProperty("inert", false);
  });

  test("not when coming from /how, and not twice in a visit", async ({ page }) => {
    await films(page, "hang");
    await page.goto("/how");
    await page.getByRole("navigation", { name: "Site" }).getByRole("link", { name: "Evidence" }).click();
    await expect(page).toHaveURL(/\/#evidence$/);
    await expect(page.locator("#film")).toBeHidden();
    await page.goto("about:blank");
    await page.goto("/");
    await expect(page.locator("#film")).toBeHidden();
  });

  test("the hero's See how it works and the section's Learn more open /how", async ({ page }) => {
    await films(page, "fail");
    await page.goto("/#how");
    await expect(page.getByRole("link", { name: /^Learn more/ })).toHaveAttribute("href", "/how");
    await page.getByRole("link", { name: "See how it works" }).click();
    await expect(page).toHaveURL(/\/how$/);
  });
});

test("/how plays its tour by default, and stops when the visitor takes over", async ({ page }) => {
  await page.goto("/how");
  await page.locator("#walk").scrollIntoViewIfNeeded();
  await expect(page.getByRole("button", { name: "Pause" })).toBeVisible();
  await expect(page.locator("#count")).toHaveText(/^2 \//, { timeout: 20_000 });
  await page.getByRole("button", { name: "Next step" }).click();
  await expect(page.getByRole("button", { name: "Play" })).toBeVisible();
});

test("/how does not play by itself with reduced motion", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/how");
  await page.locator("#walk").scrollIntoViewIfNeeded();
  await expect(page.getByRole("button", { name: "Play" })).toBeVisible();
});

test("the hero tour plays by default and survives a hover and a teaser replay", async ({ page }) => {
  await films(page, "hang");
  await page.goto("/");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("button", { name: "Pause tour" })).toBeVisible();
  await page.locator("#panel").hover();
  await page.mouse.move(5, 5);
  await expect(page.getByRole("button", { name: "Pause tour" })).toBeVisible();
  await page.getByRole("link", { name: "Watch the teaser" }).click();
  await page.keyboard.press("Escape");
  await expect(page.locator("#film")).toBeHidden();
  await expect(page.getByRole("button", { name: "Pause tour" })).toBeVisible();
  await page.getByRole("tab", { name: "Reflexes" }).click();
  await expect(page.getByRole("button", { name: "Play tour" })).toBeVisible();
});

test("the film's speaker mutes and unmutes; the skip icon skips", async ({ page }) => {
  await films(page, "hang");
  await page.goto("/");
  await page.locator("#film").hover();
  const speaker = page.getByRole("button", { name: "Unmute" });
  await speaker.click();
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", false);
  await page.getByRole("button", { name: "Mute" }).click();
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", true);
  await expect(page.getByRole("button", { name: "Unmute" })).toBeVisible();
  await page.locator("#film").screenshot({ path: "test-results/film-controls.png" });
  await page.getByRole("button", { name: "Skip intro" }).click();
  await expect(page.locator("#film")).toBeHidden();
});
