// The press (plan B4, architecture §3.12): the one path by which the mod runs a verb only a person
// may run. The binary issues one token at the session's start (`hook claude Press`, before the
// agent's first act); a verb run with `{ press, session }` on stdin is recorded as a person's
// (`via: press`) and prints the next token as its first line, so every token is spent once.
//
// What makes it a person's decision is where `press()` is reached from: an answer in the engine's
// own dialog, or a Button the person pressed in a focused pane, in this session. Never a hook's
// own logic, a timer, a feed line, a command handler, or an answer read back from a file (an
// answer kept from an earlier session is asked again, and pressed only on that answer). The
// gestures below are the only values `press()` accepts, and only dialog.ts (a dialog's answer, a
// pane Button's press) and allow.ts (the grant pane's `allow` Button) hold them, which
// scripts/check-plugin.mjs checks. The token travels on stdin, never on argv (which `ps` shows).
//
// No `$` crosses into this file: register.ts hands `guardPress` the function that calls
// `$.process.run`.
import type { ProcessRunInit } from 'claude-code'

import type { Run } from './bridge'

/**
 * The person-only verbs the mod may run, by head, each in the exact shapes `isPressAllowed` lists.
 * Kept apart from the bridge's `ALLOWED_VERBS`, which nothing but a press may widen with these.
 * `install`, `config`, `share`, `train`, `packs`, `init` and every `adage-exec` verb never.
 */
export const PRESS_VERBS: readonly string[] = ['review', 'delegate', 'on', 'add']

/** The review verbs a reflex row offers (`review.verbs`, `ui/routes.rs`). */
const REVIEW_VERBS: readonly string[] = ['approve', 'dismiss', 'strengthen', 'weaken', 'extinguish', 'pin']

const NAME = /^[A-Za-z0-9.][A-Za-z0-9._@/+-]*$/

/**
 * A track reference, a route id or a pack directory (`.adage/packs/acme`): a plain relative name,
 * never a flag, never a path out.
 */
const isName = (s: string | undefined): s is string => s !== undefined && NAME.test(s) && !s.includes('..') && !/^\.\/?$/.test(s) && s.length <= 200

/**
 * Whether `argv` (binary first) is a person-only verb the mod may press, in one of its shapes:
 * `review <verb> <track> <id>` (the six review verbs); `review delegate <track> <id> --level e2
 * --stage --json`, `… --level e2 --confirm`, `… --predict`; `delegate on`; `on`; `add <dir>
 * --trust` (a pack directory inside the repository: relative, no `..`).
 */
export function isPressAllowed(argv: readonly string[]): boolean {
  const [, verb, a, b, c, ...rest] = argv
  if (verb === undefined || !PRESS_VERBS.includes(verb)) return false
  switch (verb) {
    case 'on':
      return argv.length === 2
    case 'delegate':
      return argv.length === 3 && a === 'on'
    case 'add':
      return argv.length === 4 && isName(a) && !a.startsWith('/') && b === '--trust'
    case 'review': {
      if (a !== undefined && REVIEW_VERBS.includes(a)) return argv.length === 5 && isName(b) && isName(c)
      if (a !== 'delegate' || !isName(b) || !isName(c)) return false
      const tail = rest.join(' ')
      return tail === '--level e2 --stage --json' || tail === '--level e2 --confirm' || tail === '--predict'
    }
    default:
      return false
  }
}

/** A terminal command the binary printed (`adage review approve T R`) as the argv a press runs. */
export function pressArgv(cli: string, command: string): string[] | undefined {
  const words = command.trim().split(/\s+/)
  if (words[0] !== 'adage') return undefined
  const argv = [cli, ...words.slice(1)]
  return isPressAllowed(argv) ? argv : undefined
}

/**
 * Where a press comes from. Only these two values are accepted, so a press is always traceable to
 * a person in this session: a dialog answer, or a Button in a focused pane. There is no gesture
 * for an answer kept from an earlier session: `$.store` is a file the agent's tools can write, so
 * a kept answer is put to the person again (ux-design §9.12) and the press is that answer's.
 */
export type Gesture = { readonly from: 'answer' | 'button' }
export const ANSWER: Gesture = Object.freeze({ from: 'answer' as const })
export const BUTTON: Gesture = Object.freeze({ from: 'button' as const })
const GESTURES: readonly Gesture[] = [ANSWER, BUTTON]

