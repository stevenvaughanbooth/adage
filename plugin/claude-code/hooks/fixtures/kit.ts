// The world beneath the plugin for the L7 tests (test-plan §6): a binary that answers by argv, a
// follower that prints fixture feed lines, the session's nouns, and the engine's own drawing of
// each site. Every `$.process.run` and `$.process.spawn` is recorded (the argv recorder, T-X-04).
// The lines and objects below are shaped as the binary prints them (copied from real runs of
// `adage watch --follow --json`, `graph --json`, `doctor --json` and `--json --for-session`).
import type { On, ProcessRunInit, RenderElement, RenderPropsOf } from 'claude-code'
import { mock } from 'claude-code/testing'
import type { MockClock } from 'claude-code/testing'

export const T0 = 1_791_008_381_000
export const SESSION = 'sess-1'
export const CWD = '/w'
export const DATA = '/data'
export const CLI = `${DATA}/bin/adage`
export const HOOK = `${DATA}/bin/hook`

const REPO = 'acme@315465905b6b'

// ------------------------------------------------------------------------------- feed lines

const at = (n: number) => T0 + 1000 + n
const decision = (n: number, session: string, event: string, acts: unknown[], texts: unknown[] = [], extra: Record<string, unknown> = {}) =>
  JSON.stringify({ kind: 'decision', cursor: `1~${at(n)}~0~${session}:0:${n}`, ts: at(n), session, harness: 'claude', event, repo: REPO, acts, observed: event === 'tools' ? 1 : 0, unrouted: false, texts, ...extra })

