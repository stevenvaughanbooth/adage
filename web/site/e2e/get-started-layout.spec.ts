import { test, expect, type Page } from "@playwright/test";

async function expectFinalSectionInFlow(page: Page) {
  await expect.poll(() => page.evaluate(() => {
    const main = document.querySelector("main")!;
    const sections = [...main.querySelectorAll<HTMLElement>(":scope > section")];
    const last = sections.at(-1)!;
    const expectedTop = main.getBoundingClientRect().top + scrollY
      + sections.slice(0, -1).reduce((height, section) => height + section.offsetHeight, 0);
    return Math.abs(last.getBoundingClientRect().top + scrollY - expectedTop);
  })).toBeLessThanOrEqual(1);
}

for (const viewport of [
  { width: 1512, height: 862 },
  { width: 1280, height: 720 },
  { width: 390, height: 844 },
]) {
  test(`installation stays clear of the following section at ${viewport.width}×${viewport.height}`, async ({ page }) => {
    await page.setViewportSize(viewport);
    await page.emulateMedia({ reducedMotion: "no-preference" });
    await page.goto("/get-started");
    await page.evaluate(() => document.fonts.ready);
    await expectFinalSectionInFlow(page);

    // Expanding the final section changes its height and the scene's measured offset.
    await page.locator("#explore details summary").first().click();
    await expect(page.locator("#explore details").first()).toHaveAttribute("open", "");
    await page.evaluate(() => scrollTo({ top: 0, behavior: "instant" }));
    await expectFinalSectionInFlow(page);
    expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBe(0);
  });
}
