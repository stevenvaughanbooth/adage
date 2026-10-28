// The one way the mod reaches the binary (docs/claude-mod/architecture.md §2 rule 1): every
// decision is `<bin> hook claude <Event>` with the payload on stdin; the answer is tagged so a
// failure is never mistaken for "nothing to say", and every failure is `{ ok: false }` (fail open,
// §6 invariant 2). What the mod draws it reads from `adage` itself, by a short list of exact
// argument vectors, every one read-only or a turn-down (`off`).
//
// No `$` crosses into this file (`claude plugin validate` follows `$` only within one file, and
// refuses a module that passes it across an import). register.ts hands `guard` and `guardSpawn`
// the functions that call `$.process.run` and `$.process.spawn`; both enforce the argv allow-list
// and set CLAUDECODE=1 on the child (F11: every child the mod runs is an agent session's), so every
// child below goes through them. The person-only verbs are not here: they run only through the
// press (plan B4), never from a hook, a feed line or a command.
import type { HookStream, ProcessRunInit, ProcessRunResult, ProcessSpawnChunk, ProcessSpawnRequest, ProcessSpawnResult } from 'claude-code'

import { BIN, CLI } from './plugin'

/**
 * The verbs this module may run, by their head (`argv[1]`; the status card's `--json` reads as
 * `status`). `isAllowed` holds each to its exact shape; `scripts/check-plugin.mjs` holds this
 * literal to architecture §2's list, plus `condition` (the Health tab's one action, ux-design §6.3)
 * and `delegate` (only `delegate status --json`: the lapsed grants the band and Health list).
 */
export const ALLOWED_VERBS: readonly string[] = ['hook', 'version', 'watch', 'graph', 'doctor', 'status', 'ledger', 'off', 'map', 'condition', 'delegate', 'world']

/** `$.process.run`, as register.ts hands it over. */
export type Run = (argv: readonly string[], init?: ProcessRunInit) => Promise<ProcessRunResult>

/** `$.process.spawn`, as register.ts hands it over. */
export type Spawn = (request: ProcessSpawnRequest) => HookStream<ProcessSpawnChunk, ProcessSpawnResult>

/** What a hook event's answer may carry; the binary owns the shape, the mod reads `predict`. */
type HookAnswer = { [key: string]: unknown }

export type Asked = { ok: true; answer: HookAnswer | undefined } | { ok: false }

/** A `--json` read: the object the verb printed, or a failure. */
export type Read = { ok: true; json: HookAnswer } | { ok: false }

const isExact = (argv: readonly string[], tail: readonly string[]): boolean =>
  argv.length === tail.length + 1 && tail.every((t, i) => argv[i + 1] === t)

/** The head of an argv as T-X-04 counts it: the status card (`--json --for-session`) is `status`. */
export function headOf(argv: readonly string[]): string {
  const verb = argv[1] ?? ''
  if (verb === '--json') return 'status'
  if (verb === 'ledger') return `ledger ${argv[2] ?? ''}`.trim()
  return verb
}

/**
 * Whether `argv` (binary first) is one the mod may run, each verb in its one exact shape:
 * `version`; `hook claude <Event>` (an event name of letters only); `watch --follow --json --stdin
 * ignore --repo <dir> --session <id> [--since <cursor>]`; `graph --json`; `doctor --json`; `status --json`;
 * `--json --for-session` (the status card); `ledger note`; `off`; `map --port 0`; `condition`;
 * `delegate status --json`; the world's reads and writes: `world --json`, `world descriptors
 * --json`, `world ingest --source <name> [--error <text>]`, `world follow --session <id> --stdin
 * ignore [--since <cursor>]` (never `world on`, a person's decision, nor `pause`/`resume`, the
 * person's own verbs at a terminal).
 */