export const LINE = {
  learnerIdle: JSON.stringify({ kind: 'learner', cursor: '1~0~0~', repo: REPO, state: 'idle', next_at: null }),
  learnerWaiting: JSON.stringify({ kind: 'learner', cursor: '1~0~0~', repo: REPO, state: 'waiting', next_at: T0 + 38_000 }),
  learnerRunning: JSON.stringify({ kind: 'learner', cursor: '1~0~0~', repo: REPO, state: 'running', next_at: null }),
  healthy: JSON.stringify({ kind: 'health', cursor: '1~0~0~', lights: [{ label: 'hooks claude', lamp: 'ok', hint: null }, { label: '4 reflexes', lamp: 'ok', hint: null }, { label: 'auto-condition', lamp: 'ok', hint: null }, { label: 'bundled model', lamp: 'ok', hint: null }], state: { word: 'healthy', lamp: 'fire' }, off: false }),
  check: JSON.stringify({ kind: 'health', cursor: '1~0~0~', lights: [{ label: 'hooks claude', lamp: 'warn', hint: 'adage doctor' }], state: { word: 'check', lamp: 'prime', check: 'hooks: claude-code' }, off: false }),
  problem: JSON.stringify({ kind: 'health', cursor: '1~0~0~', lights: [{ label: 'hooks claude', lamp: 'fail', hint: 'adage doctor' }], state: { word: 'problem', lamp: 'inhibit', check: 'hook latency' }, off: false }),
  off: JSON.stringify({ kind: 'health', cursor: '1~0~0~', lights: [], state: { word: 'healthy', lamp: 'fire' }, off: true }),
  start: decision(1, SESSION, 'start', []),
  /** This session's prompt: `make gen` after a schema edit was told. */
  firedGen: decision(2, SESSION, 'tools', [{ act: 'directed', route: 'codegen-after-schema', strength: 'fire', detail: 'make gen', instinct: false }], [{ route: 'codegen-after-schema', text: 'In this repository, changes to schema/*.json must be followed by running:\n  make gen' }]),
  followedGen: decision(3, SESSION, 'tools', [{ act: 'followed', route: 'codegen-after-schema' }]),
  firedLookup: decision(4, SESSION, 'prompt', [{ act: 'directed', route: 'lookup-work-item', strength: 'fire', detail: 'scripts/wi-lookup W-2042 --json', instinct: false }], [{ route: 'lookup-work-item', text: 'Look the work item up first: scripts/wi-lookup W-2042 --json' }]),
  avoidUnittest: decision(5, SESSION, 'prompt', [{ act: 'directed', route: 'dead-end-unittest', strength: 'inhibit', detail: 'python3 -m unittest', instinct: false }], [{ route: 'dead-end-unittest', text: 'Do not run python3 -m unittest here; run make test.' }]),
  avoidedUnittest: decision(6, SESSION, 'tools', [{ act: 'followed', route: 'dead-end-unittest' }]),
  /** Another session's row in the control arm: shown as `adage watch` shows it. */
  otherControl: decision(7, 'sess-other', 'prompt', [{ act: 'withheld', route: 'lookup-work-item', strength: 'fire' }], [], { control: true }),
  /** This session's rows as the control arm spools them, before any scrubbing. */
  controlPrompt: decision(2, SESSION, 'prompt', [{ act: 'withheld', route: 'lookup-work-item', strength: 'fire' }], [{ route: 'lookup-work-item', text: 'withheld text' }], { control: true, would_have: ['lookup-work-item'] }),
  controlStep: decision(3, SESSION, 'step', [{ act: 'would_predict', route: 'codegen-after-schema' }], [], { control: true }),
  controlJudged: decision(4, SESSION, 'tools', [{ act: 'unaided', route: 'codegen-after-schema' }, { act: 'not_reached', route: 'lookup-work-item' }, { act: 'stop_withheld' }]),
  controlPretool: decision(5, SESSION, 'pretool', [{ act: 'pretool', rewrite: true, what: 'test', interpreter: '.venv/bin/python', withheld: true }]),
  /** The treated no-match rows the control rows stand beside (T-X-06). */
  quietPrompt: decision(2, SESSION, 'prompt', []),
  quietStep: decision(3, SESSION, 'step', []),
  quietTools: decision(4, SESSION, 'tools', []),
  quietPretool: decision(5, SESSION, 'pretool', []),
  learnNew: (ts: number, sessions?: string[]) => JSON.stringify({ kind: 'learn', cursor: '1~0~9~', ts, repo: REPO, line: { ts, kind: 'condition', trigger: 'auto', repo: REPO, sessions_used: 3, changes: [{ change: 'new', route: 'test-after-acme-edit', track: 'acme/learned', summary: 'new prime reflex', from: null, to: 'prime', ...(sessions === undefined ? {} : { sessions }) }] }, ticks: [{ text: 'acme/learned: test-after-acme-edit new · prime', short: 'x', tone: 'change', strength: 'prime', repo: REPO }] }),
  learnStrength: (ts: number) => JSON.stringify({ kind: 'learn', cursor: '1~0~9~', ts, repo: REPO, line: { ts, kind: 'condition', trigger: 'auto', repo: REPO, changes: [{ change: 'strength', route: 'codegen-after-schema', track: 'acme/learned', summary: 'prime → fire', from: 'prime', to: 'fire' }] }, ticks: [] }),
  learnEvidence: (ts: number) => JSON.stringify({ kind: 'learn', cursor: '1~0~9~', ts, repo: REPO, line: { ts, kind: 'condition', trigger: 'auto', repo: REPO, changes: [{ change: 'evidence', route: 'codegen-after-schema', track: 'acme/learned', summary: 'support 9/9', from: 'fire', to: 'fire' }] }, ticks: [] }),
  learnFailed: (ts: number) => JSON.stringify({ kind: 'learn', cursor: '1~0~9~', ts, repo: REPO, line: { ts, kind: 'condition_error', trigger: 'auto', repo: REPO, class: 'sandbox', error: 'x' }, ticks: [{ text: 'condition failed (sandbox)', short: 'x', tone: 'error', strength: null, repo: REPO }] }),
} as const

// --------------------------------------------------------------------------- the binary's reads

const route = (id: string, over: Record<string, unknown>) => ({
  id,
  track: 'acme/learned',
  reference: 'acme/learned',
  repo: REPO,
  imported: false,
  instinct: false,
  origin: { kind: 'learned', label: null },
  summary: `${id} summary`,
  strength: 'fire',
  state: 'firm',
  trigger: { kind: 'after_edit', label: 'an edit touches schema/*.json' },
  step: { kind: 'run', label: 'make gen' },
  directive: 'In this repository, changes to schema/*.json must be followed by running: make gen',
  evidence: { support: 13, opportunities: 17, lower_bound: 0.53, describe: '13 of 17 sessions ran it after the edit', uplift: { line: 'uplift +0.30' } },
  review: { verbs: ['strengthen', 'pin'], reasons: [], next: null },
  // What a route also carries and the pane must never draw (ux-design §1.7).
  arms: { control: { sessions: 3, applied: 2 }, treated: { sessions: 9, applied: 8 } },
  live: { would_have: 2, control: { unaided: 1, not_reached: 1, obligation_met: 0 } },
  ...over,
})

