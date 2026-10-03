// The one way the mod reaches the binary (docs/claude-mod/architecture.md §2 rule 1): every
// decision is `<bin> hook claude <Event>` with the payload on stdin; the answer is tagged so a
// failure is never mistaken for "nothing to say", and every failure is `{ ok: false }` (fail open,
// §6 invariant 2).
//
// No `$` crosses into this file (`claude plugin validate` follows `$` only within one file, and
// refuses a module that passes it across an import). register.ts hands `guard` the one function
// that calls `$.process.run`; `guard` enforces the argv allow-list and sets CLAUDECODE=1 on the
// child (F11: every child the mod runs is an agent session's), so every run below goes through it.
import type { ProcessRunInit, ProcessRunResult } from 'claude-code'

import { BIN } from './plugin'

/** The verbs this module may run (POC scope: hook events and the activation probe). */
export const ALLOWED_VERBS: readonly string[] = ['hook', 'version']

/** `$.process.run`, as register.ts hands it over. */
export type Run = (argv: readonly string[], init?: ProcessRunInit) => Promise<ProcessRunResult>

/** What a hook event's answer may carry; the binary owns the shape, the mod reads `predict`. */
export type HookAnswer = { [key: string]: unknown }

export type Asked = { ok: true; answer: HookAnswer | undefined } | { ok: false }

/**
 * Whether `argv` (binary first) is one the mod may run: `<bin> version`, or
 * `<bin> hook claude <Event>` with an event name of letters only.
 */
export function isAllowed(argv: readonly string[]): boolean {
  const verb = argv[1]
  if (verb === undefined || !ALLOWED_VERBS.includes(verb)) return false
  if (verb === 'version') return argv.length === 2
  return argv.length === 4 && argv[2] === 'claude' && /^[A-Za-z]+$/.test(argv[3] ?? '')
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

/**
 * `<data>/bin/<BIN>`, the link the session-start shim made, from CLAUDE_PLUGIN_DATA when the
 * engine's process carries it, else derived from the plugin root. There is no PATH fallback: when
 * the shim linked `scripts/quiet` (no binary could serve) the probe reads nothing and the mod stays
 * inert, as the command hooks do.
 */
export function hookBinary(pluginData: string | undefined, pluginRoot: string): string | undefined {
  const data = pluginData || dataDirFromRoot(pluginRoot)
  return data ? `${data.replace(/\/+$/, '')}/bin/${BIN}` : undefined
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
    const parsed: unknown = JSON.parse(text)
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return { ok: false }
    return { ok: true, answer: parsed as HookAnswer }
  } catch {
    return { ok: false }
  }
}
