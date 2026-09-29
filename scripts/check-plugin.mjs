#!/usr/bin/env node
// Structural checks on the Claude Code plugins and marketplace that `claude plugin validate` does
// not make: versions agree with the workspace, and every hook command points at something that
// exists. Two plugins: `adage`, and `adage-exec` (Track C §3.2), whose hooks replace the Adage
// plugin's while execution is on; both session-start shims read the same marker, so exactly one
// serves and hooks never fire twice.
//
// The marketplace installs each plugin from the `marketplace` branch (a `git-subdir` source), which
// the release workflow writes with verified binaries for every target bundled
// (scripts/stage-plugin.mjs). `--staged <dir>` checks such a staged tree: every target present,
// every binary matching dist/SHA256SUMS.
//
//   node scripts/check-plugin.mjs [--staged <dir>]

import { createHash } from "node:crypto"
import { accessSync, constants, existsSync, readFileSync } from "node:fs"
import { PLATFORMS } from "../npm/config.mjs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const read = (p) => readFileSync(path.join(repo, p), "utf8")
const json = (p) => JSON.parse(read(p))
const problems = []
const expect = (ok, msg) => ok || problems.push(msg)
const executable = (p) => {
  try {
    accessSync(p, constants.X_OK)
    return true
  } catch {
    return false
  }
}

const version = read("Cargo.toml").match(/^\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/ms)?.[1]
expect(version, "Cargo.toml: no [workspace.package] version")

const market = json(".claude-plugin/marketplace.json")
expect(market.metadata?.version === version, `marketplace metadata.version ${market.metadata?.version} != ${version}`)
const names = []

for (const [pluginDir, bin] of [
  ["plugin/claude-code", "adage"],
  ["plugin/claude-code-exec", "adage-exec"],
]) {
  const plugin = json(`${pluginDir}/.claude-plugin/plugin.json`)
  names.push(plugin.name)
  expect(plugin.version === version, `${pluginDir}: plugin.json version ${plugin.version} != workspace ${version}`)
  const entry = (market.plugins || []).find((p) => p.name === plugin.name)
  expect(entry, `marketplace.json lists no plugin named ${plugin.name}`)
  if (entry) {
    const s = entry.source || {}
    expect(
      s.source === "git-subdir" && s.url === "stevenvaughanbooth/adage" && s.path === pluginDir && s.ref === "marketplace",
      `marketplace source for ${plugin.name} must be {source: git-subdir, url: stevenvaughanbooth/adage, path: ${pluginDir}, ref: marketplace} (the branch with bundled binaries)`,
    )
    expect(!/on PATH/i.test(entry.description || ""), `marketplace description for ${plugin.name} still says it needs a binary on PATH`)
    expect(entry.version === version, `marketplace plugin ${plugin.name} version ${entry.version} != workspace ${version}`)
  }
  // A failure must reach the person: the shim prints a JSON systemMessage on stdout, and points
  // the other hooks at scripts/quiet.
  const shim = read(`${pluginDir}/scripts/session-start`)
  expect(shim.includes('{"systemMessage":'), `${pluginDir}: session-start does not print a systemMessage on failure`)
  expect(shim.includes("scripts/quiet"), `${pluginDir}: session-start does not quiet the other hooks on failure`)
  for (const s of ["scripts/session-start", "scripts/quiet"]) {
    expect(executable(path.join(repo, pluginDir, s)), `${pluginDir}: ${s} is missing or not executable`)
  }
  expect(read(`${pluginDir}/scripts/session-start`).includes("exec/enabled"), `${pluginDir}: session-start does not read the exec marker`)

  // Hook commands: ${CLAUDE_PLUGIN_ROOT} paths must exist and be executable; ${CLAUDE_PLUGIN_DATA}
  // paths are created at SessionStart, which must therefore come first.
  const hooks = json(`${pluginDir}/hooks/hooks.json`).hooks
  for (const [event, groups] of Object.entries(hooks)) {
    for (const g of groups) {
      for (const h of g.hooks) {
        const cmd = h.command || ""
        if (cmd.startsWith("${CLAUDE_PLUGIN_ROOT}/")) {
          const p = path.join(repo, pluginDir, cmd.slice("${CLAUDE_PLUGIN_ROOT}/".length))
          expect(executable(p), `${pluginDir} ${event}: ${cmd} is missing or not executable`)
        } else if (cmd.startsWith("${CLAUDE_PLUGIN_DATA}/")) {
          expect(hooks.SessionStart, `${pluginDir} ${event}: ${cmd} needs a SessionStart hook to install it`)
          expect(cmd === "${CLAUDE_PLUGIN_DATA}/bin/" + bin, `${pluginDir} ${event}: runs ${cmd}, not bin/${bin}`)
        } else {
          problems.push(`${pluginDir} ${event}: unexpected command ${cmd}`)
        }
        // An E2 action runs at the edit and Stop events of the adage-exec plugin (at most 20 s of
        // its own): those get 30 s; everything else stays at most 10 s.
        const action = bin === "adage-exec" && (event === "PostToolBatch" || event === "Stop")
        const limit = action ? 30 : 10
        expect(typeof h.timeout === "number" && h.timeout <= limit, `${pluginDir} ${event}: timeout must be set and at most ${limit} s`)
        if (action) expect(h.timeout === 30, `${pluginDir} ${event}: an action event's timeout is 30 s`)
      }
    }
  }
}

// Neither plugin runs a server: Adage acts only from the hooks it owns (Track C, revised).
for (const dir of ["plugin/claude-code", "plugin/claude-code-exec"]) {
  let server = false
  try {
    readFileSync(path.join(repo, dir, ".mcp.json"))
    server = true
  } catch {}
  expect(!server, `${dir}: Adage registers no MCP server`)
  expect(!("mcpServers" in json(`${dir}/.claude-plugin/plugin.json`)), `${dir}: Adage registers no MCP server`)
}

// A staged marketplace tree (the release workflow's output).
const stagedAt = process.argv.indexOf("--staged")
if (stagedAt > 0) {
  const root = path.resolve(process.argv[stagedAt + 1] || "")
  for (const [pluginDir, bins] of [
    ["plugin/claude-code", ["adage"]],
    ["plugin/claude-code-exec", ["adage-exec", "adage"]],
  ]) {
    const dist = path.join(root, pluginDir, "dist")
    const sums = existsSync(path.join(dist, "SHA256SUMS")) ? readFileSync(path.join(dist, "SHA256SUMS"), "utf8") : ""
    for (const p of PLATFORMS) {
      for (const b of bins) {
        if (p.os === "win32" && pluginDir.endsWith("-exec")) continue
        const rel = `${p.target}/${p.os === "win32" ? `${b}.exe` : b}`
        const file = path.join(dist, rel)
        if (!existsSync(file)) {
          problems.push(`staged ${pluginDir}: no dist/${rel}`)
          continue
        }
        const got = createHash("sha256").update(readFileSync(file)).digest("hex")
        expect(sums.includes(`${got}  ${rel}\n`), `staged ${pluginDir}: dist/${rel} is not in SHA256SUMS with its checksum`)
      }
    }
  }
}

if (problems.length) {
  for (const p of problems) console.error(`check-plugin: ${p}`)
  process.exit(1)
}
console.log(`plugins ${names.join(", ")} ${version}: ok`)
