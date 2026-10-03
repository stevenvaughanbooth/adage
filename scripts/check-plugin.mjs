#!/usr/bin/env node
// Structural checks on the Claude Code plugins and marketplace that `claude plugin validate` does
// not make: versions agree with the workspace, and every hook command points at something that
// exists. One plugin serves, `adage`, shipping both binaries (plan A8): its session-start shim
// links bin/hook to adage-exec while the execution marker exists, else to adage, so the hooks are
// registered once and never fire twice. `adage-exec` stays one release as a retired alias that
// registers its SessionStart alone and quiets what an earlier version left behind.
//
// The marketplace installs each plugin from the `marketplace` branch (a `git-subdir` source), which
// the release workflow writes with verified binaries for every target bundled
// (scripts/stage-plugin.mjs). `--staged <dir>` checks such a staged tree: every target present,
// every binary matching dist/SHA256SUMS.
//
//   node scripts/check-plugin.mjs [--staged <dir>]

import { createHash } from "node:crypto"
import { accessSync, constants, existsSync, readdirSync, readFileSync } from "node:fs"
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

// The plugin that serves, and the retired alias (`alias`: SessionStart alone, no module).
for (const [pluginDir, alias] of [
  ["plugin/claude-code", false],
  ["plugin/claude-code-exec", true],
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
  if (alias) {
    // The retired adage-exec plugin: one SessionStart that runs no binary, and nothing else.
    const h = json(`${pluginDir}/hooks/hooks.json`)
    expect(Object.keys(h.hooks || {}).join() === "SessionStart", `${pluginDir}: the retired alias registers SessionStart alone`)
    expect(!("modules" in h), `${pluginDir}: the retired alias loads no hooks module`)
    expect(!("types" in plugin), `${pluginDir}: the retired alias names no types`)
    expect(shim.includes("uninstall this one"), `${pluginDir}: the retired alias's shim does not ask to be uninstalled`)
    expect(!/\bexec "\$DATA\/bin/.test(shim), `${pluginDir}: the retired alias's shim runs a binary`)
    continue
  }
  expect(shim.includes("exec/enabled"), `${pluginDir}: session-start does not read the exec marker`)
  expect(shim.includes("$DATA/bin/hook") && shim.includes("adage-exec"), `${pluginDir}: session-start does not link bin/hook to adage or adage-exec`)

  // The mod (docs/claude-mod/architecture.md §2): hooks.json may name one hooks module under
  // `modules` beside the command hooks; the module, the types contract plugin.json names, and the
  // bridge's allow-list must be there. Only `hooks` and `modules` are read from hooks.json.
  const hooksFile = json(`${pluginDir}/hooks/hooks.json`)
  for (const key of Object.keys(hooksFile)) {
    expect(key === "hooks" || key === "modules", `${pluginDir}/hooks/hooks.json: unexpected key ${key}`)
  }
  if ("modules" in hooksFile) {
    const modules = hooksFile.modules
    expect(Array.isArray(modules) && modules.length === 1, `${pluginDir}/hooks/hooks.json: modules must name exactly one hooks module`)
    for (const m of Array.isArray(modules) ? modules : []) {
      const ok = typeof m === "string" && /^\.\/[\w./-]+\.(ts|tsx|js|mjs)$/.test(m) && !m.includes("..")
      expect(ok, `${pluginDir}/hooks/hooks.json: module ${m} must be a ./ path to a .ts, .tsx, .js or .mjs file inside hooks/`)
      if (ok) expect(existsSync(path.join(repo, pluginDir, "hooks", m)), `${pluginDir}/hooks/hooks.json: module ${m} does not exist`)
    }
    expect(typeof plugin.types === "string", `${pluginDir}: plugin.json must name the module's $.state contract under "types"`)
    const bridge = path.join(repo, pluginDir, "hooks/bridge.ts")
    expect(existsSync(bridge), `${pluginDir}: the module has no hooks/bridge.ts`)
    if (existsSync(bridge)) {
      const list = readFileSync(bridge, "utf8").match(/export const ALLOWED_VERBS: readonly string\[\] = \[([^\]]*)\]/)?.[1]
      expect(list !== undefined, `${pluginDir}/hooks/bridge.ts: no ALLOWED_VERBS literal`)
      const verbs = (list ?? "").split(",").map((v) => v.trim().replace(/^['"]|['"]$/g, "")).filter(Boolean)
      // Person-only verbs, install and config never run from a hook (architecture §2, §6 3a). The
      // list is architecture §2's, plus `condition` (the Health tab's one action) and `delegate`
      // (only `delegate status --json`, read for the lapsed grants), in this order: a change to it
      // is a change here and to bridge.test.ts together (T-B1-05, T-X-04).
      const allowed = ["hook", "version", "watch", "graph", "doctor", "status", "ledger", "off", "map", "condition", "delegate"]
      for (const v of verbs) {
        expect(allowed.includes(v), `${pluginDir}/hooks/bridge.ts: verb ${v} may not be in the allow-list`)
      }
      expect(verbs.join(",") === allowed.join(","), `${pluginDir}/hooks/bridge.ts: the allow-list changed (${verbs.join(", ")}); change scripts/check-plugin.mjs and the bridge test with it`)
      // The follower never reads stdin: the engine closes a spawned child's, which a follower
      // reading it as its parent's pipe would take for the parent's end (B2-B3 notes, issue 1).
      const watch = readFileSync(bridge, "utf8").match(/const fixed = \[([^\]]*)\]/)?.[1] ?? ""
      const shape = watch.split(",").map((v) => v.trim().replace(/^['"]|['"]$/g, "")).filter(Boolean).join(" ")
      expect(shape === "watch --follow --json --stdin ignore --repo", `${pluginDir}/hooks/bridge.ts: the watch shape must be exactly \`watch --follow --json --stdin ignore --repo …\` (found \`${shape}\`)`)
    }
    // The person-only verbs the mod runs only through a press (architecture §3.12, plan B4): a
    // second literal in hooks/press.ts, exactly these, never one the allow-list above may run and
    // never install, config, share, train, packs or init.
    const press = path.join(repo, pluginDir, "hooks/press.ts")
    expect(existsSync(press), `${pluginDir}: the module has no hooks/press.ts`)
    if (existsSync(press)) {
      const list = readFileSync(press, "utf8").match(/export const PRESS_VERBS: readonly string\[\] = \[([^\]]*)\]/)?.[1]
      expect(list !== undefined, `${pluginDir}/hooks/press.ts: no PRESS_VERBS literal`)
      const verbs = (list ?? "").split(",").map((v) => v.trim().replace(/^['"]|['"]$/g, "")).filter(Boolean)
      expect(verbs.join(",") === "review,delegate,on,add", `${pluginDir}/hooks/press.ts: the press list changed (${verbs.join(", ")}); change scripts/check-plugin.mjs with it`)
      for (const v of ["install", "config", "share", "train", "packs", "init"]) {
        expect(!verbs.includes(v), `${pluginDir}/hooks/press.ts: ${v} is never a press`)
      }
      // A press comes from a person in this session: a dialog answer or a pane Button, never an
      // answer read back from `$.store` (review H1: a file the agent's tools can write).
      const gestures = readFileSync(press, "utf8").match(/const GESTURES: readonly Gesture\[\] = \[([^\]]*)\]/)?.[1]
      expect(gestures !== undefined, `${pluginDir}/hooks/press.ts: no GESTURES literal`)
      const g = (gestures ?? "").split(",").map((v) => v.trim()).filter(Boolean)
      expect(g.join(",") === "ANSWER,BUTTON", `${pluginDir}/hooks/press.ts: the gestures changed (${g.join(", ")}); a press is a dialog answer or a pane Button in this session, nothing else`)
    }
    // Two settings rows and no more (ux-design §8): decisions are never behind a setting.
    const rows = Object.keys(plugin.userConfig ?? {})
    expect(rows.join(",") === "band,pane", `${pluginDir}: plugin.json userConfig must be exactly band and pane (found ${rows.join(", ") || "none"})`)
    if (plugin.userConfig?.pane) {
      expect(JSON.stringify(plugin.userConfig.pane.options) === JSON.stringify(["auto", "manual"]) && plugin.userConfig.pane.default === "manual", `${pluginDir}: userConfig.pane is auto | manual, manual by default`)
    }
    if (plugin.userConfig?.band) {
      expect(plugin.userConfig.band.type === "boolean" && plugin.userConfig.band.default === true, `${pluginDir}: userConfig.band is a boolean, on by default`)
    }
  }
  if (typeof plugin.types === "string") {
    expect(existsSync(path.join(repo, pluginDir, plugin.types)), `${pluginDir}: plugin.json types ${plugin.types} does not exist`)
  }

  // Hook commands: ${CLAUDE_PLUGIN_ROOT} paths must exist and be executable; ${CLAUDE_PLUGIN_DATA}
  // paths are created at SessionStart, which must therefore come first.
  const hooks = hooksFile.hooks
  for (const [event, groups] of Object.entries(hooks)) {
    for (const g of groups) {
      for (const h of g.hooks) {
        const cmd = h.command || ""
        if (cmd.startsWith("${CLAUDE_PLUGIN_ROOT}/")) {
          const p = path.join(repo, pluginDir, cmd.slice("${CLAUDE_PLUGIN_ROOT}/".length))
          expect(executable(p), `${pluginDir} ${event}: ${cmd} is missing or not executable`)
        } else if (cmd.startsWith("${CLAUDE_PLUGIN_DATA}/")) {
          expect(hooks.SessionStart, `${pluginDir} ${event}: ${cmd} needs a SessionStart hook to install it`)
          expect(cmd === "${CLAUDE_PLUGIN_DATA}/bin/hook", `${pluginDir} ${event}: runs ${cmd}, not bin/hook`)
        } else {
          problems.push(`${pluginDir} ${event}: unexpected command ${cmd}`)
        }
        // An E2 action runs at the edit and Stop events (at most 20 s of its own), whichever
        // binary bin/hook is this session: those get 30 s; everything else stays at most 10 s.
        const action = event === "PostToolBatch" || event === "Stop"
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

// The mod's API is pinned (architecture §6 invariant 10): CI installs the Claude Code version the
// plugin was validated and tested against, never the latest.
const ci = read(".github/workflows/ci.yml")
for (const m of ci.matchAll(/npm install -g @anthropic-ai\/claude-code(@\S+)?/g)) {
  expect(/^@\d+\.\d+\.\d+$/.test(m[1] || ""), `.github/workflows/ci.yml: the Claude Code CLI must be pinned to an exact version (${m[0]})`)
}
// One module, in the plugin that serves (A8): the retired alias ships none.
{
  const ts = existsSync(path.join(repo, "plugin/claude-code-exec/hooks"))
    ? readdirSync(path.join(repo, "plugin/claude-code-exec/hooks")).filter((f) => /\.tsx?$/.test(f))
    : []
  expect(ts.length === 0, `plugin/claude-code-exec ships hooks module files (${ts}); the mod lives in plugin/claude-code alone`)
}

// A staged marketplace tree (the release workflow's output).
const stagedAt = process.argv.indexOf("--staged")
if (stagedAt > 0) {
  const root = path.resolve(process.argv[stagedAt + 1] || "")
  for (const [pluginDir, bins] of [["plugin/claude-code", ["adage", "adage-exec"]]]) {
    const dist = path.join(root, pluginDir, "dist")
    const sums = existsSync(path.join(dist, "SHA256SUMS")) ? readFileSync(path.join(dist, "SHA256SUMS"), "utf8") : ""
    for (const p of PLATFORMS) {
      for (const b of bins) {
        // Execution needs an OS sandbox Windows does not have: no adage-exec there.
        if (p.os === "win32" && b === "adage-exec") continue
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
