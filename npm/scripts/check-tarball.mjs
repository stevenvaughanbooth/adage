#!/usr/bin/env node
// Validate npm pack --dry-run --json output before anything is published.
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { COMPANIONS, PLATFORMS } from "../config.mjs"

export function checkTarball(manifest, report) {
  assert.equal(report.length, 1, "expected one package in npm pack report")
  const packed = report[0]
  assert.equal(packed.name, manifest.name, "packed package name")
  assert.equal(packed.version, manifest.version, "packed package version")
  assert.equal(manifest.publishConfig?.access, "public", "scoped package must be public")
  assert.ok(!manifest.scripts, "packages must not execute lifecycle scripts")
  const required = new Set(["package.json", "README.md", "LICENSE", "NOTICE"])
  const allowed = new Set(required)
  if (manifest.bin) {
    assert.deepEqual(manifest.bin, { adage: "bin/adage.js" })
    required.add("bin/adage.js")
    allowed.add("bin/adage.js")
  } else {
    const platform = PLATFORMS.find(p => manifest.os?.[0] === p.os && manifest.cpu?.[0] === p.cpu)
    assert.ok(platform, "unknown platform package")
    const ext = platform.os === "win32" ? ".exe" : ""
    required.add(`bin/${platform.exe}`)
    allowed.add(`bin/${platform.exe}`)
    for (const companion of COMPANIONS) {
      const file = `bin/${companion.name}${ext}`
      allowed.add(file)
      if (companion.required(platform)) required.add(file)
    }
  }
  const actual = new Set()
  for (const file of packed.files) {
    assert.ok(allowed.has(file.path), `unexpected file in package: ${file.path}`)
    assert.ok(!actual.has(file.path), `duplicate file in package: ${file.path}`)
    assert.ok(file.size > 0, `empty package file: ${file.path}`)
    if (file.path.startsWith("bin/")) assert.ok(file.mode & 0o111, `binary is not executable: ${file.path}`)
    actual.add(file.path)
  }
  for (const file of required) assert.ok(actual.has(file), `missing package file: ${file}`)
  return `${manifest.name}@${manifest.version}: ${actual.size} expected files`
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  assert.equal(process.argv.length, 4, "usage: check-tarball.mjs <package.json> <npm-pack-report.json>")
  console.log(checkTarball(
    JSON.parse(readFileSync(process.argv[2], "utf8")),
    JSON.parse(readFileSync(process.argv[3], "utf8")),
  ))
}