export function isAllowed(argv: readonly string[]): boolean {
  const verb = argv[1]
  if (verb === undefined) return false
  if (verb === '--json') return isExact(argv, ['--json', '--for-session'])
  if (!ALLOWED_VERBS.includes(verb)) return false
  switch (verb) {
    case 'version':
    case 'off':
    case 'condition':
      return argv.length === 2
    case 'hook':
      return argv.length === 4 && argv[2] === 'claude' && /^[A-Za-z]+$/.test(argv[3] ?? '')
    case 'graph':
    case 'doctor':
    case 'status':
      return isExact(argv, [verb, '--json'])
    case 'ledger':
      return isExact(argv, ['ledger', 'note'])
    case 'map':
      return isExact(argv, ['map', '--port', '0'])
    case 'delegate':
      return isExact(argv, ['delegate', 'status', '--json'])
    case 'world': {
      if (isExact(argv, ['world', '--json']) || isExact(argv, ['world', 'descriptors', '--json'])) return true
      if (argv[2] === 'ingest') {
        const source = argv[4] ?? ''
        if (argv[3] !== '--source' || source === '' || source.startsWith('-')) return false
        if (argv.length === 5) return true
        return argv.length === 7 && argv[5] === '--error' && (argv[6] ?? '') !== ''
      }
      if (argv[2] === 'follow') {
        const session = argv[4] ?? ''
        if (argv[3] !== '--session' || session === '' || session.startsWith('-') || argv[5] !== '--stdin' || argv[6] !== 'ignore') return false
        if (argv.length === 7) return true
        return argv.length === 9 && argv[7] === '--since' && /^\d+$/.test(argv[8] ?? '')
      }
      return false
    }
    case 'watch': {
      const fixed = ['watch', '--follow', '--json', '--stdin', 'ignore', '--repo']
      if (!fixed.every((t, i) => argv[i + 1] === t)) return false
      const repo = argv[7] ?? ''
      const session = argv[9] ?? ''
      if (repo === '' || argv[8] !== '--session' || session === '' || session.startsWith('-')) return false
      if (argv.length === 10) return true
      return argv.length === 12 && argv[10] === '--since' && (argv[11] ?? '') !== ''
    }
    default:
      return false
  }
}

/**
 * The guarded runner: an argv off the allow-list is refused before any process starts, and the
 * child's environment always carries CLAUDECODE=1, whatever the caller passed.
 */
export function guard(run: Run): Run {
  return (argv, init = {}) => {
    if (!isAllowed(argv)) return Promise.reject(new Error(`adage bridge: argv not allowed: ${argv.slice(1).join(' ')}`))
    return run(argv, { ...init, env: { ...init.env, CLAUDECODE: '1' } })
  }
}

/** The guarded spawner: the same allow-list and CLAUDECODE=1 for a child the mod follows. */
export function guardSpawn(spawn: Spawn): Spawn {
  return request => {
    if (!isAllowed(request.argv)) throw new Error(`adage bridge: argv not allowed: ${request.argv.slice(1).join(' ')}`)
    return spawn({ ...request, env: { ...request.env, CLAUDECODE: '1' } })
  }
}

/**
 * The separator a path is spelled with: a backslash for a Windows path spelled with them (a drive,
 * `C:\Users\me`, or a share), `/` for every other (a Unix path, or Windows spelled with `/`).
 */
export const sepOf = (p: string): string => (/^(?:[A-Za-z]:\\|\\\\)/.test(p) ? '\\' : '/')

/** `base` and `parts` joined with base's own separator, base's trailing separators dropped. */
export function joinPath(base: string, ...parts: string[]): string {
  return [base.replace(/[\\/]+$/, ''), ...parts].join(sepOf(base))
}

/** A data directory's name for a plugin's key (`<plugin>@<source>`): every character but [A-Za-z0-9_-] as `-`. */
const dataName = (key: string): string => key.replace(/[^a-zA-Z0-9\-_]/g, '-')

/**
 * The plugin's data directory derived from its root, as Claude Code lays them out for an installed
 * plugin: root `<config>/plugins/cache/<marketplace>/<plugin>/<version>`, data
 * `<config>/plugins/data/<plugin@marketplace, non [A-Za-z0-9_-] as '-'>`, either separator.
 * Undefined for any other root (a --plugin-dir checkout).
 */
export function dataDirFromRoot(root: string): string | undefined {
  const m = /^(.*)[\\/]plugins[\\/]cache[\\/]([^\\/]+)[\\/]([^\\/]+)[\\/][^\\/]+[\\/]?$/.exec(root)
  if (m === null) return undefined
  const [, config = '', marketplace = '', plugin = ''] = m
  return joinPath(config, 'plugins', 'data', dataName(`${plugin}@${marketplace}`))
}

/**
 * The config directories the plugin's data may be under: CLAUDE_CONFIG_DIR when set (and then only
 * it), else `.claude` in the home directory (HOME, or USERPROFILE where Windows has no HOME; both
 * when they differ, as under Git Bash).
 */
export function configDirs(configDir: string | undefined, homes: readonly (string | undefined)[]): string[] {
  const configs = configDir ? [configDir] : homes.filter((h): h is string => !!h).map(h => joinPath(h, '.claude'))
  return [...new Set(configs.map(c => c.replace(/[\\/]+$/, '')))]
}

