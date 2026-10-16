import { test, expect, type Page } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";

// Films: "hang" keeps the overlay up (the request never answers), "fail" makes the video error out.
async function films(page: Page, mode: "hang" | "fail") {
  await page.route("**/*.mp4", (r) => (mode === "fail" ? r.abort() : undefined));
}

// /how's sections hold as scenes on wide screens (src/scenes.ts): a control scrolled into view by
// its own position can sit under the next scene. Tests that work the walkthrough put it on screen
// first, as a person scrolling to it would.
async function toWalk(page: Page) {
  await page.evaluate(() => {
    let t = document.querySelector("main")!.getBoundingClientRect().top + scrollY;
    for (const s of document.querySelectorAll<HTMLElement>("main > section")) { if (s.id === "walk") break; t += s.offsetHeight; }
    scrollTo({ top: t, behavior: "instant" });
  });
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
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Learn from the work your agent repeats.");
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

test("release updates: success, a closed list, and a bad address", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await films(page, "fail");
  let open = true;
  await page.route("**/api/waitlist", (r) => r.fulfill(r.request().method() === "GET" ? { status: 200, json: { available: true } } : open ? { status: 200, json: { ok: true } } : { status: 503, json: { error: "closed" } }));
  await page.goto("/early-access");
  await expect(page.getByRole("heading", { level: 1 })).toHaveText("Get release updates");
  await expect(page.getByRole("link", { name: "Install the experiment" })).toHaveAttribute("href", "/get-started");
  const email = page.getByRole("textbox", { name: "Email address" });
  const status = page.locator("#signup-status");
  await email.fill("not-an-email");
  await page.getByRole("button", { name: "Join the list" }).click();
  await expect(status).toContainText("Enter an email address");
  await email.fill("dev@example.com");
  await page.getByRole("button", { name: "Join the list" }).click();
  await expect(status).toHaveText("You're on the list. We'll email you once, when the next release is ready.");
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
  await page.goto("/early-access");
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
  await page.goto("/early-access");
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
  await page.goto("/early-access");
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

// The header's tabs, each a page of its own, in the order the header lists them.
const TABS = [
  { name: "How it works", path: "/how", h1: /How Adage plugs into Claude Code/ },
  { name: "Packs", path: "/packs", h1: "Packs" },
  { name: "Evidence", path: "/evidence", h1: /fewer model calls/ },
  { name: "Get started", path: "/get-started", h1: "Get started" },
];

test.describe("every tab is a page of its own", () => {
  test("the header links each page, and marks the one you are on", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await films(page, "fail");
    for (const at of ["/", ...TABS.map((t) => t.path), "/early-access"]) {
      await page.goto(at);
      const nav = page.getByRole("navigation", { name: "Site" });
      for (const t of TABS) {
        const link = nav.getByRole("link", { name: t.name, exact: true });
        await expect(link).toHaveAttribute("href", t.path);
        if (t.path === at) await expect(link).toHaveAttribute("aria-current", "page");
        else await expect(link).not.toHaveAttribute("aria-current");
      }
      const pill = nav.getByRole("link", { name: "Try Adage" });
      await expect(pill).toHaveAttribute("href", "/get-started");
    }
  });

  test("a tab opens its page at the top, and the home page no longer holds it", async ({ page }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await films(page, "fail");
    await page.goto("/");
    for (const id of ["packs", "evidence", "start", "explore", "access"]) await expect(page.locator(`#${id}`)).toHaveCount(0);
    for (const t of TABS) {
      await page.goto("/");
      await page.getByRole("navigation", { name: "Site" }).getByRole("link", { name: t.name, exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`${t.path}$`));
      await expect(page.getByRole("heading", { level: 1 })).toHaveText(t.h1);
      expect(await page.evaluate(() => scrollY)).toBe(0);
    }
    await page.goto("/");
    await page.getByRole("navigation", { name: "Site" }).getByRole("link", { name: "Try Adage" }).click();
    await expect(page).toHaveURL(/\/get-started$/);
    await expect(page.getByRole("heading", { level: 1 })).toHaveText("Get started");
  });

  test("the home page ends with where to go next", async ({ page }) => {
    await films(page, "fail");
    await page.goto("/");
    const next = page.locator("#next");
    await expect(next.locator(".next-cta").getByRole("link", { name: "Try your own history" })).toHaveAttribute("href", "/get-started");
    await expect(next.locator(".next-cta").getByRole("link", { name: "Help improve the learner" })).toHaveAttribute("href", "https://github.com/stevenvaughanbooth/adage/blob/main/CONTRIBUTING.md");
    for (const t of TABS) {
      await expect(next.locator(".pagelinks").getByRole("link", { name: t.name })).toHaveAttribute("href", t.path);
    }
  });

  test("on a phone, the pages stay in the header and nothing scrolls sideways", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await films(page, "fail");
    for (const at of ["/", "/packs", "/evidence", "/get-started", "/early-access"]) {
      await page.goto(at);
      await page.evaluate(() => document.querySelectorAll(".reveal").forEach((n) => n.classList.add("in")));
      const nav = page.getByRole("navigation", { name: "Site" });
      await expect(nav.getByRole("link", { name: "Packs", exact: true })).toBeVisible();
      // The last tab is a sideways swipe away, never off the page.
      await nav.getByRole("link", { name: "Get started", exact: true }).scrollIntoViewIfNeeded();
      await expect(nav.getByRole("link", { name: "Get started", exact: true })).toBeInViewport();
      expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(0);
    }
  });

  for (const path of ["/packs", "/get-started", "/early-access"]) {
    for (const scheme of ["dark", "light"] as const) {
      test(`no axe violations on ${path} (${scheme})`, async ({ page }) => {
        await page.route("**/api/waitlist", (r) => r.fulfill({ json: { available: true } }));
        await page.emulateMedia({ colorScheme: scheme, reducedMotion: "reduce" });
        await page.goto(path);
        const r = await new AxeBuilder({ page }).analyze();
        expect(r.violations.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(" ")).join(", ")}`)).toEqual([]);
      });
    }
  }
});

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
    await toWalk(page);
    await expect(page.getByRole("heading", { level: 1 })).toContainText("How Adage plugs into Claude Code");
    await expect(page.getByRole("heading", { level: 2, name: /From a transcript to a reflex/ })).toBeVisible();
    await expect(page.locator("#capmeta")).toContainText("04 Check the evidence");
    await page.getByRole("button", { name: "Full tour" }).click();
    await expect(page.locator("#count")).toHaveText(/^3 \/ \d+$/);
    await page.getByRole("button", { name: "Next step" }).click();
    await expect(page.locator("#count")).toHaveText(/^4 \//);
    await expect(page).toHaveURL(/step=3/);
    await page.locator("#walk").click({ position: { x: 5, y: 5 } });
    await page.keyboard.press("ArrowLeft");
    await expect(page.locator("#count")).toHaveText(/^3 \//);
    await page.getByRole("button", { name: /Prove it helped/ }).click();
    await expect(page.locator("#captitle")).toHaveText("With and without, task by task");
    await expect(page.locator("#howgl")).toBeVisible();
    // A link to the retired miners chapter lands on the chapter that replaced it.
    await page.goto("/how?ch=miners");
    await expect(page.locator("#capmeta")).toContainText("04 Check the evidence");
  });

  test("every chapter builds and every step has words", async ({ page }) => {
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    await page.goto("/how");
    await toWalk(page);
    await page.getByRole("button", { name: "Full tour" }).click();
    const chapters = await page.locator("#chaplist .chap").count();
    expect(chapters).toBe(9);
    for (let c = 0; c < chapters; c++) {
      await page.locator("#chaplist .chap").nth(c).click();
      const steps = await page.locator("#ticks button").count();
      expect(steps).toBeGreaterThan(2);
      for (let s = 0; s < steps; s++) {
        await page.locator("#ticks button").nth(s).click();
        await expect(page.locator("#captitle")).not.toHaveText("");
        await expect(page.locator("#capplain")).not.toHaveText("");
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
    await page.locator(".mathtoc").getByRole("link", { name: "Measurement" }).click();
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
    await expect(count).toHaveText("09 / 10");
    await expect(page.locator("#m-hook")).toBeInViewport();
    await expect.poll(() => page.evaluate(() => Math.abs(scrollY - document.getElementById("math")!.offsetTop))).toBeLessThan(2);
  });

  test("sections hold as scenes: the next slides over, the one underneath recedes", async ({ page }) => {
    await page.goto("/how");
    await expect(page.locator("html")).toHaveClass(/\bscenes\b/);
    // Where each section sits in the page, counted from the stack (a held section's box moves).
    const flow = (id: string) => page.evaluate((i) => {
      const secs = [...document.querySelectorAll<HTMLElement>("main > section")];
      let t = document.querySelector("main")!.getBoundingClientRect().top + scrollY;
      for (const s of secs) { if (s.id === i) break; t += s.offsetHeight; }
      return Math.round(t);
    }, id);
    const walkTop = await flow("walk");
    // Halfway into the math sliding over: the walk holds at the top, and recedes halfway.
    await page.evaluate((y) => scrollTo({ top: y, behavior: "instant" }), walkTop + 450);
    await expect.poll(() => page.evaluate(() => Math.round(document.getElementById("walk")!.getBoundingClientRect().top))).toBe(0);
    await expect.poll(() => page.evaluate(() => Math.round(document.getElementById("math")!.getBoundingClientRect().top))).toBe(450);
    await expect.poll(() => page.evaluate(() => getComputedStyle(document.getElementById("walk")!).transform)).toMatch(/^matrix\(0\.97/);
    await expect(page.getByRole("button", { name: "Pause" })).toBeVisible();
    // Fully covered, the walk says so, and its tour rests.
    await page.evaluate((y) => scrollTo({ top: y, behavior: "instant" }), walkTop + 900);
    await expect(page.locator("#walk")).toHaveAttribute("data-covered", "");
    await expect(page.getByRole("button", { name: "Play" })).toBeVisible();
    // Back up, it is uncovered.
    await page.evaluate((y) => scrollTo({ top: y, behavior: "instant" }), walkTop);
    await expect(page.locator("#walk")).not.toHaveAttribute("data-covered", "");
  });

  test("links reach held sections, above and below", async ({ page }) => {
    await films(page, "fail");
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/");
    const y = () => page.evaluate(() => Math.round(scrollY));
    await page.evaluate(() => scrollTo({ top: document.documentElement.scrollHeight, behavior: "instant" }));
    // Every section above is held under the last one; a link still finds where each one starts.
    await page.getByRole("link", { name: "See a reflex happen" }).evaluate((a: HTMLAnchorElement) => a.click());
    const top = await page.evaluate(() => {
      const secs = [...document.querySelectorAll<HTMLElement>("main > section")];
      let t = document.querySelector("main")!.getBoundingClientRect().top + scrollY;
      for (const s of secs) { if (s.id === "shortcuts") break; t += s.offsetHeight; }
      return Math.round(t);
    });
    await expect.poll(y).toBe(top);
    await expect(page).toHaveURL(/#shortcuts$/);
    await page.getByRole("link", { name: "Adage, back to the top" }).evaluate((a: HTMLAnchorElement) => a.click());
    await expect.poll(y).toBe(0);
  });

  test("phones scroll plainly: nothing holds", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/how");
    await expect(page.locator("html")).not.toHaveClass(/\bscenes\b/);
    expect(await page.evaluate(() => getComputedStyle(document.getElementById("walk")!).position)).toBe("static");
  });

  test("the quick tour stops at the key steps, and the choice is remembered", async ({ page }) => {
    await page.goto("/how");
    await toWalk(page);
    await expect(page.getByRole("button", { name: "Quick tour" })).toHaveAttribute("aria-pressed", "true");
    await expect(page.locator("#capmeta")).toContainText("01 Read what the agent did");
    await expect(page.locator("#count")).toHaveText("1 / 2");
    await page.getByRole("button", { name: "Next step" }).click();
    await expect(page.locator("#count")).toHaveText("2 / 2");
    await page.getByRole("button", { name: "Next step" }).click();
    await expect(page.locator("#capmeta")).toContainText("02 Spot the patterns");
    await page.getByRole("button", { name: "Full tour" }).click();
    await expect(page.locator("#count")).toHaveText(/^1 \/ 11$/);
    await page.reload();
    await expect(page.getByRole("button", { name: "Full tour" })).toHaveAttribute("aria-pressed", "true");
  });

  test("each step reads plainly first, with the precise account under More detail", async ({ page }) => {
    await page.goto("/how?ch=judge&step=3");
    await toWalk(page);
    await expect(page.locator("#capplain")).toContainText("like a restaurant with only a few reviews");
    await expect(page.locator("#capbody")).toBeHidden();
    await page.getByRole("button", { name: "More detail" }).click();
    await expect(page.locator("#capbody")).toBeVisible();
    await expect(page.locator("#capbody")).toContainText("Wilson lower bound");
    await expect(page.locator("#capfig")).toBeVisible();
    // A failed gate reads as failing.
    await page.getByRole("button", { name: "Full tour" }).click();
    await page.getByRole("button", { name: "Next step" }).click();
    await expect(page.locator("#capchips")).toContainText("✗ LB 0.41 < 0.80");
    // Detail stays open from step to step, and D closes it.
    await expect(page.locator("#capbody")).toBeVisible();
    await page.locator("#walk").click({ position: { x: 5, y: 5 } });
    await page.keyboard.press("d");
    await expect(page.locator("#capbody")).toBeHidden();
    await expect(page.getByRole("button", { name: "More detail" })).toHaveAttribute("aria-expanded", "false");
  });

  test("a dotted word shows its definition on hover, focus or tap", async ({ page }) => {
    await page.goto("/how?ch=subgraphs&step=10");
    await toWalk(page);
    const term = page.locator("#capplain .gloss", { hasText: "reflex" });
    await term.hover();
    await expect(page.getByRole("tooltip")).toBeVisible();
    await expect(page.getByRole("tooltip")).toContainText("A small learned rule");
    await page.mouse.move(5, 5);
    await expect(page.getByRole("tooltip")).toBeHidden();
    await term.focus();
    await expect(page.getByRole("tooltip")).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("tooltip")).toBeHidden();
    // The math's words are marked too.
    await expect(page.locator("#m-evidence .mshort .gloss").first()).toBeAttached();
  });

  test("a step links into its math, and the math links back to the walkthrough", async ({ page }) => {
    // Where the links land, not how they glide there: jumps are instant with reduced motion.
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/how?ch=judge&step=3");
    await toWalk(page);
    const link = page.getByRole("link", { name: "The math: Evidence →" });
    await link.click();
    await expect(page.locator("#mcount")).toHaveText("03 / 10");
    await expect(page.locator("#m-evidence")).toBeInViewport();
    // Read down to the end of the block, where the math's end meets the bottom of the screen (once
    // the track has grown to the block's height, and the math holds where its new end is).
    await expect.poll(() => page.evaluate(() => {
      const m = document.getElementById("math")!;
      const grown = Math.abs(document.getElementById("mtrack")!.offsetHeight - document.getElementById("m-evidence")!.offsetHeight) < 2;
      return grown && m.style.getPropertyValue("--pin") === `${Math.min(0, innerHeight - m.offsetHeight)}px`;
    })).toBe(true);
    await page.evaluate(() => {
      const m = document.getElementById("math")!;
      let t = document.querySelector("main")!.getBoundingClientRect().top + scrollY;
      for (const s of document.querySelectorAll<HTMLElement>("main > section")) { if (s === m) break; t += s.offsetHeight; }
      scrollTo({ top: t + Math.max(0, m.offsetHeight - innerHeight), behavior: "instant" });
    });
    await page.locator("#m-evidence").getByRole("link", { name: "← See it in the walkthrough: Check the evidence" }).click();
    await expect(page.locator("#capmeta")).toContainText("04 Check the evidence");
    await expect(page.locator("#walk")).toBeInViewport();
  });

  test("the uplift calculator says when telling is worth it", async ({ page }) => {
    await page.goto("/how#m-uplift");
    await expect(page.locator("#u-say")).toContainText("Telling adds 18 points. Worth saying");
    await page.locator("#us").fill("95");
    await expect(page.locator("#u-say")).toContainText("stays quiet");
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
    await page.goto("/#how");
    await expect(page.locator("#film")).toBeHidden();
    await expect(page.locator("main")).toHaveJSProperty("inert", false);
  });

  test("not when coming from /how, and not twice in a visit", async ({ page }) => {
    await films(page, "hang");
    await page.goto("/how");
    await page.getByRole("link", { name: "Adage, home" }).click();
    await expect(page).toHaveURL(/\/$/);
    await expect(page.locator("#film")).toBeHidden();
    await page.goto("about:blank");
    await page.goto("/");
    await expect(page.locator("#film")).toBeHidden();
  });

  test("the hero opens the concrete example and Learn more opens /how", async ({ page }) => {
    await films(page, "fail");
    // Where the links lead, not how the page glides there: jumps are instant with reduced motion.
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto("/#how");
    await expect(page.getByRole("link", { name: /^Learn more/ })).toHaveAttribute("href", "/how");
    // The hero is held under the scenes that slid over it: once the page has landed on #how, scroll
    // back up to the hero, as a person would.
    await expect.poll(() => page.evaluate(() => scrollY)).toBeGreaterThan(0);
    await page.evaluate(() => scrollTo({ top: 0, behavior: "instant" }));
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


test("a pack, file by file: a shared reflex, and an instinct whose blanks a repository fills", async ({ page }) => {
  await page.goto("/packs");
  const files = page.locator("#pack-files");
  const code = page.locator("#pack-code");
  // A reflex pack, read from the repository as shared: its manifest first, then any file by keyboard.
  await expect(files.getByRole("button", { name: "pack.toml" })).toHaveAttribute("aria-pressed", "true");
  await expect(code).toContainText('origin = "reflex"');
  await files.getByRole("button", { name: "routes/read-linked-files.yaml" }).focus();
  await page.keyboard.press("Enter");
  await expect(code).toContainText("summary: Read the files the work item links, together");
  await files.getByRole("button", { name: "pack.sig" }).click();
  await expect(code).toContainText("BEGIN SSH SIGNATURE");
  await expect(page.locator("#pack-fill-row")).toBeHidden();
  // The instinct: the same format; its template's blanks, then what a repository's files put there.
  await page.getByRole("button", { name: /An instinct Adage ships/ }).click();
  await expect(page.getByRole("button", { name: /An instinct Adage ships/ })).toHaveAttribute("aria-pressed", "true");
  await expect(code).toContainText('origin = "instinct"');
  await files.getByRole("button", { name: "routes/run-tests-in-project-env.yaml" }).click();
  await expect(code.locator("mark.blank")).toHaveText(["{runner}", "{signature}", "{command}"]);
  const fill = page.getByRole("button", { name: "Fill in the blanks" });
  await fill.click();
  await expect(code.locator("mark.filled")).toHaveText(["exec python3 -m pytest", "No module named pytest", ".venv/bin/python -m pytest"]);
  await expect(page.getByRole("button", { name: "Show the blanks" })).toHaveAttribute("aria-pressed", "true");
  await files.getByRole("button", { name: "CLOSURE" }).click();
  await expect(code).toContainText("detector U11 -");
  await expect(page.locator("#packs .base-packs li")).toHaveCount(8);
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
