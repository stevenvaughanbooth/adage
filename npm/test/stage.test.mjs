// Exercise release staging across the complete platform matrix using small stand-in binaries.
// No compilation, network access or publication. launcher.test.mjs exercises real host binaries.
import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { createHash } from "node:crypto"
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { COMPANIONS, PACKAGE_NAME, PLATFORMS } from "../config.mjs"
import { checkTarball } from "../scripts/check-tarball.mjs"

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..")
const work = mkdtempSync(path.join(tmpdir(), "adage-npm-stage-test-"))
const bins = path.join(work, "bins")
const out = path.join(work, "out")
const stage = (...args) => spawnSync(process.execPath, [path.join(repo, "npm/scripts/stage.mjs"),
  "--version", "0.1.0-test", "--out", out, ...args], { encoding: "utf8" })
const manifest = dir => JSON.parse(readFileSync(path.join(out, dir, "package.json"), "utf8"))

try {
  for (const p of PLATFORMS) {
    const dir = path.join(bins, p.target)
    mkdirSync(dir, { recursive: true })
    const ext = p.os === "win32" ? ".exe" : ""
    for (const name of [p.exe, ...COMPANIONS.filter(c => c.required(p)).map(c => c.name + ext)]) {
      writeFileSync(path.join(dir, name), `stand-in ${p.target} ${name}\n`)
    }
  }
  let result = stage("--bins", bins)
  assert.equal(result.status, 0, result.stderr)
  const main = manifest("cli")
  assert.deepEqual(Object.keys(main.optionalDependencies), PLATFORMS.map(p => `${PACKAGE_NAME}-${p.os}-${p.cpu}`))
  assert.ok(Object.values(main.optionalDependencies).every(v => v === "0.1.0-test"))
  assert.equal(main.publishConfig.access, "public")
  assert.equal(readFileSync(path.join(out, "publish-order.txt"), "utf8").trim().split("\n").at(-1), "cli")
  for (const p of PLATFORMS) {
    const dir = `${p.os}-${p.cpu}`
    const meta = manifest(dir)
    assert.equal(meta.name, `${PACKAGE_NAME}-${dir}`)
    assert.deepEqual([meta.os, meta.cpu], [[p.os], [p.cpu]])
    const ext = p.os === "win32" ? ".exe" : ""
    for (const c of COMPANIONS) assert.equal(existsSync(path.join(out, dir, "bin", c.name + ext)), c.required(p))
  }
  // Publication must fail closed when a required companion is missing.
  rmSync(path.join(bins, PLATFORMS[0].target, "adage-train"))
  result = stage("--bins", bins)
  assert.equal(result.status, 1)
  assert.match(result.stderr, /no adage-train binary/)

  // Release archives must be verified before extraction, even in partial local dry runs.
  const artifacts = path.join(work, "artifacts")
  const payload = path.join(work, "payload")
  mkdirSync(artifacts)
  mkdirSync(payload)
  writeFileSync(path.join(payload, "adage"), "stand-in archive binary\n")
  chmodSync(path.join(payload, "adage"), 0o755)
  const archive = path.join(artifacts, `adage-cli-${PLATFORMS[0].target}.tar.gz`)
  const tar = spawnSync("tar", ["-czf", archive, "-C", payload, "adage"], { encoding: "utf8" })
  assert.equal(tar.status, 0, tar.stderr)
  result = stage("--artifacts", artifacts, "--allow-missing")
  assert.equal(result.status, 1)
  assert.match(result.stderr, /no .sha256/)
  writeFileSync(`${archive}.sha256`, "0".repeat(64) + "\n")
  result = stage("--artifacts", artifacts, "--allow-missing")
  assert.equal(result.status, 1)
  assert.match(result.stderr, /does not match/)
  writeFileSync(`${archive}.sha256`, createHash("sha256").update(readFileSync(archive)).digest("hex") + "\n")
  result = stage("--artifacts", artifacts, "--allow-missing")
  assert.equal(result.status, 0, result.stderr)
  assert.equal(readFileSync(path.join(out, "darwin-arm64/bin/adage"), "utf8"), "stand-in archive binary\n")

  // Reject accidental source/history inclusions and omissions from a pack report.
  const report = [{ name: main.name, version: main.version, files: ["package.json", "README.md", "LICENSE", "NOTICE", "bin/adage.js"].map(p => ({ path: p, size: 10, mode: 0o755 })) }]
  assert.match(checkTarball(main, report), /5 expected files/)
  const extra = structuredClone(report)
  extra[0].files.push({ path: "traces/session.jsonl", size: 10, mode: 0o644 })
  assert.throws(() => checkTarball(main, extra), /unexpected file/)
  const missing = structuredClone(report)
  missing[0].files = missing[0].files.filter(f => f.path !== "NOTICE")
  assert.throws(() => checkTarball(main, missing), /missing package file: NOTICE/)
  console.log("npm staging ok (five platforms, required companions, checksums, tarball allowlist)")
} finally {
  rmSync(work, { recursive: true, force: true })
}