/**
 * Where the data directory of a plugin loaded with --plugin-dir may be: the engine keys it
 * `<name>@inline`, so `<config>/plugins/data/<name>-inline` under each of `configDirs`. Each a
 * candidate the shim may have written; the caller checks.
 */
export function inlineDataDirs(name: string, configDir: string | undefined, homes: readonly (string | undefined)[]): string[] {
  return configDirs(configDir, homes).map(c => joinPath(c, 'plugins', 'data', dataName(`${name}@inline`)))
}

/** A path compared as a path: either separator, no trailing one, a Windows drive path in any case. */
const pathKey = (p: string): string => {
  const k = p.replace(/\\/g, '/').replace(/\/+$/, '')
  return /^[A-Za-z]:\//.test(k) ? k.toLowerCase() : k
}
const isRecord = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)

/**
 * What the engine wrote about its plugins under one config directory, as read (the JSON text or
 * the parsed value; anything unreadable is undefined): `plugins/known_marketplaces.json` (each
 * marketplace's `installLocation` and `source.path`) and `plugins/installed_plugins.json` (each
 * `<plugin>@<marketplace>`'s `installPath`s).
 */
export type Registry = { config: string; known?: unknown; installed?: unknown }

const parsed = (v: unknown): unknown => {
  if (typeof v !== 'string') return v
  try {
    return JSON.parse(v)
  } catch {
    return undefined
  }
}

/**
 * The keys (`<name>@<marketplace>`) under which the engine loads the plugin whose folder is
 * `root`: a marketplace whose folder holds it (a directory marketplace's plugin is read in place:
 * `adage init`'s `adage@adage-local`, read from `$ADAGE_HOME/plugin/adage`), and an installed
 * entry whose `installPath` is it. Only the plugin's own name, in the order the files list them.
 */
export function registeredKeys(root: string, name: string, known: unknown, installed: unknown): string[] {
  const at = pathKey(root)
  const keys: string[] = []
  const k = parsed(known)
  if (isRecord(k)) {
    for (const [marketplace, entry] of Object.entries(k)) {
      if (!isRecord(entry)) continue
      const source = isRecord(entry.source) ? entry.source.path : undefined
      const homes = [entry.installLocation, source].filter((p): p is string => typeof p === 'string' && p !== '').map(pathKey)
      if (homes.some(h => at === h || at.startsWith(`${h}/`))) keys.push(`${name}@${marketplace}`)
    }
  }
  const i = parsed(installed)
  const plugins = isRecord(i) && isRecord(i.plugins) ? i.plugins : {}
  for (const [key, entries] of Object.entries(plugins)) {
    if (!key.startsWith(`${name}@`) || !Array.isArray(entries)) continue
    if (entries.some(e => isRecord(e) && typeof e.installPath === 'string' && pathKey(e.installPath) === at)) keys.push(key)
  }
  return [...new Set(keys)]
}

/**
 * Where the plugin's data directory may be when the engine's process does not carry
 * CLAUDE_PLUGIN_DATA (it hands it to command hooks only), in order: derived from an installed
 * plugin's root (and then only that); else, under each config directory, the data directory of
 * each key the engine registered the plugin's folder under (`registeredKeys`), then the
 * --plugin-dir layout's (`<name>-inline`). Each a candidate the shim may have written; the
 * caller checks. A key the folder is not registered under is never a candidate: its link may be
 * another install's.
 */
export function dataDirCandidates(root: string, name: string, registries: readonly Registry[]): string[] {
  const installed = dataDirFromRoot(root)
  if (installed !== undefined) return [installed]
  const out: string[] = []
  for (const r of registries) {
    const data = (key: string) => joinPath(r.config, 'plugins', 'data', dataName(key))
    out.push(...registeredKeys(root, name, r.known, r.installed).map(data), data(`${name}@inline`))
  }
  return [...new Set(out)]
}

/**
 * The suffix the session-start shim gives the entries it makes: on Windows it copies `hook.exe`
 * and `adage.exe` (a link would not run), elsewhere it links `hook` and `adage`. Windows is read
 * from the engine's environment (`OS=Windows_NT`, which Windows sets for every process).
 */
export const exeSuffix = (os: string | undefined): string => (os === 'Windows_NT' ? '.exe' : '')

/**
 * `<data>/bin/<BIN>`, the link the session-start shim made in the plugin's data directory
 * (binary.ts `dataDir`). There is no PATH fallback: when the shim linked `scripts/quiet` (no
 * binary could serve) the probe reads nothing and the mod stays inert, as the command hooks do.
 */
export function hookBinary(data: string | undefined, exe = ''): string | undefined {
  return data ? joinPath(data, 'bin', `${BIN}${exe}`) : undefined
}