/**
 * The guarded runner for presses: an argv off the press list is refused before any process
 * starts, the child always carries CLAUDECODE=1 (the binary checks the token, not the variable),
 * and the token must be on stdin.
 */
export function guardPress(run: Run): Run {
  return (argv, init = {}) => {
    if (!isPressAllowed(argv)) return Promise.reject(new Error(`adage press: argv not allowed: ${argv.slice(1).join(' ')}`))
    if (typeof init.stdin !== 'string' || !init.stdin.includes('"press"')) return Promise.reject(new Error('adage press: no press on stdin'))
    return run(argv, { ...init, env: { ...init.env, CLAUDECODE: '1' } })
  }
}

/** A token as the binary prints it: 64 lowercase hex characters. */
export const isToken = (v: unknown): v is string => typeof v === 'string' && /^[0-9a-f]{64}$/.test(v)

/** What a press came to. */
export type Pressed =
  /** The verb ran and succeeded; `next` is the token for the following press. */
  | { ok: true; next: string | undefined; lines: string[]; stdout: string }
  /**
   * It did not succeed: `refused` when the binary did not take the press (it printed no next
   * token: a wrong, spent or stale token, or the session is not the live one), else the verb
   * itself failed after the token was spent (`next` is then the token to keep).
   */
  | { ok: false; refused: boolean; next: string | undefined; error: string }

/** How long a person-only verb may take: a stage runs the step in the sandbox once. */
const PRESS_TIMEOUT_MS = 120_000

/**
 * Runs one person-only verb with the press on stdin and reads the binary's answer: the first
 * stdout line `{ "next": <token> }`, then the verb's own output (`--stage --json` and `--confirm`
 * print one JSON object on the second line).
 */
export async function press(
  gesture: Gesture,
  run: Run,
  argv: readonly string[],
  token: string,
  session: string,
  cwd: string,
  extra: Readonly<Record<string, unknown>> = {},
): Promise<Pressed> {
  if (!GESTURES.includes(gesture)) return { ok: false, refused: true, next: undefined, error: 'not a press' }
  if (!isToken(token) || session === '') return { ok: false, refused: true, next: undefined, error: 'no press is held' }
  try {
    const init: ProcessRunInit = { stdin: JSON.stringify({ ...extra, press: token, session }), timeoutMs: PRESS_TIMEOUT_MS }
    if (cwd !== '') init.cwd = cwd
    const ran = await run(argv, init)
    const [first = '', ...lines] = ran.stdout.split('\n')
    let next: string | undefined
    try {
      const v: unknown = JSON.parse(first)
      if (v !== null && typeof v === 'object' && isToken((v as { next?: unknown }).next)) next = (v as { next: string }).next
    } catch {
      // Not the next line: the binary did not take the press.
    }
    const rest = next === undefined ? [first, ...lines] : lines
    const error = ran.stderr.trim().split('\n').filter(l => l.trim() !== '').slice(-1)[0] ?? ''
    if (ran.exitCode === 0 && next !== undefined && !ran.isStdoutTruncated) return { ok: true, next, lines: rest.filter(l => l.trim() !== ''), stdout: rest.join('\n') }
    return { ok: false, refused: next === undefined, next, error: error.replace(/^error:\s*/i, '') }
  } catch (e) {
    return { ok: false, refused: false, next: undefined, error: e instanceof Error ? e.message : String(e) }
  }
}

/** The JSON object a stage or confirm printed after the next line, if there is one. */
export function objectOf(lines: readonly string[]): Record<string, unknown> | undefined {
  for (const l of lines) {
    try {
      const v: unknown = JSON.parse(l)
      if (v !== null && typeof v === 'object' && !Array.isArray(v)) return v as Record<string, unknown>
    } catch {
      // a line of the verb's prose
    }
  }
  return undefined
}

/**
 * The tamper signal (architecture §3.12 "Hold"): more `press_issued` records for this session than
 * tokens the binary handed the mod means something else claimed the window.
 */
export const tampered = (recordsSeen: number, issuedToMod: number): boolean => recordsSeen > issuedToMod
