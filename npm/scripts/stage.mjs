#!/usr/bin/env node
// Stage the npm packages for a release: stamp the name, version and license into the main package
// and one package per platform, and copy each platform's binaries in: `adage`, and beside it
// `adage-exec` (what `adage delegate` runs) and `adage-train` (what `adage train` runs), each from
// its own release archive.
//
//   node npm/scripts/stage.mjs --version 0.1.0 --out <dir> (--bins <dir> | --artifacts <dir>)
//                              [--name <npm name>] [--allow-missing]
//
//   --bins <dir>       binaries laid out as <dir>/<target>/adage[.exe] (and adage-exec, adage-train)
//   --artifacts <dir>  dist's release archives (adage-cli-<target>.tar.xz / .zip, and adage-exec-…,
//                      adage-train-…), extracted here
//   --name             override PACKAGE_NAME from npm/config.mjs (tests only)
//   --allow-missing    stage only the platforms that have a binary (local tests); a release needs all
//
// Writes <out>/<os>-<cpu>/ per platform, <out>/cli/, and <out>/publish-order.txt (platform
// packages first, so the main package never points at a version that is not yet published).
// Publishes nothing.

import { execFileSync } from "node:child_process"
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { COMPANIONS, PACKAGE_NAME, PLATFORMS } from "../config.mjs"

const here = path.dirname(fileURLToPath(import.meta.url))
const npmDir = path.resolve(here, "..")
const repoDir = path.resolve(npmDir, "..")
// Copied into every package: npm adds LICENSE on its own, never NOTICE (third-party notices).
const LEGAL = ["LICENSE", "NOTICE"]

function args(argv) {
  const out = { "allow-missing": false }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith("--")) throw new Error(`unexpected argument ${a}`)
    const k = a.slice(2)
    if (k === "allow-missing") out[k] = true
    else out[k] = argv[++i]
  }
  return out
}

function fail(msg) {
  console.error(`stage: ${msg}`)
  process.exit(1)
}

// The workspace license (Cargo.toml [workspace.package], Apache-2.0) is the one source of truth.
function workspaceLicense() {
  const toml = readFileSync(path.join(repoDir, "Cargo.toml"), "utf8")
  const m = toml.match(/^\[workspace\.package\][^[]*?^license\s*=\s*"([^"]+)"/ms)
  if (!m) fail("no license in [workspace.package] of Cargo.toml")
  return m[1]
}

function fill(template, vars) {
  let s = readFileSync(template, "utf8")
  for (const [k, v] of Object.entries(vars)) s = s.replaceAll(`__${k}__`, v)
  const left = s.match(/__[A-Z_]+__/)
  if (left) fail(`${template}: unfilled placeholder ${left[0]}`)
  return JSON.parse(s)
}

function find(dir, name) {
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) {
      const hit = find(p, name)
      if (hit) return hit
    } else if (e.name === name) return p
  }
  return null
}

// Extract dist's archive of `app` for one target into a scratch directory and return the path of
// the binary `exe` in it.
function fromArchive(artifacts, app, p, exe, scratch) {
  const base = `${app}-${p.target}`
  const archive = [".tar.xz", ".tar.gz", ".zip"].map((ext) => path.join(artifacts, base + ext)).find(existsSync)
  if (!archive) return null
  const into = path.join(scratch, app, p.target)
  mkdirSync(into, { recursive: true })
  if (archive.endsWith(".zip")) {
    try {
      execFileSync("unzip", ["-q", "-o", archive, "-d", into], { stdio: "inherit" })
    } catch {
      execFileSync("tar", ["-xf", archive, "-C", into], { stdio: "inherit" }) // bsdtar reads zip
    }
  } else {
    execFileSync("tar", ["-xf", archive, "-C", into], { stdio: "inherit" })
  }
  return find(into, exe)
}

// The companion binary `c` for platform `p`, or null.
function companion(o, c, p, scratch) {
  const exe = p.os === "win32" ? `${c.name}.exe` : c.name
  const bin = o.bins ? path.join(o.bins, p.target, exe) : fromArchive(o.artifacts, c.app, p, exe, scratch)
  return bin && existsSync(bin) && statSync(bin).isFile() ? { bin, exe } : null
}

