#!/usr/bin/env node
// Development aid: screenshot a running map. `node scripts/shot.mjs <url> <out.png> [day|night] [width] [height]`
// Uses the preinstalled Chromium (PLAYWRIGHT_BROWSERS_PATH, default /opt/pw-browsers).

import { chromium } from "@playwright/test";

const [url, out, theme = "night", w = "1440", h = "900"] = process.argv.slice(2);
if (!url || !out) {
  console.error("usage: node scripts/shot.mjs <url> <out.png> [day|night] [width] [height]");
  process.exit(2);
}
process.env.PLAYWRIGHT_BROWSERS_PATH ??= "/opt/pw-browsers";
const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: Number(w), height: Number(h) },
  colorScheme: theme === "day" ? "light" : "dark",
  reducedMotion: "reduce",
});
const errors = [];
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(url);
await page.waitForTimeout(1500);
await page.screenshot({ path: out });
await browser.close();
if (errors.length) console.error(errors.join("\n"));
