#!/usr/bin/env node
// CI: rebuild the map and fail if the committed embedded copy differs (the binary would ship a
// page that does not match its source).

import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const web = join(dirname(fileURLToPath(import.meta.url)), "..");
const repo = join(web, "..", "..");
const run = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, stdio: "inherit" });

run("pnpm", ["build"], web);
run("node", ["scripts/embed.mjs"], web);
const paths = ["crates/adage-cli/assets/map", "crates/adage-cli/src/map/assets_list.rs"];
const status = execFileSync("git", ["status", "--porcelain", "--", ...paths], { cwd: repo, encoding: "utf8" });
if (status.trim()) {
  console.error(`The embedded map is stale. Run \`pnpm assets\` in web/map and commit:\n${status}`);
  run("git", ["--no-pager", "diff", "--stat", "--", ...paths], repo);
  process.exit(1);
}
console.log("The embedded map matches web/map.");
