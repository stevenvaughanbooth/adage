#!/usr/bin/env node
// Development aid: walk a running map through its main interactions and screenshot each step.
// `node scripts/walk.mjs <url> <out-dir> [day|night]` (preinstalled Chromium).

import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { join } from "node:path";

const [url, out, theme = "night"] = process.argv.slice(2);
if (!url || !out) {
  console.error("usage: node scripts/walk.mjs <url> <out-dir> [day|night]");
  process.exit(2);
}
mkdirSync(out, { recursive: true });
process.env.PLAYWRIGHT_BROWSERS_PATH ??= "/opt/pw-browsers";
const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 1440, height: 900 },
  colorScheme: theme === "day" ? "light" : "dark",
  reducedMotion: "reduce",
});
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
const shot = async (name) => {
  await page.waitForTimeout(700);
  await page.screenshot({ path: join(out, `${name}-${theme}.png`) });
  console.log(`${name}: ${page.url()}`);
};

await page.goto(url);
await page.waitForTimeout(1500);
await shot("1-map");
// Select the first reflex node on the canvas.
const reflex = page.locator(".react-flow__node").filter({ hasText: "REFLEX" }).first();
if (await reflex.count()) {
  await reflex.click();
  await shot("2-inspector");
}
await page.keyboard.press("Escape");
await page.keyboard.press("Meta+k");
await page.keyboard.press("Control+k");
await page.keyboard.type("dead");
await shot("3-palette");
await page.keyboard.press("Escape");
for (const [name, key] of [["4-traces", "t"], ["5-evidence", "e"], ["6-health", "h"]]) {
  await page.keyboard.press("g");
  await page.keyboard.press(key);
  await shot(name);
}
await page.keyboard.press("?");
await shot("7-shortcuts");
await browser.close();
if (errors.length) {
  console.error("console errors:\n" + errors.join("\n"));
  process.exit(1);
}
