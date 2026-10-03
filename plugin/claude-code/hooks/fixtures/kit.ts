// The world beneath the plugin for the L7 tests (test-plan §6): a binary that answers by argv, a
// follower that prints fixture feed lines, the session's nouns, and the engine's own drawing of
// each site. Every `$.process.run` and `$.process.spawn` is recorded (the argv recorder, T-X-04),
// and so is every ask (`$.ui.ask` is a `tool.call` of `AskUserQuestion`, answered here). The lines
// and objects below are shaped as the binary prints them (copied from real runs of `adage watch
// --follow --json`, `graph --json`, `doctor --json`, `--json --for-session`, `status --json`,
// `review delegate … --stage --json` and `hook claude SkillPrompt`).
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

/** The press tokens the binary issues and chains (64 lowercase hex each). */
export const TOKEN = 'a'.repeat(64)
export const NEXT_TOKEN = 'b'.repeat(64)
export const THIRD_TOKEN = 'c'.repeat(64)

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
  /** The press the binary issued at this session's start, as the spool's `press_issued` record reads in the feed. */
  pressIssued: decision(0, SESSION, 'press_issued', []),
  /** A second issue for the same start: something else claimed the window (the tamper signal). */
  pressIssuedAgain: decision(8, SESSION, 'press_issued', []),
  /** A tool batch that completed a candidate's context (plan D2): noticed once, in both arms. */
  noticed: (n: number, candidate: string, ids: string[], over: Record<string, unknown> = {}) =>
    decision(n, SESSION, 'tools', [{ act: 'noticed', candidate, support: 2, s_min: 3, review: false, ...over }], [], { tool_use_ids: ids }),
  learnNew: (ts: number, sessions?: string[]) => JSON.stringify({ kind: 'learn', cursor: '1~0~9~', ts, repo: REPO, line: { ts, kind: 'condition', trigger: 'auto', repo: REPO, sessions_used: 3, changes: [{ change: 'new', route: 'test-after-acme-edit', track: 'acme/learned', summary: 'new prime reflex', from: null, to: 'prime', ...(sessions === undefined ? {} : { sessions }) }] }, ticks: [{ text: 'acme/learned: test-after-acme-edit new · prime', short: 'x', tone: 'change', strength: 'prime', repo: REPO }] }),
  learnStrength: (ts: number, sessions?: string[]) => JSON.stringify({ kind: 'learn', cursor: '1~0~9~', ts, repo: REPO, line: { ts, kind: 'condition', trigger: 'auto', repo: REPO, changes: [{ change: 'strength', route: 'codegen-after-schema', track: 'acme/learned', summary: 'prime → fire', from: 'prime', to: 'fire', ...(sessions === undefined ? {} : { sessions }) }] }, ticks: [] }),
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

/** The graph with an execution offer on the codegen reflex (`review.offers`, plan A7): the two-act ask. */
export const OFFER = { kind: 'execute', command: 'adage review delegate acme/learned codegen-after-schema --level e2 --stage --json', confirm: 'adage review delegate acme/learned codegen-after-schema --level e2 --confirm' }
export const GRAPH_OFFER = { ...GRAPH, routes: [route('codegen-after-schema', { gist: 'run make gen after a schema edit', review: { verbs: ['strengthen', 'pin'], reasons: [], offers: [OFFER] } }), ...GRAPH.routes.slice(1, 3)] }
/** The graph with a prediction offer: one act. */
export const GRAPH_PREDICT = { ...GRAPH, routes: [route('codegen-after-schema', { gist: 'run make gen after a schema edit', review: { verbs: ['strengthen', 'pin'], reasons: [], offers: [{ kind: 'predict', command: 'adage review delegate acme/learned codegen-after-schema --predict' }] } }), ...GRAPH.routes.slice(1, 3)] }

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

