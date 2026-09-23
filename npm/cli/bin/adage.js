#!/usr/bin/env node
// Launcher for the adage binary. npm installs exactly one platform package from this package's
// optionalDependencies (their `os`/`cpu` fields rule out the rest); this finds it and runs its
// binary with the same arguments, stdio, and exit status. Nothing is downloaded, at install time
// or ever. Hooks should call the binary directly (`adage install` records its real path), so this
// launcher is not on the per-event hook path.
"use strict"

const { spawnSync } = require("node:child_process")
const path = require("node:path")
const pkg = require("../package.json")

function findBinary() {
  for (const name of Object.keys(pkg.optionalDependencies || {})) {
    let manifest
    try {
      manifest = require.resolve(`${name}/package.json`)
    } catch {
      continue // not installed: another platform's package
    }
    const meta = require(manifest)
    if ((meta.os || []).includes(process.platform) && (meta.cpu || []).includes(process.arch)) {
      return path.join(path.dirname(manifest), "bin", process.platform === "win32" ? "adage.exe" : "adage")
    }
  }
  return null
}

const bin = findBinary()
if (!bin) {
  const names = Object.keys(pkg.optionalDependencies || {}).join(", ")
  console.error(
    `adage: no prebuilt binary for ${process.platform}-${process.arch} is installed (${pkg.name} ships: ${names}).\n` +
      "If you installed with --omit=optional or --no-optional, reinstall without it. Otherwise build from source: " +
      "cargo install --locked --git https://github.com/stevenvaughanbooth/adage adage-cli",
  )
  process.exit(1)
}

// The Claude Code plugin asks which binary this launcher runs, so its hooks can call that binary
// directly instead of starting node on every event.
if (process.env.ADAGE_NPM_WHICH === "1") {
  console.log(bin)
  process.exit(0)
}

const r = spawnSync(bin, process.argv.slice(2), { stdio: "inherit", windowsHide: true })
if (r.error) {
  console.error(`adage: could not run ${bin}: ${r.error.message}`)
  process.exit(1)
}
if (r.signal) {
  // Die the way the binary did, so a shell sees the same signal (128 + n if the signal is ignored).
  process.exitCode = 128 + (require("node:os").constants.signals[r.signal] || 0)
  process.kill(process.pid, r.signal)
} else {
  process.exit(r.status === null ? 1 : r.status)
}
