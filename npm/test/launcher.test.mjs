// Stages the npm packages around a locally built adage binary, lays them out the way npm installs
// them (main package plus this platform's package in node_modules), and runs the launcher.
//
//   node npm/test/launcher.test.mjs [path/to/adage]     (default: target/debug/adage[.exe])
//
// Uses only the local binary: installs, downloads and publishes nothing.

import assert from "node:assert/strict"
import { spawnSync } from "node:child_process"
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { COMPANIONS, PACKAGE_NAME, PLATFORMS } from "../config.mjs"

const here = path.dirname(fileURLToPath(import.meta.url))
const repo = path.resolve(here, "../..")
const exe = process.platform === "win32" ? "adage.exe" : "adage"
const bin = path.resolve(process.argv[2] || path.join(repo, "target/debug", exe))
assert.ok(existsSync(bin), `no binary at ${bin}; build it first (cargo build -p adage-cli)`)
const host = PLATFORMS.find((p) => p.os === process.platform && p.cpu === process.arch)
assert.ok(host, `no release platform for ${process.platform}-${process.arch}`)

const work = mkdtempSync(path.join(tmpdir(), "adage-npm-test-"))
const run = (cmd, argv, opts = {}) => spawnSync(cmd, argv, { encoding: "utf8", ...opts })

try {
  const cargoVersion = readFileSync(path.join(repo, "Cargo.toml"), "utf8").match(/^version\s*=\s*"([^"]+)"/m)[1]
  mkdirSync(path.join(work, "bins", host.target), { recursive: true })
  cpSync(bin, path.join(work, "bins", host.target, host.exe))
  // The companion binaries built beside it (adage-exec, adage-train), when they are.
  const ext = process.platform === "win32" ? ".exe" : ""
  const companions = COMPANIONS.map((c) => c.name + ext).filter((e) => existsSync(path.join(path.dirname(bin), e)))
  for (const e of companions) cpSync(path.join(path.dirname(bin), e), path.join(work, "bins", host.target, e))

  for (const name of [PACKAGE_NAME, "adage-cli-launcher-test"]) {
    const out = path.join(work, "out")
    const s = run(process.execPath, [
      path.join(repo, "npm/scripts/stage.mjs"),
      "--version", cargoVersion,
      "--bins", path.join(work, "bins"),
      "--out", out,
      "--name", name,
      "--allow-missing",
    ])
    assert.equal(s.status, 0, s.stderr)

    const main = JSON.parse(readFileSync(path.join(out, "cli/package.json"), "utf8"))
    const plat = JSON.parse(readFileSync(path.join(out, `${host.os}-${host.cpu}/package.json`), "utf8"))
    assert.equal(main.name, name)
    assert.equal(main.version, cargoVersion)
    assert.equal(plat.name, `${name}-${host.os}-${host.cpu}`)
    assert.deepEqual(main.optionalDependencies, { [plat.name]: cargoVersion })
    assert.deepEqual([plat.os, plat.cpu], [[host.os], [host.cpu]])
    assert.equal(main.license, plat.license)
    assert.deepEqual(
      readFileSync(path.join(out, "publish-order.txt"), "utf8").trim().split("\n"),
      [`${host.os}-${host.cpu}`, "cli"],
    )

    // Companions sit beside adage in the platform package.
    for (const e of companions) assert.ok(existsSync(path.join(out, `${host.os}-${host.cpu}`, "bin", e)), `${e} staged`)

    // Lay out node_modules as npm would, then run the launcher.
    const nm = path.join(work, "node_modules")
    rmSync(nm, { recursive: true, force: true })
    cpSync(path.join(out, "cli"), path.join(nm, name), { recursive: true })
    cpSync(path.join(out, `${host.os}-${host.cpu}`), path.join(nm, plat.name), { recursive: true })
    const launcher = path.join(nm, name, "bin/adage.js")
    const env = { ...process.env, ADAGE_HOME: path.join(work, "home") }
    delete env.ADAGE_DISABLE

    const v = run(process.execPath, [launcher, "version"], { env })
    assert.equal(v.status, 0, v.stderr)
    assert.equal(v.stdout.trim(), `adage ${cargoVersion}`)

    // Arguments pass through untouched, and so does a failing exit status.
    const bad = run(process.execPath, [launcher, "install", "no-such-harness", "--print"], { env })
    assert.equal(bad.status, 1)
    assert.match(bad.stderr, /usage: adage install/)

    // stdin reaches the binary: a hook call reads its payload and always exits 0 (fail open).
    const hook = run(process.execPath, [launcher, "hook", "claude", "SessionStart"], {
      env,
      input: JSON.stringify({ session_id: "npm-test", cwd: work, hook_event_name: "SessionStart" }),
    })
    assert.equal(hook.status, 0, hook.stderr)

    // Asked by the Claude Code plugin, it names the binary instead of running it.
    const which = run(process.execPath, [launcher, "version"], { env: { ...env, ADAGE_NPM_WHICH: "1" } })
    assert.equal(which.status, 0, which.stderr)
    assert.equal(realpathSync(which.stdout.trim()), realpathSync(path.join(nm, plat.name, "bin", host.exe)))

    // Without the platform package the launcher says so and fails; it never downloads.
    rmSync(path.join(nm, plat.name), { recursive: true })
    const none = run(process.execPath, [launcher, "version"], { env })
    assert.equal(none.status, 1)
    assert.match(none.stderr, /no prebuilt binary for/)
  }
  console.log(`npm launcher ok (companions staged: ${companions.join(", ") || "none built"})`)
} finally {
  rmSync(work, { recursive: true, force: true })
}
