// Render the editable README artwork using the website's existing Playwright dependency.
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { mkdir, stat } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const requireSite = createRequire(new URL("../../../web/site/package.json", import.meta.url));
const { chromium } = requireSite("@playwright/test");
const macChrome = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const executablePath = process.env.CHROMIUM_EXECUTABLE || (process.platform === "darwin" && existsSync(macChrome) ? macChrome : undefined);
const output = resolve(process.argv[2] || resolve(here, "../../assets/readme-header.jpg"));
const browser = await chromium.launch({ executablePath, args: ["--allow-file-access-from-files", "--font-render-hinting=none", "--disable-lcd-text"] });
try {
  const page = await browser.newPage({ viewport: { width: 1200, height: 630 }, deviceScaleFactor: 1 });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.on("requestfailed", request => errors.push(request.url()));
  await page.goto(pathToFileURL(resolve(here, "index.html")).href);
  await page.evaluate(async () => {
    await Promise.all(["Bricolage Grotesque", "Hanken Grotesk", "Red Hat Mono"].map(font => document.fonts.load(`16px "${font}"`)));
    await document.fonts.ready;
  });
  const ready = await page.evaluate(() => ["Bricolage Grotesque", "Hanken Grotesk", "Red Hat Mono"].every(font => document.fonts.check(`16px "${font}"`)));
  if (!ready || errors.length) throw new Error(`Header assets failed: ${errors.join(", ")}`);
  await mkdir(dirname(output), { recursive: true });
  await page.screenshot({ path: output, type: "jpeg", quality: 92 });
  console.log(`Rendered ${output} (1200×630, ${(await stat(output)).size} bytes)`);
} finally {
  await browser.close();
}
