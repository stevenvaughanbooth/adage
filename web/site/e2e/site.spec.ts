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
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", false);
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
  await page.emulateMedia({ reducedMotion: "reduce" });
  await films(page, "fail");
  let open = true;
  await page.route("**/api/waitlist", (r) => r.fulfill(r.request().method() === "GET" ? { status: 200, json: { available: true } } : open ? { status: 200, json: { ok: true } } : { status: 503, json: { error: "closed" } }));
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
  await expect(page.locator("#email")).toHaveValue("dev@example.com");
  await page.getByRole("link", { name: "Walk through how Adage works", exact: true }).click();
  await expect(page).toHaveURL(/\/how(?:[?#]|$)/);
});

test("closed registration asks for no address and can reopen without a reload", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await films(page, "fail");
  let available = false;
  let posts = 0;
  await page.route("**/api/waitlist", (route) => {
    if (route.request().method() === "POST") posts++;
    return route.fulfill({ json: { available } });
  });
  await page.goto("/#access");
  await expect(page.locator("#signup-status")).toContainText("aren't open yet");
  await expect(page.getByRole("textbox", { name: "Email address" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Join the list" })).toHaveCount(0);
  expect(posts).toBe(0);
  available = true;
  await page.getByRole("button", { name: "Check availability again" }).click();
  await expect(page.getByRole("textbox", { name: "Email address" })).toBeFocused();
  await expect(page.getByRole("button", { name: "Join the list" })).toBeEnabled();
});

test("an unavailable registration check recovers and a closing list preserves the address", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await films(page, "fail");
  let healthy = false;
  await page.route("**/api/waitlist", (route) => route.fulfill(
    route.request().method() === "POST" ? { status: 503, json: { error: "closed" } }
      : healthy ? { json: { available: true } } : { status: 502, json: {} },
  ));
  await page.goto("/#access");
  await expect(page.locator("#signup-status")).toContainText("Couldn't check registration");
  healthy = true;
  await page.getByRole("button", { name: "Check availability again" }).click();
  const email = page.locator("#email");
  await email.fill("dev@example.com");
  await page.getByRole("button", { name: "Join the list" }).click();
  await expect(page.locator("#signup-status")).toContainText("aren't open yet");
  await expect(email).toBeHidden();
  await page.getByRole("button", { name: "Check availability again" }).click();
  await expect(email).toBeFocused();
  await expect(email).toHaveValue("dev@example.com");
});

test("a stalled registration request unlocks without losing the address or double posting", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await films(page, "fail");
  let posts = 0;
  await page.route("**/api/waitlist", (route) => {
    if (route.request().method() === "GET") return route.fulfill({ json: { available: true } });
    posts++;
    // Keep the response pending; the client's timeout must release the form.
  });
  await page.goto("/#access");
  const email = page.locator("#email");
  await email.fill("dev@example.com");
  const join = page.getByRole("button", { name: "Join the list" });
  await join.click();
  await expect(join).toBeDisabled();
  await page.locator("#signup").evaluate((form: HTMLFormElement) => form.requestSubmit());
  await expect(page.locator("#signup-status")).toContainText("timed out", { timeout: 15_000 });
  await expect(join).toBeEnabled();
  await expect(email).toHaveValue("dev@example.com");
  expect(posts).toBe(1);
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
    await page.goto("/how?ch=judge&step=2");
    await expect(page.getByRole("heading", { level: 1 })).toContainText("From a transcript to a reflex");
    await expect(page.locator("#capmeta")).toContainText("04 Judge the evidence");
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
    // A link to the retired miners chapter lands on the chapter that replaced it.
    await page.goto("/how?ch=miners");
    await expect(page.locator("#capmeta")).toContainText("04 Judge the evidence");
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

  test("the math is a carousel: arrows, pills, keys and #m- links move it", async ({ page }) => {
    await page.goto("/how");
    const count = page.locator("#mcount");
    await expect(count).toHaveText("01 / 10");
    await expect(page.locator("#mprev")).toBeDisabled();
    await page.locator("#mnext").click();
    await expect(count).toHaveText("02 / 10");
    await expect(page.locator(".mathtoc a[aria-current]")).toHaveText("Graph engine");
    await expect(page).toHaveURL(/#m-graph$/);
    await page.locator(".mathtoc").getByRole("link", { name: "Skills" }).click();
    await expect(count).toHaveText("10 / 10");
    await expect(page.locator("#mnext")).toBeDisabled();
    await page.locator("#mtrack").focus();
    await page.keyboard.press("Home");
    await expect(count).toHaveText("01 / 10");
    // The track is as tall as the block in view.
    await expect.poll(() => page.evaluate(() => {
      const t = document.getElementById("mtrack")!;
      return Math.abs(t.offsetHeight - document.querySelector<HTMLElement>(".mblock.on")!.offsetHeight);
    })).toBeLessThan(2);

    await page.goto("/how#m-hook");
    await expect(count).toHaveText("08 / 10");
    await expect(page.locator("#m-hook")).toBeInViewport();
    await expect.poll(() => page.evaluate(() => Math.abs(scrollY - document.getElementById("math")!.offsetTop))).toBeLessThan(2);
  });

  test("the page moves a section at a time", async ({ page }) => {
    await page.goto("/how");
    const top = (id: string) => page.evaluate((i) => document.getElementById(i)!.offsetTop, id);
    const y = () => page.evaluate(() => Math.round(scrollY));
    await page.evaluate(() => scrollTo({ top: document.getElementById("walk")!.offsetTop, behavior: "instant" }));
    await page.mouse.move(720, 450);
    // A short wheel turn finishes the move: down to the next section, and back up.
    await page.mouse.wheel(0, 120);
    await expect.poll(y, { timeout: 5000 }).toBe(await top("math"));
    await page.mouse.move(720, 60);
    await page.mouse.wheel(0, -120);
    await expect.poll(y, { timeout: 5000 }).toBe(await top("walk"));

    // A section taller than the screen reads freely.
    await page.goto("/#evidence");
    const ev = await top("evidence");
    await expect.poll(y).toBe(ev);
    await page.mouse.move(720, 450);
    await page.mouse.wheel(0, 300);
    await page.waitForTimeout(800);
    expect(await y()).toBe(ev + 300);
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

  test("the hero opens the concrete example and Learn more opens /how", async ({ page }) => {
    await films(page, "fail");
    await page.goto("/#how");
    await expect(page.getByRole("link", { name: /^Learn more/ })).toHaveAttribute("href", "/how");
    await page.getByRole("link", { name: "See a reflex happen" }).click();
    await expect(page).toHaveURL(/#shortcuts$/);
    await expect(page.locator("#story-output")).toBeVisible();
    await page.getByRole("link", { name: /^Learn more/ }).click();
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
  await page.getByRole("button", { name: "Mute" }).click();
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", true);
  await expect(page.getByRole("button", { name: "Unmute" })).toBeVisible();
  await page.getByRole("button", { name: "Unmute" }).click();
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", false);
  await page.locator("#film").screenshot({ path: "test-results/film-controls.png" });
  await page.getByRole("button", { name: "Skip intro" }).click();
  await expect(page.locator("#film")).toBeHidden();
});


test("the illustrative trace becomes a reflex, then a directive, by keyboard", async ({ page }) => {
  await page.goto("/#shortcuts");
  await expect(page.locator("#film")).toBeHidden();
  await expect(page.locator("#story-code")).toContainText("generated files are out of date");
  await page.getByRole("button", { name: /The pattern becomes a reflex/ }).focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("#story-code")).toContainText("trigger   edit schema/*.json");
  await page.keyboard.press("Tab");
  await page.keyboard.press("Enter");
  await expect(page.locator("#story-code")).toContainText("Adage: Changes to schema/*.json");
  await expect(page.getByRole("button", { name: /The next session gets a shortcut/ })).toHaveAttribute("aria-pressed", "true");
  await expect(page.locator(".story-footnote")).toContainText("Illustrative sequence, not a benchmark");
});


test("blocked audible autoplay offers a click to play with sound", async ({ page }) => {
  await films(page, "hang");
  await page.addInitScript(() => {
    let attempts = 0;
    HTMLMediaElement.prototype.play = function () {
      this.dataset.playAttempts = String(++attempts);
      if (attempts === 1) return Promise.reject(new DOMException("Gesture required", "NotAllowedError"));
      return Promise.resolve();
    };
  });
  await page.goto("/");
  const play = page.getByRole("button", { name: /Play the teaser with sound/ });
  await expect(play).toBeVisible();
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", false);
  await play.click();
  await expect(play).toBeHidden();
  await expect(page.locator("#film-video")).toHaveAttribute("data-play-attempts", "2");
  await expect(page.locator("#film-video")).toHaveJSProperty("muted", false);
  await expect(page.locator("#film")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.locator("#film")).toBeHidden();
});
