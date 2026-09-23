#!/usr/bin/env node
// Structural checks on the Claude Code plugin and marketplace that `claude plugin validate` does not
// make: versions agree with the workspace, and every hook command points at something that exists.
//
//   node scripts/check-plugin.mjs

import { accessSync, constants, readFileSync } from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")
const read = (p) => readFileSync(path.join(repo, p), "utf8")
const json = (p) => JSON.parse(read(p))
const problems = []
const expect = (ok, msg) => ok || problems.push(msg)

const version = read("Cargo.toml").match(/^\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/ms)?.[1]
expect(version, "Cargo.toml: no [workspace.package] version")

const pluginDir = "plugin/claude-code"
const plugin = json(`${pluginDir}/.claude-plugin/plugin.json`)
expect(plugin.version === version, `plugin.json version ${plugin.version} != workspace ${version}`)

const market = json(".claude-plugin/marketplace.json")
const entry = (market.plugins || []).find((p) => p.name === plugin.name)
expect(entry, `marketplace.json lists no plugin named ${plugin.name}`)
if (entry) {
  expect(path.normalize(entry.source) === path.normalize(`./${pluginDir}`), `marketplace source ${entry.source} != ./${pluginDir}`)
  expect(entry.version === version, `marketplace plugin version ${entry.version} != workspace ${version}`)
}
expect(market.metadata?.version === version, `marketplace metadata.version ${market.metadata?.version} != ${version}`)

// Hook commands: ${CLAUDE_PLUGIN_ROOT} paths must exist and be executable; ${CLAUDE_PLUGIN_DATA}
// paths are created at SessionStart, which must therefore come first.
const hooks = json(`${pluginDir}/hooks/hooks.json`).hooks
for (const [event, groups] of Object.entries(hooks)) {
  for (const g of groups) {
    for (const h of g.hooks) {
      const cmd = h.command || ""
      if (cmd.startsWith("${CLAUDE_PLUGIN_ROOT}/")) {
        const p = path.join(repo, pluginDir, cmd.slice("${CLAUDE_PLUGIN_ROOT}/".length))
        try {
          accessSync(p, constants.X_OK)
        } catch {
          problems.push(`${event}: ${cmd} is missing or not executable`)
        }
      } else if (cmd.startsWith("${CLAUDE_PLUGIN_DATA}/")) {
        expect(hooks.SessionStart, `${event}: ${cmd} needs a SessionStart hook to install it`)
      } else {
        problems.push(`${event}: unexpected command ${cmd}`)
      }
      expect(typeof h.timeout === "number" && h.timeout <= 10, `${event}: timeout must be set and at most 10 s`)
    }
  }
}

if (problems.length) {
  for (const p of problems) console.error(`check-plugin: ${p}`)
  process.exit(1)
}
console.log(`plugin ${plugin.name} ${version}: ok`)
