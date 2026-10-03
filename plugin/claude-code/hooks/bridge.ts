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
export const ALLOWED_VERBS: readonly string[] = ['hook', 'version', 'watch', 'graph', 'doctor', 'status', 'ledger', 'off', 'map', 'condition', 'delegate']

/** `$.process.run`, as register.ts hands it over. */
export type Run = (argv: readonly string[], init?: ProcessRunInit) => Promise<ProcessRunResult>

/** `$.process.spawn`, as register.ts hands it over. */
export type Spawn = (request: ProcessSpawnRequest) => HookStream<ProcessSpawnChunk, ProcessSpawnResult>

/** What a hook event's answer may carry; the binary owns the shape, the mod reads `predict`. */
export type HookAnswer = { [key: string]: unknown }

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
 * `delegate status --json`.
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
 * The plugin's data directory derived from its root, as Claude Code lays them out for an installed
 * plugin: root `<config>/plugins/cache/<marketplace>/<plugin>/<version>`, data
 * `<config>/plugins/data/<plugin@marketplace, non [A-Za-z0-9_-] as '-'>`. Undefined for any other
 * root (a --plugin-dir checkout), where only CLAUDE_PLUGIN_DATA can say.
 */
export function dataDirFromRoot(root: string): string | undefined {
  const m = /^(.*)\/plugins\/cache\/([^/]+)\/([^/]+)\/[^/]+\/?$/.exec(root)
  if (m === null) return undefined
  const [, config, marketplace, plugin] = m
  return `${config}/plugins/data/${`${plugin}@${marketplace}`.replace(/[^a-zA-Z0-9\-_]/g, '-')}`
}

/** The plugin's data directory: CLAUDE_PLUGIN_DATA when the engine carries it, else derived. */
export function dataDir(pluginData: string | undefined, pluginRoot: string): string | undefined {
  const data = pluginData || dataDirFromRoot(pluginRoot)
  return data ? data.replace(/\/+$/, '') : undefined
}

/**
 * `<data>/bin/<BIN>`, the link the session-start shim made, from CLAUDE_PLUGIN_DATA when the
 * engine's process carries it, else derived from the plugin root. There is no PATH fallback: when
 * the shim linked `scripts/quiet` (no binary could serve) the probe reads nothing and the mod stays
 * inert, as the command hooks do.
 */
export function hookBinary(pluginData: string | undefined, pluginRoot: string): string | undefined {
  const data = dataDir(pluginData, pluginRoot)
  return data ? `${data}/bin/${BIN}` : undefined
}

/** `<data>/bin/adage`, which the shim links beside `bin/hook`: the verbs the mod draws from. */
export function cliBinary(pluginData: string | undefined, pluginRoot: string): string | undefined {
  const data = dataDir(pluginData, pluginRoot)
  return data ? `${data}/bin/${CLI}` : undefined
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
export const READ_TIMEOUT_MS = 10_000

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