export const GRAPH = {
  routes: [
    route('codegen-after-schema', { gist: 'run make gen after a schema edit' }),
    route('lookup-work-item', { gist: 'look up the work item with scripts/wi-lookup', strength: 'fire', trigger: { kind: 'prompt', label: 'a prompt names a work item' }, step: { kind: 'run', label: 'scripts/wi-lookup {id} --json' } }),
    route('dead-end-unittest', { gist: 'avoid python3 -m unittest here; use make test', strength: 'inhibit', review: { verbs: ['weaken'], reasons: [] } }),
    route('test-after-acme-edit', { gist: 'run make test after editing acme/**', strength: 'prime', state: 'nascent', review: { verbs: ['approve', 'dismiss'], reasons: [{ kind: 'approval', why: 'x' }] } }),
    route('finish-line-e7e809', { gist: 'after editing schema/*.json, run make gen', instinct: true, strength: 'prime', state: 'forming', reference: 'acme/instincts', track: 'acme/instincts', review: { verbs: ['approve', 'extinguish', 'pin'], reasons: [{ kind: 'instinct', why: 'x' }] } }),
  ],
  events: [{ ts: T0, session: SESSION, arm: 'control' }],
}

export const DOCTOR = {
  checks: [
    { check: 'binary', label: 'binary', status: 'pass', detail: 'adage 0.1.0' },
    { check: 'index', label: 'index', status: 'warn', detail: 'stale: tracks changed since it was compiled', fix: 'run `adage condition` to recompile it', action: { kind: 'run', command: 'adage condition' } },
    { check: 'hooks: claude-code', label: 'hooks: claude-code', status: 'warn', detail: 'not registered', fix: '`adage install claude-code --write` (this repository) or add `--scope user`', action: { kind: 'run', command: 'adage install claude-code --write' } },
    { check: 'model', label: 'embedding model', status: 'fail', detail: 'missing', fix: 'reinstall the plugin', action: { kind: 'manual' } },
  ],
}

export const CARD = {
  version: '0.1.0',
  header: 'adage 0.1.0 · acme@315465905b6b · ● healthy',
  repo: REPO,
  connected: true,
  off: false,
  health: { word: 'healthy', lamp: 'fire', check: null },
  rows: [
    { key: 'reflexes', text: '4 active: 3 conditioned, 1 instinct' },
    { key: 'activity', text: 'last 14 days: 9 sessions · 12 directives · 10 followed · 1 dead end headed off' },
    { key: 'savings', text: 'measured: 2.1 fewer model requests per session (1.2–3.0)' },
    { key: '', text: 'over 30 treated and 10 control sessions' },
    { key: 'embeddings', text: 'potion-base-8m-i8 (bundled)' },
  ],
  next: [
    { command: '/adage watch', why: 'live rows, reflexes and health' },
    { command: 'adage map', why: 'explore reflexes in the browser' },
    { command: 'adage doctor', why: 'check binary, data, hooks and model' },
  ],
}

export const DELEGATE = { enabled: true, predict: 'on', grants: {}, registrations: [], duplicates: [], lapsed: [] as unknown[] }
export const LAPSED = [{ route: 'acme/learned/codegen-after-schema', level: 'e2', mode: 'predict', changed: ['argv_templates'], reapprove: 'adage review delegate acme/learned codegen-after-schema --predict' }]
export const STATUS = { tracks: [], candidates: [] as unknown[] }

// ----------------------------------------------------------------------------------- the world

export type Ran = { argv: readonly string[]; init?: ProcessRunInit }

export type WorldOptions = {
  /** CLAUDE_PLUGIN_DATA (unset when undefined). */
  data?: string
  /** What `<bin> version` prints ('' is scripts/quiet). */
  version?: string
  /** `$DATA/status`, the shim's reason, when no binary serves. */
  status?: string
  /** The follower's lines (each child prints them all, then waits). */
  feed?: readonly string[]
  /** Lines the resumed child prints once the test calls `deliver()`. */
  later?: readonly string[]
  /** Keeps the first child up (a live follower) instead of letting it end. */
  hold?: boolean
  /** What the command hooks beneath answer, by classic event. */
  classic?: Record<string, unknown>
  /** What the PreToolUse command hook answers for a Bash call. */
  pretool?: unknown
  /** What `hook claude <Event>` prints, by event. */
  hook?: Record<string, string>
  graph?: unknown
  doctor?: unknown
  card?: unknown
  delegate?: unknown
  status_json?: unknown
  /** Whether `$.ui.copy` takes. */
  copies?: boolean
  env?: Record<string, string>
  store?: Record<string, unknown>
  harness?: string
  /** The session's id now (a /clear changes it); `sess-1` otherwise. */
  session?: () => string
  /** A run that fails (exit 1) for every head in the list. */
  failing?: readonly string[]
}

