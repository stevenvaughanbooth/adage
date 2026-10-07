// The active binary, found and asked (architecture §2 rule 1; ux-design §9.2): `dataDir` finds
// the plugin's data directory, where the session-start shim links the binary, and every hook event
// the mod sends goes through `askBinary`, which counts the binary's failed answers in a row and, at
// three, warns once that hooks are not answering. bridge.ts makes the run itself.
import type { Asked, Registry } from './bridge'
import { ask, configDirs, dataDirCandidates, exeSuffix, hookBinary, joinPath } from './bridge'
import type { Mod } from './mod'
import { warnOnce } from './notify'

/**
 * The plugin's data directory: CLAUDE_PLUGIN_DATA when the engine's process carries it, else where
 * Claude Code lays it out (bridge.ts `dataDirCandidates`), the first candidate in which the shim
 * made `bin/hook`. The engine substitutes CLAUDE_PLUGIN_DATA into command hooks only, so the mod
 * derives it itself: from an installed plugin's cache root, from the key the engine registered the
 * plugin's folder under (its `plugins/known_marketplaces.json` and `installed_plugins.json`: `adage
 * init`'s `adage@adage-local` is read in place from `$ADAGE_HOME/plugin/adage`, data
 * `<config>/plugins/data/adage-adage-local`), or `<name>-inline` for --plugin-dir. A candidate is
 * taken only once the shim has linked a binary there. Undefined while there is none: the mod
 * stays inert.
 */
export async function dataDir(m: Mod): Promise<string | undefined> {
  const given = await m.env.pluginData()
  if (given) return given.replace(/[\\/]+$/, '')
  const homes = [await m.env.home(), await m.env.userProfile()]
  const read = (path: string) => m.fs.read(path).catch(() => undefined)
  const registries: Registry[] = await Promise.all(
    configDirs(await m.env.configDir(), homes).map(async config => ({
      config,
      known: await read(joinPath(config, 'plugins', 'known_marketplaces.json')),
      installed: await read(joinPath(config, 'plugins', 'installed_plugins.json')),
    })),
  )
  const exe = exeSuffix(await m.env.os())
  for (const data of dataDirCandidates(m.plugin.root(), m.plugin.name(), registries)) {
    const hook = hookBinary(data, exe)
    if (hook !== undefined && (await m.fs.exists(hook).catch(() => false))) return data
  }
  return undefined
}

/** The bridge's failures in a row before the status line says so (ux-design §9.2). */
export const UNANSWERING = 3

/** One question to the active binary, tagged; three failures in a row warn once. */
export async function askBinary(m: Mod, event: string, payload: { cwd?: string }, timeoutMs: number): Promise<Asked> {
  const asked = await ask(m.run, m.rt.bin, event, payload, timeoutMs)
  if (m.rt.bin === undefined) return asked
  m.rt.failures = asked.ok ? 0 : m.rt.failures + 1
  if (m.rt.failures >= UNANSWERING) await warnOnce(m, 'unanswering', 'hooks are not answering; run adage doctor').catch(() => undefined)
  return asked
}
