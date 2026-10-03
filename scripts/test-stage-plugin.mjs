// The release publisher must reject missing or incorrect archive checksums before extraction.
// Test archives contain tiny fake binaries; this proves packaging integrity, not native execution.
import assert from "node:assert/strict"
import { createHash } from "node:crypto"
import { execFileSync, spawnSync } from "node:child_process"
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { PLATFORMS } from "../npm/config.mjs"

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
  console.log("plugin archive integrity: verified pair accepted into one plugin; missing and incorrect checksums rejected for both binaries")
} finally {
  rmSync(root, { recursive: true, force: true })
}