const o = args(process.argv.slice(2))
const version = o.version
if (!version || !/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) fail("--version <semver> is required")
if (!o.out) fail("--out <dir> is required")
if (!o.bins === !o.artifacts) fail("exactly one of --bins <dir> or --artifacts <dir> is required")
const name = o.name || PACKAGE_NAME
const license = workspaceLicense()
const out = path.resolve(o.out)
rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })
const scratch = o.artifacts ? mkdtempSync(path.join(tmpdir(), "adage-stage-")) : null

const staged = []
try {
  for (const p of PLATFORMS) {
    const bin = o.bins ? path.join(o.bins, p.target, p.exe) : fromArchive(o.artifacts, "adage-cli", p, p.exe, scratch)
    if (!bin || !existsSync(bin) || !statSync(bin).isFile()) {
      if (o["allow-missing"]) continue
      fail(`no binary for ${p.target}`)
    }
    const dir = path.join(out, `${p.os}-${p.cpu}`)
    mkdirSync(path.join(dir, "bin"), { recursive: true })
    const manifest = fill(path.join(npmDir, "platform/package.json"), {
      PACKAGE_NAME: name,
      OS: p.os,
      CPU: p.cpu,
      TARGET: p.target,
      LICENSE: license,
    })
    manifest.version = version
    writeFileSync(path.join(dir, "package.json"), JSON.stringify(manifest, null, 2) + "\n")
    writeFileSync(
      path.join(dir, "README.md"),
      `# ${manifest.name}\n\nThe \`adage\` binary for ${p.os}-${p.cpu} (\`${p.target}\`). Install [\`${name}\`](https://www.npmjs.com/package/${name}) instead; it depends on this package on ${p.os}-${p.cpu} only.\n`,
    )
    // The binary embeds third-party data (the MIT embedding model): its notices travel with it.
    for (const f of LEGAL) copyFileSync(path.join(repoDir, f), path.join(dir, f))
    const dest = path.join(dir, "bin", p.exe)
    copyFileSync(bin, dest)
    chmodSync(dest, 0o755)
    // Companions sit beside `adage`, where `adage delegate on` and `adage train` look first.
    for (const c of COMPANIONS) {
      const found = companion(o, c, p, scratch)
      if (!found) {
        if (o["allow-missing"] || !c.required(p)) continue
        fail(`no ${c.name} binary for ${p.target}`)
      }
      const to = path.join(dir, "bin", found.exe)
      copyFileSync(found.bin, to)
      chmodSync(to, 0o755)
    }
    staged.push({ dir, name: manifest.name })
  }
  if (staged.length === 0) fail("no platform binaries found")

  const main = fill(path.join(npmDir, "cli/package.json"), { PACKAGE_NAME: name, LICENSE: license })
  main.version = version
  main.optionalDependencies = Object.fromEntries(staged.map((s) => [s.name, version]))
  const mainDir = path.join(out, "cli")
  mkdirSync(path.join(mainDir, "bin"), { recursive: true })
  writeFileSync(path.join(mainDir, "package.json"), JSON.stringify(main, null, 2) + "\n")
  copyFileSync(path.join(npmDir, "cli/bin/adage.js"), path.join(mainDir, "bin/adage.js"))
  chmodSync(path.join(mainDir, "bin/adage.js"), 0o755)
  copyFileSync(path.join(npmDir, "cli/README.md"), path.join(mainDir, "README.md"))
  for (const f of LEGAL) copyFileSync(path.join(repoDir, f), path.join(mainDir, f))

  const order = [...staged.map((s) => s.dir), mainDir]
  writeFileSync(path.join(out, "publish-order.txt"), order.map((d) => path.relative(out, d)).join("\n") + "\n")
  console.log(`staged ${name}@${version} with ${staged.length} platform package(s) in ${out}`)
  for (const d of order) console.log(`  ${path.relative(out, d)}`)
} finally {
  if (scratch) rmSync(scratch, { recursive: true, force: true })
}