/** A candidate as `adage status --json` lists it (plan D1): a sequence (a context of two events) not yet a reflex. */
export const CANDIDATE = { id: 'run-a54d7c', repo: 'acme', track: 'acme/learned', gist: 'after make test, run make lint', summary: 'run make lint after make test', state: 'candidate', directive: null, trigger: { kind: 'context', label: 'a context of 2 events' }, step: { kind: 'run', label: 'make lint' }, candidate: { support: 2, s_min: 3, days: 2, last_seen: '2026-09-09', review: false }, next: null }
/** A candidate waiting for review: the learned track's own nascent route with a context trigger. */
export const CANDIDATE_REVIEW = { ...CANDIDATE, id: 'lookup-then-read', gist: 'scripts/wi-lookup then reading the files it names', state: 'nascent', candidate: { support: 4, s_min: 5, days: 3, last_seen: '2026-09-30', review: true }, next: 'adage review approve acme/learned lookup-then-read' }
/** A one-event record (should not exist): never drawn. */
export const CANDIDATE_SINGLE = { ...CANDIDATE, id: 'run-single', gist: 'run make lint', trigger: { kind: 'after_edit', label: 'an edit touches acme/**' } }
export const STATUS = { tracks: [], candidates: [] as unknown[] }
export const PACKS = [{ name: 'alice@acme.com', path: '.adage/packs/acme', command: 'adage add .adage/packs/acme --trust' }]

/** What `review delegate … --level e2 --stage --json` prints after the `next` line (plan A7). */
export const STAGE = {
  route: 'acme/learned/codegen-after-schema',
  repo: REPO,
  level: 'e2',
  terms: { outputs: ['acme/generated/**'], cache: ['__pycache__/**'] },
  proposed: true,
  first: true,
  ok: true,
  digest: 'd1'.repeat(32) + '.' + Array.from({ length: 11 }, (_, k) => String(k).repeat(16)).join('.'),
  recorded: false,
  facts: {
    argv: ['make', 'gen'],
    inputs: [{ slot: 'path', contract: 'schema/*.json', from: 'the edited path, a repository path' }],
    effect: { approved: 'regenerate', classified: 'regenerate' },
    output: { mode: 'files', fields: [] },
    exit_when: {},
    scripts: [{ path: 'scripts/gen.sh', sha256: 'e3b0c442', text: '#!/bin/sh\npython -m acme.gen\n' }],
    sandbox: 'Linux, Landlock ABI 7 + seccomp; network off',
    profile_id: 'p9f8e7d6',
    route_exec_digest: 'r1a2b3c4',
    evidence: { support: 13, opportunities: 17, days: 9 },
    attestations: [],
    action: { kind: 'regenerate', outputs: ['acme/generated/**'], cache: ['__pycache__/**'] },
    staged: {
      backend: 'landlock',
      exit: 0,
      diff: [{ path: 'acme/generated/models.py', change: 'modify', before_sha256: 'aa', after_sha256: 'bb', diff: '@@ -10,3 +10,4 @@\n class Invoice:\n     id: str\n+    currency: str = "USD"\n     total: int\n' }],
      refused: [],
      discarded: 0,
    },
  },
}
/** The same stage after the agent kept working: the diff moved, nothing recorded. */
export const STAGE_MOVED = { ...STAGE, changed: 'snapshot', digest: 'e2'.repeat(32) + STAGE.digest.slice(64), facts: { ...STAGE.facts, staged: { ...STAGE.facts.staged, diff: [{ ...STAGE.facts.staged.diff[0], after_sha256: 'cc', diff: '@@ -10,3 +10,5 @@\n class Invoice:\n     id: str\n+    currency: str = "USD"\n+    locale: str = "en"\n     total: int\n' }] } } }
export const CONFIRMED = { ...STAGE, recorded: true, wrote: ['policy', 'marker', 'grant'], takes_effect: 'in new sessions' }