export type World = {
  ran: Ran[]
  spawned: (readonly string[])[]
  opened: unknown[]
  toasts: string[]
  statuses: (string | undefined)[]
  copies: string[]
  registered: unknown[]
  invalidated: number
  requests: number
  clock: MockClock
  /** Lets the resumed child print the `later` lines. */
  deliver: () => Promise<void>
}

/** Installs the world beneath the plugin on `on`. */
export function world(on: On, o: WorldOptions = {}): World {
  const w: World = { ran: [], spawned: [], opened: [], toasts: [], statuses: [], copies: [], registered: [], invalidated: 0, requests: 0, clock: mock.clock(on, { now: T0 }), deliver: async () => undefined }
  mock.env(on, { ...(o.data === undefined && 'data' in o ? {} : { CLAUDE_PLUGIN_DATA: o.data ?? DATA }), ...o.env })
  mock.store(on, o.store ?? {})
  const json = (v: unknown) => JSON.stringify(v)
  on('process.run', ($, e) => {
    w.ran.push({ argv: e.argv, init: e.init })
    const head = e.argv[1] === '--json' ? 'card' : (e.argv[1] ?? '')
    if (o.failing?.includes(head)) return { value: { exitCode: 1, stdout: '', stderr: 'x', isStdoutTruncated: false, isStderrTruncated: false } }
    const out =
      head === 'version' ? (o.version ?? 'adage 0.1.0\n')
      : head === 'hook' ? (o.hook?.[e.argv[3] ?? ''] ?? '')
      : head === 'graph' ? json(o.graph ?? GRAPH)
      : head === 'doctor' ? json(o.doctor ?? DOCTOR)
      : head === 'card' ? json(o.card ?? CARD)
      : head === 'delegate' ? json(o.delegate ?? DELEGATE)
      : head === 'status' ? json(o.status_json ?? STATUS)
      : head === 'off' ? 'adage is off for this repository (.adage/disabled). adage on turns it back on.\n'
      : head === 'condition' ? 'adage condition · 0.4 s\n'
      : ''
    return { value: { exitCode: 0, stdout: out, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  // The follower: a fresh child prints the feed lines and ends (a resumed one, `--since`, has
  // nothing new), so the mod's reconnect loop runs and settles; `hold` keeps the first child up,
  // as a live session's is; `later` lines come from the resumed child once the test calls
  // `deliver()`. The map prints its address and serves until the test ends.
  let release: () => void = () => undefined
  const gate = new Promise<void>(r => (release = r))
  let delivered = false
  w.deliver = async () => {
    release()
    await w.clock.settle()
  }
  on('process.spawn', async function* ($, e) {
    w.spawned.push(e.argv)
    if (e.argv[1] === 'map') {
      yield { stream: 'stdout' as const, text: 'map  http://127.0.0.1:43123/ · read-only · this machine only\n' }
      await w.clock.sleep(1e9)
      return { value: { code: 0, signal: null } }
    }
    if (!e.argv.includes('--since')) {
      const lines = o.feed ?? []
      if (lines.length > 0) yield { stream: 'stdout' as const, text: lines.join('\n') + '\n' }
      if (o.hold) await w.clock.sleep(1e9)
      return { value: { code: 0, signal: null } }
    }
    if (o.later !== undefined && !delivered) {
      await gate
      delivered = true
      yield { stream: 'stdout' as const, text: o.later.join('\n') + '\n' }
    }
    return { value: { code: 0, signal: null } }
  })
  on('fs.read', ($, e) => (o.status === undefined ? { deny: 'ENOENT' } : { value: `${o.status}\n` }))
  on('session.id', () => ({ value: o.session?.() ?? SESSION }))
  on('session.cwd', () => ({ value: CWD }))
  on('session.version', () => ({ value: { version: o.harness ?? '2.1.288' } }))
  on('session.start', ($, e) => ({ cwd: e.cwd }))
  on('command.register', ($, e) => {
    w.registered.push(e)
    return { value: { command: 'adage' } }
  })
  on('ui.open', ($, e) => {
    w.opened.push(e)
    return { value: { isPlaced: true } }
  })
  on('ui.toast', ($, e) => {
    w.toasts.push(e.text)
    return { value: undefined }
  })
  on('ui.status', ($, e) => {
    w.statuses.push(e.text)
    return { value: undefined }
  })
  on('ui.copy', ($, e) => {
    w.copies.push(e.text)
    return { value: o.copies === false ? { isCopied: false, reason: 'no-clipboard' } : { isCopied: true } }
  })
  on('ui.invalidate', () => {
    w.invalidated++
    return { value: undefined }
  })
  on('session.append', ($, e) => ({ message: e.message, uuid: e.uuid }))
  for (const event of ['SessionStart', 'UserPromptSubmit', 'PostToolBatch', 'PostToolUseFailure', 'Stop'] as const) {
    on(`classic.${event}`, () => {
      const a = o.classic?.[event]
      return (typeof a === 'function' ? a() : (a ?? {})) as never
    })
  }
  on('classic.PreToolUse', () => (o.pretool ?? {}) as never)
  on('tool.call', () => ({ result: { stdout: 'ok', stderr: '', interrupted: false } }) as never)
  // The model: one text response that ends the turn (counted).
  on('turn.step', async function* ($, e) {
    w.requests++
    const usage = { input_tokens: 1, output_tokens: 1, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, model: 'm' }
    yield { kind: 'text' as const, index: 0, text: 'Done.' }
    yield { kind: 'stop' as const, stopReason: 'tool_use' as const, usage }
    return { turnId: e.turnId, index: e.index, answer: 'Done.', toolUses: [], stopReason: 'tool_use' as const, usage }
  })
  // The engine's own drawing of every site: one Text naming it (and its text, where it has one).
  on('ui.render', ($, e) => {
    const { Text } = $.ui.resolve(e)
    const props = e.props as Record<string, unknown>
    const text = typeof props.text === 'string' ? props.text : typeof props.word === 'string' ? `${props.word}${typeof props.suffix === 'string' ? props.suffix : ''}` : ''
    return h(Text, {}, `ENGINE ${e.component}${text === '' ? '' : `: ${text}`}`) as RenderElement
  })
  return w
}

/** Starts an interactive session on `surface` and lets the feed's lines land. */
export async function start($: { session: { start: (e: { cwd: string; surface: 'terminal' | 'desktop' | null; isInteractive: boolean }) => Promise<unknown> } }, w: World, surface: 'terminal' | 'desktop' | null = 'terminal', isInteractive = true): Promise<void> {
  await $.session.start({ cwd: CWD, surface, isInteractive })
  await w.clock.settle()
}

// --------------------------------------------------------------------------------- site props

export const SURFACES = ['terminal', 'desktop'] as const
export type Surface = (typeof SURFACES)[number]

export const BAND = (bodyColumns = 120): RenderPropsOf['AbovePrompt'] =>
  ({ hasSurvey: false, isWorking: false, maxRows: 6, bodyColumns, scroll: { offset: 0, bodyRows: 5 }, view: {} }) as unknown as RenderPropsOf['AbovePrompt']
export const PANE_PROPS = (bodyColumns = 120): RenderPropsOf['Pane'] =>
  ({ title: 'adage', isFocused: true, bodyColumns, placement: 'dock', scroll: { offset: 0, bodyRows: 20 }, view: {} }) as unknown as RenderPropsOf['Pane']
export const USER = (text: string, isExpanded = false): RenderPropsOf['UserMessage'] =>
  ({ text, origin: { kind: 'composer' }, isExpanded }) as unknown as RenderPropsOf['UserMessage']
export const TOOL = (id: string, command = 'make gen'): RenderPropsOf['ToolUse'] =>
  ({ tool_use_id: id, tool: 'Bash', input: { command }, isRunning: false, isErrored: false, isInterrupted: false }) as RenderPropsOf['ToolUse']
export const RESULT = (id: string): RenderPropsOf['ToolResult'] => ({ tool_use_id: id, tool: 'Bash', output: { stdout: '48 passed' }, isErrored: false }) as RenderPropsOf['ToolResult']
export const OUTPUT = (args: string, text: string): RenderPropsOf['CommandOutput'] => ({ command: 'adage', args, text, isErrored: false }) as RenderPropsOf['CommandOutput']

/** A `<adage-reflex>` block as a command hook delivers it. */
export const block = (route: string, strength = 'fire') =>
  `<adage-reflex track="acme/learned@1" route="${route}" strength="${strength}" kind="run">\nDo it.\n</adage-reflex>`

/** A predicted step, as `hook claude TurnStep` answers it. */
export const PREDICT = JSON.stringify({ predict: { kind: 'obligation', route: 'codegen-after-schema', track: 'acme/learned@1', calls: [{ tool: 'Bash', input: { command: 'make gen' } }] } })
export const PREDICTED_ID = 'adage_codegen-after-schema_turn-1_2_0'
export const STEP = { turnId: 'turn-1', index: 2, model: 'm', messageCount: 4 }

/** The words a default surface never carries (ux-design §10, T-B2-10). */
export const BANNED = /\b(fire|prime|inhibit|forming|firm|dormant|approval|saved)\b/i
