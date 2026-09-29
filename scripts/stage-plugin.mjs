#!/usr/bin/env node
// Stage the Claude Code plugins with their binaries bundled, for the `marketplace` branch that
// `/plugin install adage@adage` installs from (docs/zero-config.md P0-1). The marketplace plugin
// then needs no `adage` on PATH and never downloads anything: its session-start shim picks
// dist/<target>/adage for the machine and verifies it against dist/SHA256SUMS.
//
//   node scripts/stage-plugin.mjs --out <dir> (--bins <dir> | --artifacts <dir>) [--allow-missing]
//
//   --bins <dir>       binaries laid out as <dir>/<target>/adage[.exe] (and adage-exec)
//   --artifacts <dir>  dist's release archives (adage-cli-<target>.tar.xz / .zip, adage-exec-…)
//   --allow-missing    stage only the targets that have binaries (local tests); a release needs all
//
// Writes <out>/plugin/claude-code/ and <out>/plugin/claude-code-exec/ (each the plugin as in this
// repository, plus dist/<target>/… and dist/SHA256SUMS), and <out>/README.md. Every binary staged
// is checksummed; the one for this machine, if staged, is run (`adage version`) as a smoke test.

import { execFileSync } from "node:child_process"
import { createHash } from "node:crypto"
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { PLATFORMS } from "../npm/config.mjs"

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")

function fail(msg) {
  console.error(`stage-plugin: ${msg}`)
  process.exit(1)
}

function args(argv) {
  const out = { "allow-missing": false }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith("--")) fail(`unexpected argument ${a}`)
    const k = a.slice(2)
    if (k === "allow-missing") out[k] = true
    else out[k] = argv[++i]
  }
  return out
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

function fromArchive(artifacts, app, target, exe, scratch) {
  const base = `${app}-${target}`
  const archive = [".tar.xz", ".tar.gz", ".zip"].map((ext) => path.join(artifacts, base + ext)).find(existsSync)
  if (!archive) return null
  // dist writes <archive>.sha256 beside each archive: a mismatch stops the release.
  if (existsSync(`${archive}.sha256`)) {
    const want = readFileSync(`${archive}.sha256`, "utf8").trim().split(/\s+/)[0]
    if (want !== sha256(archive)) fail(`${archive} does not match ${path.basename(archive)}.sha256`)
  }
  const into = path.join(scratch, app, target)
  mkdirSync(into, { recursive: true })
  try {
    execFileSync(archive.endsWith(".zip") ? "unzip" : "tar", archive.endsWith(".zip") ? ["-q", "-o", archive, "-d", into] : ["-xf", archive, "-C", into], { stdio: "inherit" })
  } catch {
    execFileSync("tar", ["-xf", archive, "-C", into], { stdio: "inherit" }) // bsdtar reads zip
  }
  return find(into, exe)
}

const sha256 = (p) => createHash("sha256").update(readFileSync(p)).digest("hex")

const o = args(process.argv.slice(2))
if (!o.out) fail("--out <dir> is required")
if (!o.bins === !o.artifacts) fail("exactly one of --bins <dir> or --artifacts <dir> is required")
const out = path.resolve(o.out)
rmSync(out, { recursive: true, force: true })
mkdirSync(out, { recursive: true })
const scratch = o.artifacts ? mkdtempSync(path.join(tmpdir(), "adage-plugin-")) : null

// Each plugin and the binaries it bundles. adage-exec's plugin carries adage too: adage-exec finds
// the learner beside itself, and serves with adage when execution is unavailable.
const PLUGINS = [
  { dir: "plugin/claude-code", bins: [{ name: "adage", app: "adage-cli", required: () => true }] },
  {
    dir: "plugin/claude-code-exec",
    bins: [
      { name: "adage-exec", app: "adage-exec", required: (p) => p.os !== "win32" },
      { name: "adage", app: "adage-cli", required: (p) => p.os !== "win32" },
    ],
  },
]

const host = PLATFORMS.find((p) => p.os === process.platform && p.cpu === process.arch)
try {
  for (const plugin of PLUGINS) {
    const dest = path.join(out, plugin.dir)
    cpSync(path.join(repo, plugin.dir), dest, { recursive: true, filter: (s) => !s.split(path.sep).includes("dist") })
    // The binaries embed third-party data (the MIT embedding model): its notices travel with them.
    for (const f of ["LICENSE", "NOTICE"]) copyFileSync(path.join(repo, f), path.join(dest, f))
    const sums = []
    for (const p of PLATFORMS) {
      for (const b of plugin.bins) {
        const exe = p.os === "win32" ? `${b.name}.exe` : b.name
        const bin = o.bins ? path.join(o.bins, p.target, exe) : fromArchive(o.artifacts, b.app, p.target, exe, scratch)
        if (!bin || !existsSync(bin) || !statSync(bin).isFile()) {
          if (o["allow-missing"] || !b.required(p)) continue
          fail(`${plugin.dir}: no ${exe} for ${p.target}`)
        }
        const to = path.join(dest, "dist", p.target, exe)
        mkdirSync(path.dirname(to), { recursive: true })
        copyFileSync(bin, to)
        chmodSync(to, 0o755)
        sums.push(`${sha256(to)}  ${p.target}/${exe}`)
      }
    }
    if (sums.length === 0) fail(`${plugin.dir}: no binaries staged`)
    writeFileSync(path.join(dest, "dist", "SHA256SUMS"), sums.join("\n") + "\n")
    // Verify what was written, as the shim will.
    for (const line of sums) {
      const [want, rel] = line.split(/\s+/)
      if (sha256(path.join(dest, "dist", rel)) !== want) fail(`${plugin.dir}: ${rel} does not match its checksum`)
    }
    console.log(`staged ${plugin.dir} with ${sums.length} binaries`)
    if (host) {
      const smoke = path.join(dest, "dist", host.target, "adage")
      if (existsSync(smoke)) {
        const v = execFileSync(smoke, ["version"], { encoding: "utf8" }).trim()
        if (!v.startsWith("adage ")) fail(`${smoke} version printed ${v}`)
        console.log(`  smoke: ${host.target}/adage version -> ${v}`)
      }
    }
  }
  writeFileSync(
    path.join(out, "README.md"),
    "# Adage Claude Code plugins, with binaries\n\nThis branch is written by the release workflow: the plugins from the tagged commit, each with\nverified binaries for every release target under `dist/`. Install with\n`/plugin marketplace add stevenvaughanbooth/adage` then `/plugin install adage@adage`.\nSource: https://github.com/stevenvaughanbooth/adage\n",
  )
} finally {
  if (scratch) rmSync(scratch, { recursive: true, force: true })
}