/** What `hook claude SkillPrompt` answers for a skill `adage train` verified here (plan D4). */
export const SKILL_TEXT = 'Look the work item up, then run the tests.'
export const SKILL_ANNOTATED = `${SKILL_TEXT}\n\nAdage verified these steps in this repository on 2026-10-03:\n- scripts/wi-lookup {id}: works\n- pytest: stale (exit 1: command not found)\n- make test: works`

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
  /** What `hook claude <Event>` prints, by event (`Press` answers a token unless `press` is false). */
  hook?: Record<string, string>
  /** Whether `hook claude Press` issues a token (the window is open) at each start; true by default. */
  press?: boolean
  /** A person-only verb refused by the binary (no `next` line, exit 1): a wrong or spent press. */
  refuse?: boolean
  /** What `--stage --json` answers (a function sees each call), and what `--confirm` answers. */
  stage?: unknown | (() => unknown)
  confirm?: unknown | (() => unknown)
  graph?: unknown
  doctor?: unknown
  card?: unknown
  delegate?: unknown
  status_json?: unknown
  /** The answer to each `$.ui.ask` (the label, or undefined for a dismissed dialog). */
  ask?: (question: string, options: readonly string[]) => string | undefined
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
  closed: string[]
  toasts: string[]
  statuses: (string | undefined)[]
  copies: string[]
  registered: unknown[]
  /** Every `$.ui.ask`: the question and its labels, in order. */
  asked: { question: string; options: string[] }[]
  invalidated: number
  requests: number
  clock: MockClock
  /** What the store holds now (`$.store`). */
  store: Record<string, unknown>
  /** Lets the resumed child print the `later` lines. */
  deliver: () => Promise<void>
}

/** The verbs only a press runs, by their head: the kit answers them as the binary does after a spend. */
const PERSON_ONLY = new Set(['review', 'on', 'add', 'delegate'])

