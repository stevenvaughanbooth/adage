// `pnpm test:e2e`: the built site, served by `vite preview`. The films are stubbed per test (headless
// Chromium has no H.264), and /api/waitlist is mocked (it is a Vercel Function, not part of the preview).

import { defineConfig, devices } from "@playwright/test";
import { existsSync } from "node:fs";

// The preinstalled Chromium, where there is one (no `playwright install` needed).
if (!process.env.PLAYWRIGHT_BROWSERS_PATH && existsSync("/opt/pw-browsers")) process.env.PLAYWRIGHT_BROWSERS_PATH = "/opt/pw-browsers";

// Allow isolated local runs when the usual preview port is already occupied.
const port = Number(process.env.ADAGE_SITE_E2E_PORT ?? 4173);
if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("ADAGE_SITE_E2E_PORT must be a port from 1 to 65535");
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "e2e",
  timeout: 45_000,
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? "line" : "list",
  use: {
    ...devices["Desktop Chrome"],
    baseURL,
    viewport: { width: 1440, height: 900 },
    // The hero draws with WebGL: software rendering, so it runs without a GPU.
    launchOptions: { args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] },
  },
  webServer: {
    command: `pnpm build && pnpm exec vite preview --host 127.0.0.1 --port ${port} --strictPort`,
    url: `${baseURL}/`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    stdout: "ignore",
  },
});
