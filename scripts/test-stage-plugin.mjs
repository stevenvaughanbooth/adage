// The release publisher (.github/workflows/publish-marketplace.yml runs scripts/stage-plugin.mjs,
// then check-plugin.mjs --staged) must stage one plugin with three binaries per target and reject
// missing or incorrect archive checksums before extraction. Test archives and binaries are tiny
// fake programs; this proves packaging integrity, not native execution.
//
//   T-O1-01  --bins lacking adage-train for one target fails, naming that target
//   T-O2-04  the workflow's local smoke: every target staged from --bins passes check-plugin --staged,
//            and the archive route accepts a verified set and rejects a missing or wrong .sha256
import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { execFileSync, spawnSync } from "node:child_process"
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { COMPANIONS, PLATFORMS } from "../npm/config.mjs"

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const host = PLATFORMS.find((p) => p.os === process.platform && p.cpu === process.arch)
assert(host && process.platform !== "win32", "run the archive staging test on supported Unix")
const root = mkdtempSync(path.join(tmpdir(), "adage-stage-test-"))
const version = readFileSync(path.join(repo, "Cargo.toml"), "utf8").match(/^\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/ms)[1]
// The apps a release archives, and the binary each holds: adage (adage-cli) and its companions.
const APPS = [["adage-cli", "adage"], ...COMPANIONS.map((c) => [c.app, c.name])]
assert.deepEqual(APPS.map(([, b]) => b), ["adage", "adage-exec", "adage-train"], "the plugin bundles three binaries")
const fake = (file) => {
  mkdirSync(path.dirname(file), { recursive: true })
  writeFileSync(file, `#!/bin/sh\necho 'adage ${version}'\n`)
  chmodSync(file, 0o755)
}
const run = (script, args) => spawnSync(process.execPath, [script, ...args], { cwd: repo, encoding: "utf8" })
try {
  // T-O1-01 and T-O2-04 (--bins): every target, every binary it needs; one adage-train missing.
  const bins = path.join(root, "bins")
  for (const p of PLATFORMS) {
    for (const [, bin] of APPS) {
      if (bin === "adage-exec" && p.os === "win32") continue
      fake(path.join(bins, p.target, p.os === "win32" ? `${bin}.exe` : bin))
    }
  }
  const full = path.join(root, "full")
  let result = run("scripts/stage-plugin.mjs", ["--bins", bins, "--out", full])
  assert.equal(result.status, 0, result.stdout + result.stderr)
  const sumsAll = readFileSync(path.join(full, "plugin", "claude-code", "dist", "SHA256SUMS"), "utf8")
  for (const p of PLATFORMS) {
    for (const [, bin] of APPS) {
      const exe = p.os === "win32" ? `${bin}.exe` : bin
      const listed = sumsAll.includes(`  ${p.target}/${exe}\n`)
      assert.equal(listed, !(bin === "adage-exec" && p.os === "win32"), `${p.target}/${exe} in SHA256SUMS`)
    }
  }
  assert.ok(!existsSync(path.join(full, "plugin", "claude-code-exec")), "no retired alias is staged")
  assert.deepEqual(readdirSync(path.join(full, "plugin")), ["claude-code"], "one plugin")
  result = run("scripts/check-plugin.mjs", ["--staged", full])
  assert.equal(result.status, 0, result.stdout + result.stderr)
  const lacking = PLATFORMS.find((p) => p.os === "linux" && p.cpu === "arm64")
  rmSync(path.join(bins, lacking.target, "adage-train"))
  result = run("scripts/stage-plugin.mjs", ["--bins", bins, "--out", path.join(root, "lacking")])
  assert.notEqual(result.status, 0, "a release without adage-train for every target is refused")
  assert.match(result.stderr, new RegExp(`no adage-train for ${lacking.target}`))
  // ... and a staged tree without it fails the publisher's check, naming the file.
  rmSync(path.join(full, "plugin", "claude-code", "dist", lacking.target, "adage-train"))
  result = run("scripts/check-plugin.mjs", ["--staged", full])
  assert.notEqual(result.status, 0)
  assert.match(result.stderr, new RegExp(`no dist/${lacking.target}/adage-train`))

  // The archive route (what the workflow runs): this machine's three archives, each with dist's
  // .sha256 beside it.
  const artifacts = path.join(root, "artifacts")
  mkdirSync(artifacts)
  const sums = new Map()
  for (const [app, bin] of APPS) {
    const dir = path.join(root, app)
    fake(path.join(dir, bin))
    const archive = path.join(artifacts, `${app}-${host.target}.tar.gz`)
    execFileSync("tar", ["-czf", archive, "-C", dir, bin])
    const digest = createHash("sha256").update(readFileSync(archive)).digest("hex")
    sums.set(`${archive}.sha256`, `${digest}  ${path.basename(archive)}\n`)
  }
  const stage = () => run("scripts/stage-plugin.mjs", ["--artifacts", artifacts, "--out", path.join(root, "out"), "--allow-missing"])
  for (const [file, value] of sums) writeFileSync(file, value)
  result = stage()
  assert.equal(result.status, 0, result.stdout + result.stderr)
  // One plugin, three binaries (plan A8, onboarding plan O1).
  const staged = path.join(root, "out", "plugin")
  const sumsFile = readFileSync(path.join(staged, "claude-code", "dist", "SHA256SUMS"), "utf8")
  for (const [, bin] of APPS) assert.match(sumsFile, new RegExp(`  ${host.target}/${bin}\n`))
  assert.match(result.stdout, /smoke: .*adage version -> adage /, "the host's adage was run")
  // The mod is a tree of modules (docs/claude-mod/impl/mod-structure.md): register.ts imports
  // view/ and draw/, so every module file must be staged where it was.
  const modules = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((d) => (d.isDirectory() ? modules(path.join(dir, d.name)) : [path.join(dir, d.name)]))
  const hooks = path.join(repo, "plugin", "claude-code", "hooks")
  const shipped = modules(hooks).filter((f) => /\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f) && !f.includes(`${path.sep}fixtures${path.sep}`))
  assert.ok(shipped.some((f) => f.endsWith(`${path.sep}view${path.sep}band.ts`)), "the mod's modules were found")
  for (const f of shipped) assert.ok(existsSync(path.join(staged, "claude-code", "hooks", path.relative(hooks, f))), `the staged mod lacks hooks/${path.relative(hooks, f)}`)
  for (const [file, value] of sums) {
    rmSync(file)
    result = stage()
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /no \.sha256; refusing an unverified release archive/)
    writeFileSync(file, "0".repeat(64) + "\n")
    result = stage()
    assert.notEqual(result.status, 0)
    assert.match(result.stderr, /does not match/)
    writeFileSync(file, value)
  }
  console.log("plugin staging: every target staged with adage, adage-exec and adage-train passes check-plugin --staged; a missing adage-train is refused naming its target (T-O1-01); the archive route accepts a verified set and rejects missing and incorrect checksums for all three (T-O2-04); every mod module staged")
} finally {
  rmSync(root, { recursive: true, force: true })
}
