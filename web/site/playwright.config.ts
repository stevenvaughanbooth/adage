// `pnpm test:e2e`: the built site, served by `vite preview`. The films are stubbed per test (headless
// Chromium has no H.264), and /api/waitlist is mocked (it is a Vercel Function, not part of the preview).

import { defineConfig, devices } from "@playwright/test";
import { existsSync } from "node:fs";

// The preinstalled Chromium, where there is one (no `playwright install` needed).
if (!process.env.PLAYWRIGHT_BROWSERS_PATH && existsSync("/opt/pw-browsers")) process.env.PLAYWRIGHT_BROWSERS_PATH = "/opt/pw-browsers";

export default defineConfig({
  testDir: "e2e",
  timeout: 45_000,
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? "line" : "list",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://127.0.0.1:4173",
    viewport: { width: 1440, height: 900 },
    // The hero draws with WebGL: software rendering, so it runs without a GPU.
    launchOptions: { args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] },
  },
  webServer: {
    command: "pnpm build && pnpm preview",
    url: "http://127.0.0.1:4173/",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
    stdout: "ignore",
  },
});
