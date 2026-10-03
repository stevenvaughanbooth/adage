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

// The mod's structure (docs/claude-mod/impl/mod-structure.md): register.ts is the only file that
// touches `$`, and holds the state and store tables' cells and one delegating `on(...)` line per
// hook, in VALIDATE.txt's order; every other module takes a `Mod`, has its test file, and the tests
// share one set of fixtures. Each rule below is one of that page's "how to add" steps.
checkModStructure("plugin/claude-code")

function checkModStructure(pluginDir) {
  const hooks = path.join(repo, pluginDir, "hooks")
  const rel = (f) => path.relative(path.join(repo, pluginDir), f)
  const walk = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((d) => (d.isDirectory() ? walk(path.join(dir, d.name)) : [path.join(dir, d.name)]))
  const files = walk(hooks).filter((f) => /\.tsx?$/.test(f))
  const tests = files.filter((f) => /\.test\.tsx?$/.test(f))
  const modules = files.filter((f) => !tests.includes(f) && !f.includes(`${path.sep}fixtures${path.sep}`))
  const src = (f) => readFileSync(f, "utf8")
  const register = path.join(hooks, "register.ts")
  const reg = src(register)

  // `$` only in register.ts: no other module reads or writes state, the store or the engine.
  for (const f of modules) {
    if (f === register) continue
    const text = src(f).replace(/^\s*(\/\/|\*|\/\*\*).*$/gm, "")
    expect(!/\$\.[a-z]+\.[a-zA-Z]+\(/.test(text), `${rel(f)}: calls \`$\`; only register.ts does (it hands modules a Mod)`)
    expect(!/import\s*\{[^}]*\b(atom|read|update)\b[^}]*\}\s*from\s*'claude-code'/.test(text), `${rel(f)}: imports the state library; only register.ts holds the atoms`)
  }

  // The state table: types/index.d.ts, plugin.ts `STATE0`, register.ts's atoms and its cells, the same keys in order.
  const contract = src(path.join(repo, pluginDir, "types/index.d.ts")).match(/interface PluginState\s*\{\s*adage:\s*\{([^}]*)\}/)?.[1] ?? ""
  const typeKeys = [...contract.matchAll(/^\s*(\w+):/gm)].map((m) => m[1])
  const state0 = src(path.join(hooks, "plugin.ts")).match(/export const STATE0: AdageState = \{([^}]*)\}/)?.[1] ?? ""
  const initialKeys = [...state0.matchAll(/^\s*(\w+):/gm)].map((m) => m[1])
  const atoms = [...reg.matchAll(/^const (\w+) = atom\(\{ plugin: 'adage', key: '(\w+)' \} as const, STATE0\.(\w+)\)$/gm)]
  const cells = [...reg.matchAll(/^\s+(\w+): \{ read: \(\) => read\(\$, (\w+)\), update: f => update\(\$, (\w+), f\) \},$/gm)]
  const byConst = new Map(atoms.map((m) => [m[1], m[2]]))
  expect(typeKeys.length > 0, `${pluginDir}/types/index.d.ts: no PluginState.adage`)
  expect(initialKeys.join() === typeKeys.join(), `${pluginDir}/hooks/plugin.ts: STATE0's keys (${initialKeys}) are not types/index.d.ts's (${typeKeys})`)
  expect(atoms.map((m) => m[2]).join() === typeKeys.join(), `${pluginDir}/hooks/register.ts: the atoms' keys (${atoms.map((m) => m[2])}) are not the state table's (${typeKeys})`)
  for (const m of atoms) expect(m[2] === m[3], `${pluginDir}/hooks/register.ts: atom ${m[1]} starts from STATE0.${m[3]}, not STATE0.${m[2]}`)
  expect(cells.map((m) => m[1]).join() === typeKeys.join(), `${pluginDir}/hooks/register.ts: the state cells (${cells.map((m) => m[1])}) are not the state table's (${typeKeys})`)
  for (const m of cells) expect(byConst.get(m[2]) === m[1] && m[2] === m[3], `${pluginDir}/hooks/register.ts: the ${m[1]} cell reads ${m[2]} and writes ${m[3]}, not its own atom`)

  // The store table: plugin.ts `STORE_KEYS` and register.ts's store cells; no other `$.store` key.
  const storeKeys = (src(path.join(hooks, "plugin.ts")).match(/export const STORE_KEYS = \[([^\]]*)\] as const/)?.[1] ?? "").split(",").map((v) => v.trim().replace(/^'|'$/g, "")).filter(Boolean)
  const storeCells = [...reg.matchAll(/^\s+(\w+): \{ get: \(\) => \$\.store\.get\('adage\.(\w+)'\), set: v => \$\.store\.set\('adage\.(\w+)', v\) \},$/gm)]
  expect(storeKeys.length > 0, `${pluginDir}/hooks/plugin.ts: no STORE_KEYS literal`)
  expect(storeCells.map((m) => m[1]).join() === storeKeys.join(), `${pluginDir}/hooks/register.ts: the store cells (${storeCells.map((m) => m[1])}) are not STORE_KEYS (${storeKeys})`)
  for (const m of storeCells) expect(m[1] === m[2] && m[2] === m[3], `${pluginDir}/hooks/register.ts: the ${m[1]} store cell names adage.${m[2]} / adage.${m[3]}`)
  expect([...reg.matchAll(/\$\.store\.(get|set)\(/g)].length === storeKeys.length * 2, `${pluginDir}/hooks/register.ts: a $.store call outside the store cells`)

  // The runners: `$.process` only inside the guards (bridge.ts `guard`, `guardSpawn`; press.ts `guardPress`).
  const runs = [...reg.matchAll(/\$\.process\.(run|spawn)\(/g)].length
  const guarded = [...reg.matchAll(/guard(Spawn|Press)?\(\([^)]*\)( => |: ProcessSpawnRequest\) => )\$\.process\.(run|spawn)\(/g)].length
  expect(runs === 3 && guarded === 3, `${pluginDir}/hooks/register.ts: every $.process call is the argument of guard, guardSpawn or guardPress (found ${runs} calls, ${guarded} guarded)`)

  // The hooks: one delegating `on(...)` line each, in VALIDATE.txt's order, to an exported `on…` of the module.
  const imports = new Map([...reg.matchAll(/^import \* as (\w+) from '\.\/([\w/]+)'$/gm)].map((m) => [m[1], m[2]]))
  const ons = [...reg.matchAll(/^\s*(?:if \([^)]*\) )?on\('([\w.]+)', (?:(\{.*?\}), )?async (?:function\* )?\(\$, e(?:, next)?\) (?:=> |\{ return yield\* )(\w+)\.(on\w+)\(mod\(\$, options\), e(?:, next)?\)(?: \})?\)$/gm)]
  const onCount = [...reg.matchAll(/^\s*(?:if \([^)]*\) )?on\(/gm)].length
  expect(ons.length === onCount, `${pluginDir}/hooks/register.ts: ${onCount - ons.length} on(...) line(s) do more than delegate to a module's on… function with mod($, options)`)
  const matcherText = (m) => {
    if (m === undefined) return ""
    const o = JSON.parse(m.replace(/'/g, '"').replace(/(\w+):/g, '"$1":'))
    const part = (k, v) => (Array.isArray(v) ? `${k}=${v.join("|")}` : `${k}=${v}`)
    return `{${Object.entries(o).map(([k, v]) => (k === "props" ? `props has {${Object.entries(v).map(([a, b]) => part(a, b)).join(", ")}}` : part(k, v))).join(", ")}}`
  }
  const listed = ons.map((m) => `${m[1]}${matcherText(m[2])}`).join(", ")
  const validated = (src(path.join(repo, pluginDir, "VALIDATE.txt")).match(/> \.\/register\.ts hooks: (.*)$/m)?.[1] ?? "").replace(/… \[\+\d+ chars\]$/, "")
  if (validated === "" || !listed.startsWith(validated)) {
    let k = 0
    while (k < validated.length && listed[k] === validated[k]) k++
    const comma = listed.lastIndexOf(", ", k)
    const at = comma < 0 ? 0 : comma + 2
    problems.push(`${pluginDir}/hooks/register.ts: the hooks in order differ from VALIDATE.txt's at \`${listed.slice(at, at + 60)}\` (VALIDATE.txt: \`${validated.slice(at, at + 60)}\`); a hook added or moved needs VALIDATE.txt regenerated`)
  }
  for (const m of ons) {
    const mod = imports.get(m[3])
    const file = mod === undefined ? undefined : [".ts", ".tsx"].map((x) => path.join(hooks, `${mod}${x}`)).find(existsSync)
    expect(file !== undefined, `${pluginDir}/hooks/register.ts: ${m[3]} is not a module of hooks/`)
    if (file !== undefined) expect(new RegExp(`^export (async )?function\\*? ${m[4]}\\(m: Mod`, "m").test(src(file)), `${rel(file)}: no exported ${m[4]}(m: Mod, …) for ${m[1]}`)
  }

  // A press comes from a gesture only dialog.ts and allow.ts hold.
  for (const f of modules) {
    const holds = /import\s*\{[^}]*\b(ANSWER|BUTTON)\b[^}]*\}\s*from\s*'\.\/press'/.test(src(f))
    expect(!holds || ["dialog.ts", "allow.ts"].includes(path.basename(f)), `${rel(f)}: holds a press gesture; only dialog.ts and allow.ts do`)
  }

  // One test file per module, named by it (a module of types alone excepted); test names carry the
  // test plan's ids; the world beneath the plugin is fixtures/kit.ts's alone.
  for (const f of modules) {
    const typesOnly = !/^export (async )?(function|const|let|class)\b/m.test(src(f))
    const test = f.replace(/\.tsx?$/, ".test.ts")
    expect(typesOnly || existsSync(test), `${rel(f)}: no ${path.basename(test)} beside it`)
  }
  for (const f of tests) {
    const text = src(f)
    const titled = [...text.matchAll(/^(describe|test)\((['"`])(.*?)\2/gm)]
    for (const t of titled) expect(/T-[A-Z0-9]+-\d\d/.test(t[3]), `${rel(f)}: ${t[1]} '${t[3]}' names no test-plan id (T-…)`)
    expect(!/\bon\('(process\.run|process\.spawn|env\.get)'|mock\.(env|clock)\(|function world\(/.test(text), `${rel(f)}: stubs the engine itself; the world beneath the plugin is fixtures/kit.ts's`)
  }
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
