// `pnpm test:e2e`: the map as the binary serves it. Two servers start from target/debug/adage
// (brought up to date by cargo first, so it embeds the committed page): `--demo` (sample traces playing) and an empty home (first run).
// Each gets a throwaway HOME and ADAGE_HOME. ADAGE_BIN overrides the binary.

import { defineConfig, devices } from "@playwright/test";
import { existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { DEMO_PORT, EMPTY_PORT } from "./e2e/ports";

const repo = resolve(import.meta.dirname, "../..");
const bin = process.env.ADAGE_BIN ?? join(repo, "target/debug/adage");
// The preinstalled Chromium, where there is one (no `playwright install` needed).
if (!process.env.PLAYWRIGHT_BROWSERS_PATH && existsSync("/opt/pw-browsers")) process.env.PLAYWRIGHT_BROWSERS_PATH = "/opt/pw-browsers";

function server(port: number, extra: string) {
  const home = mkdtempSync(join(tmpdir(), "adage-e2e-"));
  const build = process.env.ADAGE_BIN ? "" : `cargo build -q -p adage-cli --manifest-path '${repo}/Cargo.toml' && `;
  return {
    command: `${build}exec '${bin}' map --no-open --port ${port} ${extra}`,
    url: `http://127.0.0.1:${port}/api/v1/graph`,
    cwd: home,
    env: { HOME: home, ADAGE_HOME: join(home, ".adage"), NO_COLOR: "1" },
    reuseExistingServer: false,
    timeout: 600_000,
    stdout: "ignore" as const,
  };
}

export default defineConfig({
  testDir: "e2e",
  timeout: 45_000,
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? "line" : "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1440, height: 900 },
    contextOptions: { reducedMotion: "reduce" },
    // How it works draws with WebGL: software rendering, so it runs without a GPU.
    launchOptions: { args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] },
  },
  webServer: [server(DEMO_PORT, "--demo"), server(EMPTY_PORT, "")],
});