/** Installs the world beneath the plugin on `on`. */
export function world(on: On, o: WorldOptions = {}): World {
  const store: Record<string, unknown> = { ...(o.store ?? {}) }
  const w: World = { ran: [], spawned: [], opened: [], closed: [], toasts: [], statuses: [], copies: [], registered: [], asked: [], invalidated: 0, requests: 0, clock: mock.clock(on, { now: T0 }), store, deliver: async () => undefined }
  mock.env(on, { ...(o.data === undefined && 'data' in o ? {} : { CLAUDE_PLUGIN_DATA: o.data ?? DATA }), ...o.env })
  on('store.get', ($, e) => ({ value: store[e.key] }))
  on('store.set', ($, e) => {
    store[e.key] = e.value
    return { value: undefined }
  })
  const json = (v: unknown) => JSON.stringify(v)
  const call = (v: unknown) => (typeof v === 'function' ? (v as () => unknown)() : v)
  // The chain of tokens the binary prints as each press is spent.
  const chain = [NEXT_TOKEN, THIRD_TOKEN]
  let spent = 0
  on('process.run', ($, e) => {
    w.ran.push({ argv: e.argv, init: e.init })
    const head = e.argv[1] === '--json' ? 'card' : (e.argv[1] ?? '')
    if (o.failing?.includes(head)) return { value: { exitCode: 1, stdout: '', stderr: 'x', isStdoutTruncated: false, isStderrTruncated: false } }
    if (head === 'hook' && e.argv[3] === 'Press') {
      const out = o.hook?.Press ?? (o.press === false ? '' : json({ press: TOKEN }) + '\n')
      return { value: { exitCode: 0, stdout: out, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
    }
    if (PERSON_ONLY.has(head) && !(head === 'delegate' && e.argv[2] === 'status')) {
      // A person-only verb: the binary spends the press on stdin and prints the next token first.
      if (o.refuse === true) return { value: { exitCode: 1, stdout: '', stderr: 'error: adage review is a decision for a person: run it at a terminal\n', isStdoutTruncated: false, isStderrTruncated: false } }
      const next = chain[Math.min(spent++, chain.length - 1)] ?? THIRD_TOKEN
      const tail = e.argv.join(' ')
      const body =
        tail.endsWith('--stage --json') ? json(call(o.stage ?? STAGE))
        : tail.endsWith('--confirm') ? json(call(o.confirm ?? CONFIRMED))
        : head === 'on' ? 'adage is on for this repository.'
        : head === 'add' ? 'added 6 reflexes from alice@acme.com'
        : `recorded ${e.argv[2] ?? ''} for ${e.argv[4] ?? ''}`
      return { value: { exitCode: 0, stdout: `${json({ next })}\n${body}\n`, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
    }
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
  on('ui.close', ($, e) => {
    w.closed.push(e.id)
    return { value: undefined }
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
  // `$.ui.ask` is a `tool.call` of AskUserQuestion: answered with the label the test chose, or
  // refused (a dismissed dialog); every other tool runs as `ok`.
  on('tool.call', ($, e) => {
    if (e.tool === 'AskUserQuestion') {
      const q = (e as unknown as { questions?: { question: string; options: { label: string }[] }[] }).questions?.[0]
      const question = q?.question ?? ''
      const options = (q?.options ?? []).map(x => x.label)
      w.asked.push({ question, options })
      const answer = o.ask?.(question, options)
      if (answer === undefined) return { deny: 'dismissed' } as never
      return { result: { questions: [q], answers: { [question]: answer } } } as never
    }
    return { result: { stdout: 'ok', stderr: '', interrupted: false } } as never
  })
  // The model: one text response that ends the turn (counted).
  on('turn.step', async function* ($, e) {
    w.requests++
    const usage = { input_tokens: 1, output_tokens: 1, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, model: 'm' }
    yield { kind: 'text' as const, index: 0, text: 'Done.' }
    yield { kind: 'stop' as const, stopReason: 'tool_use' as const, usage }
    return { turnId: e.turnId, index: e.index, answer: 'Done.', toolUses: [], stopReason: 'tool_use' as const, usage }
  })
  on('turn.complete', ($, e) => ({ text: e.answer }))
  on('skill.prompt', ($, e) => ({ text: e.text }))
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

/** The end of a turn the model answered (`turn.complete`), as the engine raises it on the main loop. */
export const TURN_END = { turnId: 'turn-1', answer: 'Done.', durationMs: 1000, isAborted: false, reason: 'answer' } as const

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
/** A Skill tool row (`Skill(acme-work-item)`). */
export const SKILL_TOOL = (id: string, skill = 'acme-work-item'): RenderPropsOf['ToolUse'] =>
  ({ tool_use_id: id, tool: 'Skill', input: { skill }, isRunning: false, isErrored: false, isInterrupted: false }) as RenderPropsOf['ToolUse']
/** A folded run of tool calls (`ToolGroup`), by their ids. */
export const GROUP = (ids: readonly string[]): RenderPropsOf['ToolGroup'] =>
  ({ calls: ids.map(id => ({ tool_use_id: id, tool: 'Bash', input: { command: 'make test' }, isRunning: false, isErrored: false })), isActive: false, isExpanded: false }) as unknown as RenderPropsOf['ToolGroup']
export const RESULT = (id: string): RenderPropsOf['ToolResult'] => ({ tool_use_id: id, tool: 'Bash', output: { stdout: '48 passed' }, isErrored: false }) as RenderPropsOf['ToolResult']
export const OUTPUT = (args: string, text: string): RenderPropsOf['CommandOutput'] => ({ command: 'adage', args, text, isErrored: false }) as RenderPropsOf['CommandOutput']

/** A `<adage-reflex>` block as a command hook delivers it. */
export const block = (route: string, strength = 'fire') =>
  `<adage-reflex track="acme/learned@1" route="${route}" strength="${strength}" kind="run">\nDo it.\n</adage-reflex>`

/** A predicted step, as `hook claude TurnStep` answers it. */
export const PREDICT = JSON.stringify({ predict: { kind: 'obligation', route: 'codegen-after-schema', track: 'acme/learned@1', calls: [{ tool: 'Bash', input: { command: 'make gen' } }] } })
export const PREDICTED_ID = 'adage_codegen-after-schema_turn-1_2_0'
/** A predicted skill (plan D3): the Skill tool in the model's place. */
export const PREDICT_SKILL = JSON.stringify({ predict: { kind: 'skill', route: 'release-after-bump', track: 'acme/learned@1', calls: [{ tool: 'Skill', input: { skill: 'acme-work-item' } }] } })
export const PREDICTED_SKILL_ID = 'adage_release-after-bump_turn-1_2_0'
export const STEP = { turnId: 'turn-1', index: 2, model: 'm', messageCount: 4 }

/** The words a default surface never carries (ux-design §10, T-B2-10). */
export const BANNED = /\b(fire|prime|inhibit|forming|firm|dormant|approval|saved)\b/i