/** `<data>/bin/adage`, which the shim links beside `bin/hook`: the verbs the mod draws from. */
export function cliBinary(data: string | undefined, exe = ''): string | undefined {
  return data ? joinPath(data, 'bin', `${CLI}${exe}`) : undefined
}

/**
 * The presence probe (`<bin> version`): a binary is there when it exits 0 and prints something.
 * `scripts/quiet` prints nothing, so with no binary the mod stays inert. Resolves the binary that
 * serves, or undefined.
 */
export async function probe(run: Run, bin: string | undefined): Promise<string | undefined> {
  if (bin === undefined) return undefined
  try {
    const ran = await run([bin, 'version'], { timeoutMs: 1000 })
    return ran.exitCode === 0 && ran.stdout.trim() !== '' ? bin : undefined
  } catch {
    return undefined // cannot start (no link yet), timed out, refused
  }
}

const parseObject = (text: string): HookAnswer | undefined => {
  const parsed: unknown = JSON.parse(text)
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return undefined
  return parsed as HookAnswer
}

/**
 * Asks the binary about one event. `{ ok: true, answer: undefined }` is "nothing to do" (exit 0,
 * empty stdout); a JSON object is the answer; everything else is `{ ok: false }`.
 */
export async function ask(
  run: Run,
  bin: string | undefined,
  event: string,
  payload: { cwd?: string; [key: string]: unknown },
  timeoutMs: number,
): Promise<Asked> {
  if (bin === undefined) return { ok: false }
  try {
    const init: ProcessRunInit = { stdin: JSON.stringify(payload), timeoutMs }
    if (typeof payload.cwd === 'string' && payload.cwd !== '') init.cwd = payload.cwd
    const ran = await run([bin, 'hook', 'claude', event], init)
    if (ran.exitCode !== 0 || ran.isStdoutTruncated) return { ok: false }
    const text = ran.stdout.trim()
    if (text === '') return { ok: true, answer: undefined }
    const answer = parseObject(text)
    return answer === undefined ? { ok: false } : { ok: true, answer }
  } catch {
    return { ok: false }
  }
}

/** How long a read of `adage … --json` may take before the mod draws without it. */
const READ_TIMEOUT_MS = 10_000

/**
 * Reads one `adage … --json` object (the card, `graph`, `doctor`, `status`, `delegate status`),
 * in the session's directory. Anything but exit 0 with one JSON object is `{ ok: false }`, and the
 * drawing keeps what it had.
 */
export async function read(run: Run, cli: string | undefined, argv: readonly string[], cwd: string): Promise<Read> {
  if (cli === undefined) return { ok: false }
  try {
    const init: ProcessRunInit = { timeoutMs: READ_TIMEOUT_MS }
    if (cwd !== '') init.cwd = cwd
    const ran = await run([cli, ...argv], init)
    if (ran.exitCode !== 0 || ran.isStdoutTruncated) return { ok: false }
    const json = parseObject(ran.stdout.trim())
    return json === undefined ? { ok: false } : { ok: true, json }
  } catch {
    return { ok: false }
  }
}

/** Runs one allowed verb that prints a line for the person (`off`, `condition`): its stdout, or undefined. */
export async function say(run: Run, cli: string | undefined, argv: readonly string[], cwd: string): Promise<string | undefined> {
  if (cli === undefined) return undefined
  try {
    const init: ProcessRunInit = { timeoutMs: 120_000 }
    if (cwd !== '') init.cwd = cwd
    const ran = await run([cli, ...argv], init)
    return ran.exitCode === 0 ? ran.stdout.trim() : undefined
  } catch {
    return undefined
  }
}

/** How long `adage ledger note` may take: it merges under the ledger's lock, off every hook's path. */
export const NOTE_TIMEOUT_MS = 10_000

/**
 * Hands the session's usage to `adage ledger note` (plan A4, architecture §3.9) on stdin, in the
 * session's directory. True when the binary recorded it; a refusal (exit 2: a malformed note, or a
 * session the spool does not know) or any failure is false, and nothing else happens (fail open).
 */
export async function note(run: Run, cli: string | undefined, payload: unknown, cwd: string): Promise<boolean> {
  if (cli === undefined) return false
  try {
    const init: ProcessRunInit = { stdin: JSON.stringify(payload), timeoutMs: NOTE_TIMEOUT_MS }
    if (cwd !== '') init.cwd = cwd
    const ran = await run([cli, 'ledger', 'note'], init)
    return ran.exitCode === 0
  } catch {
    return false
  }
}
