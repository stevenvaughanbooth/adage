// scripts/check-plugin.mjs on a copy of the files it reads, changed one way at a time: it passes
// with the one plugin as committed, and fails when the marketplace lists a second plugin, when the
// description says the plugin needs a binary on PATH or names Homebrew, when the retired alias's
// directory comes back, when the source URL requires SSH setup, and when the shim stops placing
// adage-train (T-O1-04, T-O12-04).
//
//   node scripts/test-check-plugin.mjs
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { copyFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const root = mkdtempSync(path.join(tmpdir(), "adage-check-plugin-test-"))
// Everything check-plugin reads, and nothing else.
const FILES = ["Cargo.toml", ".claude-plugin/marketplace.json", ".github/workflows/ci.yml"]
const DIRS = ["plugin/claude-code", "crates/adage-hook/tests/wire"]
const check = () => spawnSync(process.execPath, [path.join(repo, "scripts/check-plugin.mjs"), "--root", root], { encoding: "utf8" })
const market = path.join(root, ".claude-plugin/marketplace.json")
const shim = path.join(root, "plugin/claude-code/scripts/session-start")
// Change one file, check that check-plugin fails saying `want`, and put the file back.
const refuses = (file, change, want, what) => {
  const before = readFileSync(file, "utf8")
  change()
  const r = check()
  assert.notEqual(r.status, 0, `check-plugin accepted ${what}`)
  assert.match(r.stderr, want, `${what}: ${r.stderr}`)
  writeFileSync(file, before)
}
try {
  for (const f of FILES) {
    mkdirSync(path.dirname(path.join(root, f)), { recursive: true })
    copyFileSync(path.join(repo, f), path.join(root, f))
  }
  for (const d of DIRS) {
    cpSync(path.join(repo, d), path.join(root, d), { recursive: true, filter: (s) => !s.split(path.sep).includes("dist") })
  }
  let r = check()
  assert.equal(r.status, 0, r.stdout + r.stderr)
  assert.match(r.stdout, /^plugin adage \S+: ok$/m)

  const m = JSON.parse(readFileSync(market, "utf8"))
  // Claude Code resolves owner/repository git-subdir sources through SSH, which fails on a
  // fresh machine without host trust. Preserve the explicit public HTTPS install route.
  for (const url of ["stevenvaughanbooth/adage", "git@github.com:stevenvaughanbooth/adage.git", "ssh://git@github.com/stevenvaughanbooth/adage.git"]) {
    refuses(market, () => {
      const plugin = { ...m.plugins[0], source: { ...m.plugins[0].source, url } }
      writeFileSync(market, JSON.stringify({ ...m, plugins: [plugin] }, null, 2))
    }, /url: https:\/\/github\.com\/stevenvaughanbooth\/adage\.git.*without SSH setup/, `an SSH-dependent source URL (${url})`)
  }
  refuses(market, () => {
    const second = { ...m.plugins[0], name: "adage-exec", source: { ...m.plugins[0].source, path: "plugin/claude-code-exec" } }
    writeFileSync(market, JSON.stringify({ ...m, plugins: [...m.plugins, second] }, null, 2))
  }, /must list exactly one plugin, adage \(found adage, adage-exec\)/, "a second plugin")
  refuses(market, () => {
    writeFileSync(market, JSON.stringify({ ...m, plugins: [{ ...m.plugins[0], description: "Adage. Needs adage on PATH." }] }, null, 2))
  }, /still says it needs a binary on PATH/, "an on-PATH description")
  refuses(market, () => {
    writeFileSync(market, JSON.stringify({ ...m, plugins: [{ ...m.plugins[0], description: "Adage, or brew install it." }] }, null, 2))
  }, /names Homebrew/, "a Homebrew description")
  const alias = path.join(root, "plugin/claude-code-exec")
  mkdirSync(alias, { recursive: true })
  r = check()
  assert.notEqual(r.status, 0, "check-plugin accepted the alias directory")
  assert.match(r.stderr, /the retired adage-exec alias is gone/)
  rmSync(alias, { recursive: true })
  refuses(shim, () => {
    writeFileSync(shim, readFileSync(shim, "utf8").replace("bundled adage-train;", "true;"))
  }, /does not place the bundled adage-train/, "a shim without adage-train")

  r = check()
  assert.equal(r.status, 0, r.stdout + r.stderr)
  console.log("check-plugin: one HTTPS plugin passes; SSH-dependent sources, a second plugin, an on-PATH or Homebrew description, the alias directory and a shim without adage-train each fail (T-O1-04)")
} finally {
  rmSync(root, { recursive: true, force: true })
}
