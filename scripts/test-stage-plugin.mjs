// The release publisher must reject missing or incorrect archive checksums before extraction.
// Test archives contain tiny fake binaries; this proves packaging integrity, not native execution.
import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { execFileSync, spawnSync } from "node:child_process"
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { PLATFORMS } from "../npm/config.mjs"
import { validatedHooks, matchesValidatedHooks } from "./plugin-validation.mjs"

// Validator 2.1.288 uses a Unicode marker; older captured reports used ASCII.
const hooks = "session.start, classic.Stop, ui.close"
for (const marker of [">", "❯"]) {
  const complete = validatedHooks(`  ${marker} ./register.ts hooks: ${hooks}\n`)
  assert.ok(matchesValidatedHooks(hooks, complete))
  assert.ok(!matchesValidatedHooks(hooks + ", new.hook", complete), "an untruncated report must match completely")
  const truncated = validatedHooks(`  ${marker} ./register.ts hooks: session.start, classic.… [+14 chars]\n`)
  assert.ok(matchesValidatedHooks(hooks, truncated))
  assert.ok(!matchesValidatedHooks(hooks + ", new.hook", truncated), "truncation cannot hide an added hook")
  assert.ok(!matchesValidatedHooks("classic.Stop, session.start, ui.close", complete), "hook reordering must fail")
}
assert.equal(validatedHooks("no hook report"), null)
assert.equal(validatedHooks(`> ./register.ts hooks: ${hooks}\n❯ ./register.ts hooks: ${hooks}\n`), null)
assert.equal(validatedHooks(`x ./register.ts hooks: ${hooks}\n`), null)
assert.ok(!matchesValidatedHooks(hooks, validatedHooks("> ./register.ts hooks: session.start… [+0 chars]")))
console.log("validator hook reports: ASCII and Unicode markers accepted; missing, duplicate, reordered and length-changing records rejected")

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const host = PLATFORMS.find((p) => p.os === process.platform && p.cpu === process.arch)
assert(host && process.platform !== "win32", "run the archive staging test on supported Unix")
const root = mkdtempSync(path.join(tmpdir(), "adage-stage-test-"))
try {
  const artifacts = path.join(root, "artifacts")
  mkdirSync(artifacts)
  const sums = new Map()
  for (const [app, bin] of [["adage-cli", "adage"], ["adage-exec", "adage-exec"]]) {
    const dir = path.join(root, app)
    mkdirSync(dir)
    writeFileSync(path.join(dir, bin), "#!/bin/sh\necho 'adage 0.1.0'\n")
    chmodSync(path.join(dir, bin), 0o755)
    const archive = path.join(artifacts, `${app}-${host.target}.tar.gz`)
    execFileSync("tar", ["-czf", archive, "-C", dir, bin])
    const digest = createHash("sha256").update(readFileSync(archive)).digest("hex")
    sums.set(`${archive}.sha256`, `${digest}  ${path.basename(archive)}\n`)
  }
  const stage = () => spawnSync(process.execPath, ["scripts/stage-plugin.mjs", "--artifacts", artifacts,
    "--out", path.join(root, "out"), "--allow-missing"], { cwd: repo, encoding: "utf8" })
  for (const [file, value] of sums) writeFileSync(file, value)
  let result = stage()
  assert.equal(result.status, 0, result.stdout + result.stderr)
  // One plugin, both binaries (plan A8); the retired alias bundles nothing.
  const staged = path.join(root, "out", "plugin")
  const sumsFile = readFileSync(path.join(staged, "claude-code", "dist", "SHA256SUMS"), "utf8")
  for (const bin of ["adage", "adage-exec"]) assert.match(sumsFile, new RegExp(`  ${host.target}/${bin}\n`))
  assert.ok(!existsSync(path.join(staged, "claude-code-exec", "dist")), "the alias bundles no binaries")
  assert.ok(existsSync(path.join(staged, "claude-code-exec", "scripts", "session-start")), "the alias is staged")
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
  console.log("plugin archive integrity: verified pair accepted into one plugin, every mod module staged; missing and incorrect checksums rejected for both binaries")
} finally {
  rmSync(root, { recursive: true, force: true })
}
