// The adage mod (plan B1–B3, the C4/C6 hooks). The plugin ships both binaries and one copy of
// this module (A8): at session.start it probes the hook binary the shim linked (adage-exec while
// execution is on, else adage), and while none is there (the link is scripts/quiet) it stays
// inert: every hook answers what next(e) answered, unchanged, and nothing is drawn but the band's
// one line saying why (ux-design §9.1).
//
// Every `$` call of the mod is in this file: `claude plugin validate` follows `$` only into
// functions declared in the same file, and only hooks written inline at `on(...)`, so the other
// files take plain values and closures and are pure enough to test without the engine. What is
// drawn is a function of `$.state` (draw/*.tsx over view.ts); what the binary says reaches
// `$.state` from the classic observers, the feed (feed.ts) and the `--json` reads (bridge.ts).
import { atom, read, update } from 'claude-code'
import type { EngineInterface, ProcessSpawnRequest, Register, RenderSurface, Timer } from 'claude-code'

import { ask, cliBinary, dataDir, guard, guardSpawn, hookBinary, probe, read as readJson, say } from './bridge'
import type { Asked, Run, Spawn } from './bridge'
import { ARGUMENT_HINT, DESCRIPTION, runCommand, verbOf } from './commands'
import { bandTree } from './draw/band'
import { cardTree, whyTree } from './draw/card'
import type { Els } from './draw/els'
import { inspectTree, paneTree } from './draw/pane'
import type { PaneActions } from './draw/pane'
import { beside, receipt, under } from './draw/rows'
import { fold, follow, followArgs, parseLine, stem } from './feed'
import type { Line } from './feed'
import { COMMAND, FEED0, INSPECT, MAP0, MODE0, PANE, TESTED, TURN0, VIEW0 } from './plugin'
import type { Fired, Learner, Mark, Predicted, Row, View } from './plugin'
import { parseReflexes } from './reflex'
import { TURN_STEP_TIMEOUT_MS, turnStep } from './step'
import { PRE_TOOL_TIMEOUT_MS, substituteCall } from './substitute'
import {
  band,
  cardOf,
  checksOf,
  firedThisSession,
  inspectText,
  lapseToast,
  lapsedOf,
  learnToast,
  mapUrl,
  markText,
  openNotice,
  packsOf,
  promptLines,
  receiptBeside,
  receiptCard,
  receiptOf,
  reflexOf,
  reflexesOf,
  rewriteOf,
  spinnerSuffix,
  turnSummary,
  versionLine,
  visibleRows,
  waiting,
  whyText,
} from './view'
import type { RowMark } from './view'

// The $.state values the mod keeps (types/index.d.ts). `claude plugin validate` reads a state
// reference only as literals in a const of this file, so the plugin name is spelled here.
const FIRED = atom({ plugin: 'adage', key: 'fired' } as const, [])
const PREDICTED = atom({ plugin: 'adage', key: 'predicted' } as const, [])
const MARKS = atom({ plugin: 'adage', key: 'marks' } as const, [])
const BATCHES = atom({ plugin: 'adage', key: 'batches' } as const, [])
const FEED = atom({ plugin: 'adage', key: 'feed' } as const, FEED0)
const ROWS = atom({ plugin: 'adage', key: 'rows' } as const, [])
const TICKS = atom({ plugin: 'adage', key: 'ticks' } as const, [])
const HEALTH = atom({ plugin: 'adage', key: 'health' } as const, null)
const LEARNER = atom({ plugin: 'adage', key: 'learner' } as const, null)
const REFLEXES = atom({ plugin: 'adage', key: 'reflexes' } as const, [])
const CHECKS = atom({ plugin: 'adage', key: 'checks' } as const, [])
const LAPSED = atom({ plugin: 'adage', key: 'lapsed' } as const, [])
const PACKS = atom({ plugin: 'adage', key: 'packs' } as const, [])
const MODE = atom({ plugin: 'adage', key: 'mode' } as const, MODE0)
const VIEW = atom({ plugin: 'adage', key: 'view' } as const, VIEW0)
const MAP = atom({ plugin: 'adage', key: 'map' } as const, MAP0)
const TURN = atom({ plugin: 'adage', key: 'turn' } as const, TURN0)

type Answered = { additionalContext?: string[]; block?: string }
type Stoppable = AsyncGenerator<unknown, unknown>

/** The most tool batches kept for matching a feed judgement to its row. */
const MAX_BATCHES = 50
/** The most marks, fired reflexes and predicted steps kept for drawing in one session. */
const MAX_MARKS = 500
/** The bridge's failures in a row before the status line says so (ux-design §9.2). */
const UNANSWERING = 3
/** At most one refresh of the `--json` reads in this long, however many learn lines arrive. */
const REFRESH_MS = 5000

/** The hook binary that answered the probe; undefined while the mod is inert. */
let bin: string | undefined
/** `adage` itself, beside it (`<data>/bin/adage`): the verbs the mod reads and draws from. */
let cli: string | undefined
/** Whether a person is at the prompt (`session.start`'s `isInteractive`): nothing draws headless. */
let interactive = false
/** The surface the session drew on at start. */
let surface: RenderSurface | null = null

/**
 * Observe mode (ADAGE_MOD_OBSERVE set and non-empty; the demo's `mod-observe` arm): the binary is
 * still asked at every step and Bash call, with `observe`, and records what it would have done as
 * the control arm does; the mod never yields a predicted call and never replaces a command.
 */
let observing = false

/** The follower: the session it follows, its generation (a new one ends the old), its child. */
let feedSession = ''
let feedGen = 0
let paused = false
let child: Stoppable | undefined
/** The redraw timers (the learner's countdown, `learned` fading). */
let countdown: Timer | undefined
let fading: Timer | undefined
let lastRefresh = -Infinity
/** The hook binary's failed answers in a row (three: a status warning, ux-design §9.2). */
let failures = 0

/**
 * Lets work run on without the hook waiting for it, failing quietly: a hook never fails because
 * a drawing's read did (fail open, architecture §6 invariant 2).
 */
function quietly(work: Promise<unknown>): void {
  work.catch(() => undefined)
}

/** `$.process.run` behind the allow-list and CLAUDECODE=1 (bridge.ts `guard`). */
function runner($: EngineInterface): Run {
  return guard((argv, init) => $.process.run(argv, init))
}

/** `$.process.spawn` behind the same allow-list and CLAUDECODE=1. */
function spawner($: EngineInterface): Spawn {
  return guardSpawn((request: ProcessSpawnRequest) => $.process.spawn(request))
}

/** `ask($, event, payload, timeoutMs)`: one question to the active binary, tagged; three failures in a row warn once. */
async function askBinary($: EngineInterface, event: string, payload: { cwd?: string }, timeoutMs: number): Promise<Asked> {
  const asked = await ask(runner($), bin, event, payload, timeoutMs)
  if (bin === undefined) return asked
  failures = asked.ok ? 0 : failures + 1
  if (failures >= UNANSWERING) await warnOnce($, 'unanswering', 'hooks are not answering; run adage doctor').catch(() => undefined)
  return asked
}

/** A status line (warnings only; the engine prefixes `⚠ adage:`), once per session for its id. */
async function warnOnce($: EngineInterface, id: string, text: string): Promise<void> {
  if (!interactive || (await read($, MODE)).warned.includes(id)) return
  await update($, MODE, m => ({ ...m, warned: [...m.warned, id] }))
  $.ui.status(text)
}

/** A toast, once per session for its id (a lapse, a failed learn, the desktop's opening line). */
async function toastOnce($: EngineInterface, id: string, text: string): Promise<void> {
  if (!interactive || (await read($, MODE)).warned.includes(id)) return
  await update($, MODE, m => ({ ...m, warned: [...m.warned, id] }))
  $.ui.toast(text)
}

async function observe<A extends Answered>($: EngineInterface, answered: A, event: string, extra: Partial<Fired> = {}): Promise<A> {
  if (bin === undefined) return answered
  const fired = parseReflexes([...(answered.additionalContext ?? []), answered.block], event).map(f => ({ ...f, ...extra }))
  if (fired.length > 0) await update($, FIRED, cur => [...cur, ...fired].slice(-MAX_MARKS)).catch(() => undefined)
  return answered
}

/** Probes the binary the shim linked; the mod is active only while it answers. */
async function activate($: EngineInterface): Promise<void> {
  const pluginData = await $.env.get('CLAUDE_PLUGIN_DATA')
  observing = ((await $.env.get('ADAGE_MOD_OBSERVE')) ?? '') !== ''
  const ascii = (await $.env.get('ADAGE_ASCII')) === '1'
  const reducedMotion = (await $.env.get('ADAGE_REDUCED_MOTION')) === '1'
  const data = dataDir(pluginData, $.plugin.root)
  bin = await probe(runner($), hookBinary(pluginData, $.plugin.root))
  cli = bin === undefined ? undefined : cliBinary(pluginData, $.plugin.root)
  let nobinary = ''
  if (bin === undefined && data !== undefined) {
    const said = await $.fs.read(`${data}/status`).catch(() => '')
    nobinary = typeof said === 'string' ? said.trim() : ''
  }
  const was = await read($, MODE)
  const active = bin !== undefined
  if (was.active !== active || was.interactive !== interactive || was.surface !== surface || was.nobinary !== nobinary || was.ascii !== ascii || was.reducedMotion !== reducedMotion) {
    await update($, MODE, m => ({ ...m, active, interactive, surface, nobinary, ascii, reducedMotion }))
  }
  if (nobinary !== '') await warnOnce($, 'nobinary', nobinary)
  if (bin === undefined) return
  await $.command.register({ name: COMMAND, description: DESCRIPTION, argumentHint: ARGUMENT_HINT, immediate: true }).catch(() => undefined)
  if (!interactive) return
  await harnessVersion($, ascii).catch(() => undefined)
  const view = await read($, VIEW)
  paused = view.paused
  const kept = await $.store.get('adage.tab').catch(() => undefined)
  const tab: View['tab'] | undefined = kept === 'live' || kept === 'reflexes' || kept === 'health' ? kept : undefined
  if (tab !== undefined && tab !== view.tab) await update($, VIEW, v => ({ ...v, tab }))
}

/** `band.version` (ux-design §9.6): once per new harness version per machine, until the first prompt. */
async function harnessVersion($: EngineInterface, ascii: boolean): Promise<void> {
  const { version } = await $.session.version()
  const seen = await $.store.get('adage.harness')
  const line = seen === version ? '' : versionLine(version, TESTED, ascii)
  if ((await read($, MODE)).versionLine !== line) await update($, MODE, m => ({ ...m, versionLine: line }))
}

/** Starts (or restarts) the follower for this session, from the last cursor. */
async function startFeed($: EngineInterface, session: string, cwd: string): Promise<void> {
  if (cli === undefined || !interactive) return
  const gen = ++feedGen
  feedSession = session
  const exe = cli
  const mine = stem(session)
  const spawn = spawner($)
  const live = () => gen === feedGen
  const was = await read($, FEED)
  const since = (await $.clock.now()) - 5000
  quietly(follow(
    {
      spawn: argv => {
        const stream = spawn({ argv, cwd })
        child = stream
        return stream
      },
      now: () => $.clock.now(),
      argv: since => followArgs(exe, cwd, session, since),
      parse: text => parseLine(text, mine),
      isPaused: () => paused || !live(),
      apply: async (lines, cursor) => {
        if (!live()) return
        await apply($, lines, mine, since)
        await update($, FEED, f => ({ ...f, cursor }))
      },
      status: async (status, failures) => {
        if (live()) await update($, FEED, f => ({ ...f, status, failures }))
      },
    },
    was.cursor,
    0,
  ))
}

/**
 * Ends the follower's child (a pause, a new session); the loop then says how it stands. Not
 * awaited: the child ends when the engine ends it, and nothing waits on a child to go.
 */
function stopFeed(): void {
  const c = child
  child = undefined
  void c?.return(undefined).catch(() => undefined)
}

/** What a chunk of feed lines changes: rows, ticks, health, learner; the toasts and marks they earn. */
async function apply($: EngineInterface, lines: Line[], mine: string, since: number): Promise<void> {
  const now = await $.clock.now()
  const was = { rows: await read($, ROWS), ticks: await read($, TICKS), health: await read($, HEALTH), learner: await read($, LEARNER) }
  const next = fold(was, lines, now)
  if (next.rows !== was.rows) await update($, ROWS, () => next.rows)
  if (next.ticks !== was.ticks) await update($, TICKS, () => next.ticks)
  if (next.health !== was.health) await update($, HEALTH, () => next.health)
  if (next.learner !== was.learner) {
    await update($, LEARNER, () => next.learner)
    timers($, next.learner)
  }
  const ascii = (await read($, MODE)).ascii
  const reflexes = await read($, REFLEXES)
  for (const l of lines) {
    // A learn line from before this follower started is history (the backfill), not news.
    if (l.kind !== 'learn' || l.ts < since) continue
    const toast = learnToast(l.line, mine, ascii, route => reflexOf(reflexes, route)?.gist)
    if (toast === undefined) continue
    if (l.line.kind === 'condition_error') await toastOnce($, 'learn-failed', toast)
    else if (interactive) $.ui.toast(toast)
  }
  if (lines.some(l => l.kind === 'learn')) quietly(refresh($))
  const decisions = lines.filter(l => l.kind === 'decision').length
  if (decisions > 0) await avoided($, next.rows.slice(-decisions), next.rows)
}

/**
 * The avoid marker (`tool.avoided`): a dead end this session was told about (an inhibit directive)
 * and then followed, judged at a tool batch; drawn under that batch's last call, matched by time.
 */
async function avoided($: EngineInterface, added: readonly Row[], rows: readonly Row[]): Promise<void> {
  const inhibit = new Set(rows.filter(r => r.mine).flatMap(r => r.acts.filter(a => a.act === 'directed' && a.strength === 'inhibit').map(a => a.route ?? '')))
  const hits = added.filter(r => r.mine).flatMap(r => r.acts.filter(a => a.act === 'followed' && inhibit.has(a.route ?? '')).map(a => ({ route: a.route ?? '', ts: r.ts })))
  if (hits.length === 0) return
  const batches = await read($, BATCHES)
  const marks: Mark[] = []
  for (const h of hits) {
    const b = [...batches].reverse().find(x => h.ts >= x.from - 50 && h.ts <= x.to + 2000)
    const id = b?.ids[b.ids.length - 1]
    if (id !== undefined) marks.push({ id, kind: 'avoided', route: h.route })
  }
  if (marks.length > 0) await update($, MARKS, cur => [...cur, ...marks].slice(-MAX_MARKS))
}

/** The learner's countdown redraws each second while it waits; `learned` fades after 10 s. */
function timers($: EngineInterface, learner: Learner | null): void {
  if (learner?.state === 'waiting') {
    countdown ??= $.clock.every(1000, () => $.ui.invalidate('ui.render'))
  } else {
    countdown?.cancel()
    countdown = undefined
  }
  if (learner?.ranAt != null) {
    fading?.cancel()
    fading = $.clock.after(10_500, () => $.ui.invalidate('ui.render'))
  }
}

const isOk = (r: { ok: boolean }): r is { ok: true; json: Record<string, unknown> } => r.ok

/**
 * Reads again what the pane and the band draw from the binary: the reflexes (`graph --json`), the
 * checks (`doctor --json`), the lapsed grants (`delegate status --json`), the packs awaiting
 * trust (`status --json`) and the opt-in lines (the card). At most once per `REFRESH_MS` unless
 * forced; a read that fails keeps what was drawn.
 */
async function refresh($: EngineInterface, force = false): Promise<void> {
  if (cli === undefined || !interactive) return
  const now = await $.clock.now()
  if (!force && now - lastRefresh < REFRESH_MS) return
  lastRefresh = now
  const run = runner($)
  const cwd = await $.session.cwd()
  const [graph, doctor, deleg, status, card] = await Promise.all([
    readJson(run, cli, ['graph', '--json'], cwd),
    readJson(run, cli, ['doctor', '--json'], cwd),
    readJson(run, cli, ['delegate', 'status', '--json'], cwd),
    readJson(run, cli, ['status', '--json'], cwd),
    readJson(run, cli, ['--json', '--for-session'], cwd),
  ])
  if (isOk(graph)) await update($, REFLEXES, () => reflexesOf(graph.json))
  if (isOk(doctor)) await update($, CHECKS, () => checksOf(doctor.json))
  if (isOk(status)) await update($, PACKS, () => packsOf(status.json))
  if (isOk(card)) {
    // The opt-in lines, when the binary reports them (`ui.spinner`, `ui.turn_line`); off otherwise.
    const ui = card.json.ui
    const on = (k: string) => typeof ui === 'object' && ui !== null && (ui as Record<string, unknown>)[k] === true
    await update($, MODE, m => ({ ...m, spinner: on('spinner'), turnLine: on('turn_line') }))
  }
  if (isOk(deleg)) {
    const lapsed = lapsedOf(deleg.json)
    await update($, LAPSED, () => lapsed)
    const ascii = (await read($, MODE)).ascii
    for (const l of lapsed) await toastOnce($, `lapsed:${l.route}`, lapseToast(l, ascii))
  }
}

/** Opens the pane on a tab, focused: the person asked (`/adage …`, ux-design §6). */
async function openPane($: EngineInterface, tab: View['tab']): Promise<void> {
  await update($, VIEW, v => ({ ...v, tab }))
  await $.ui.open({ id: PANE, title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 })
  if (tab !== 'live') quietly(refresh($, true))
}

/** Copies a command for the person and says so (`toast.copied`, `toast.copy.unavailable`). */
async function copy($: EngineInterface, command: string, on?: string): Promise<boolean> {
  const target = on === 'terminal' || on === 'desktop' || on === 'vscode' || on === 'mobile' ? on : undefined
  const done = await $.ui.copy(target === undefined ? { text: command } : { text: command, surface: target }).catch(() => ({ isCopied: false as const }))
  $.ui.toast(done.isCopied ? `copied: ${command}` : 'copy is not available here · the command is in /adage health')
  return done.isCopied
}

/** Pauses or resumes the feed and the band (`/adage pause`, `p` in the pane); true when now paused. */
async function togglePause($: EngineInterface): Promise<boolean> {
  const view = await update($, VIEW, v => ({ ...v, paused: !v.paused }))
  paused = view.paused
  if (paused) stopFeed()
  else if (feedSession !== '') await startFeed($, feedSession, await $.session.cwd())
  return paused
}

/** `Explore in browser` (ux-design §6): starts `adage map` once (it opens the browser) and keeps its address. */
async function explore($: EngineInterface): Promise<string | undefined> {
  const now = await read($, MAP)
  if (now.url !== '') return now.url
  if (cli === undefined || now.starting) return undefined
  await update($, MAP, m => ({ ...m, starting: true }))
  const stream = spawner($)({ argv: [cli, 'map', '--port', '0'], cwd: await $.session.cwd() })
  return new Promise<string | undefined>(resolve => {
    let said = ''
    let found = false
    quietly((async () => {
      try {
        for await (const chunk of stream) {
          said = (said + chunk.text).slice(-4096)
          const url = found ? undefined : mapUrl(said)
          if (url !== undefined) {
            found = true
            await update($, MAP, () => ({ url, starting: false }))
            resolve(url)
          }
        }
      } catch {
        // It could not start, or it ended: the link goes, and a press starts it again.
      }
      if (!found) resolve(undefined)
      await update($, MAP, () => MAP0)
    })())
  })
}

/** The pane's handlers, bound to `$` (draw/pane.tsx calls them from presses, picks and fields). */
function actions($: EngineInterface): PaneActions {
  return {
    tab: tab => {
      quietly(update($, VIEW, v => ({ ...v, tab })))
      quietly($.store.set('adage.tab', tab))
    },
    filter: (value, submit) => quietly(update($, VIEW, v => ({ ...v, filter: submit && value.trim() === '' ? '' : value }))),
    inspect: seq => {
      quietly((async () => {
        const view = await read($, VIEW)
        const rows = await read($, ROWS)
        const ascii = (await read($, MODE)).ascii
        // A row the filter removed is never inspected (ux-design §6.1).
        if (!visibleRows(rows, view.filter, view.hideQuiet, ascii).some(r => r.seq === seq)) return
        await update($, VIEW, v => ({ ...v, selected: seq }))
        await $.ui.open({ id: INSPECT, title: 'adage · inspect', focus: true, closeOnEscape: true, rows: 12 })
      })())
    },
    pause: () => quietly(togglePause($)),
    quiet: () => quietly(update($, VIEW, v => ({ ...v, hideQuiet: !v.hideQuiet }))),
    newest: () => quietly($.ui.scroll({ in: PANE, to: 'start' })),
    rfilter: value => quietly(update($, VIEW, v => ({ ...v, rfilter: value }))),
    pick: (which, value) => quietly(update($, VIEW, v => (which === 'strength' ? { ...v, strength: value } : { ...v, lifecycle: value }))),
    expand: key => quietly(update($, VIEW, v => ({ ...v, expanded: v.expanded === key ? '' : key }))),
    copy: (command, on) => quietly(copy($, command, on)),
    condition: () => {
      quietly((async () => {
        await say(runner($), cli, ['condition'], await $.session.cwd())
        await refresh($, true)
      })())
    },
    explore: () => quietly(explore($)),
  }
}

/** Whether a drawing may draw: a person at the prompt, on a surface the trees fit (ux-design §4.10). */
function drawable(e: { surface: RenderSurface }): boolean {
  return interactive && (e.surface === 'terminal' || e.surface === 'desktop')
}

/** Starts the per-session counts again (a /clear is a new engine session, ux-design §4.9). */
async function newSession($: EngineInterface): Promise<void> {
  await update($, FIRED, () => [])
  await update($, PREDICTED, () => [])
  await update($, MARKS, () => [])
  await update($, BATCHES, () => [])
  await update($, TURN, () => TURN0)
  await update($, ROWS, rows => rows.map(r => ({ ...r, mine: false })))
  failures = 0
  await update($, MODE, m => ({ ...m, warned: [] }))
}

/** Whether the session can run this skill now: a Skill call it cannot is never made in its place. */
async function hasSkill($: EngineInterface, skill: string): Promise<boolean> {
  return (await $.command.list()).some(c => c.name === skill)
}

/** The person's rules on a Bash command now, as the engine decides them (nothing runs). */
async function checkBash($: EngineInterface, command: string): Promise<string> {
  return (await $.tool.check({ tool: 'Bash', input: { command } })).decision
}

async function record($: EngineInterface, p: Predicted): Promise<void> {
  await update($, PREDICTED, cur => [...cur, p].slice(-MAX_MARKS))
}

async function mark($: EngineInterface, m: Mark): Promise<void> {
  await update($, MARKS, cur => [...cur.filter(x => !(x.id === m.id && x.kind === m.kind)), m].slice(-MAX_MARKS))
}

export const register: Register = (on, options) => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    interactive = e.isInteractive
    surface = e.surface
    await activate($)
    if (bin !== undefined && interactive) {
      await startFeed($, await $.session.id(), e.cwd)
      quietly(refresh($, true))
      // Unasked only with `pane: auto`; the engine seats an unasked pane from 144 columns (§6).
      if (options.pane === 'auto') quietly($.ui.open({ id: PANE, title: 'adage' }))
    }
    return started
  })

  // The link is made by the shim, the SessionStart command hook beneath this observer: probe
  // again once it has run, so a first session (no link yet at session.start) and a /clear (the
  // shim relinks) see the binary the shim chose. A /clear is a new session: its counts start
  // again and the follower follows the new id.
  on('classic.SessionStart', async ($, e, next) => {
    const answered = await next(e)
    await activate($)
    if (bin !== undefined && interactive) {
      const session = await $.session.id()
      if (session !== feedSession) {
        if (feedSession !== '') {
          stopFeed()
          await newSession($)
        }
        await startFeed($, session, await $.session.cwd())
        quietly(refresh($, true))
      }
    }
    return answered
  })

  // Observers of the command hooks (§3.1): the command hooks beneath answer once; the answer is
  // returned unchanged, and the reflexes it delivered are kept for drawing.
  on('classic.UserPromptSubmit', async ($, e, next) => {
    const answered = await next(e)
    if (bin === undefined || e.agent_id !== undefined || (e.source !== undefined && e.source !== 'user' && e.source !== 'sdk')) return answered
    const fired = await read($, FIRED)
    const predicted = await read($, PREDICTED)
    const marks = await read($, MARKS)
    const turn = await update($, TURN, t => ({ submits: t.submits + 1, fired: fired.length, predicted: predicted.length, marks: marks.length }))
    if ((await read($, MODE)).versionLine !== '') {
      const { version } = await $.session.version()
      await $.store.set('adage.harness', version)
      await update($, MODE, m => ({ ...m, versionLine: '' }))
    }
    return observe($, answered, 'UserPromptSubmit', { prompt: turn.submits, promptText: e.prompt })
  })
  on('classic.PostToolBatch', async ($, e, next) => {
    const from = bin === undefined ? 0 : await $.clock.now()
    const answered = await next(e)
    if (bin === undefined) return answered
    const to = await $.clock.now()
    const ids = e.tool_calls.map(c => c.tool_use_id).filter(id => typeof id === 'string' && id !== '')
    if (ids.length > 0) await update($, BATCHES, cur => [...cur, { ids, from, to }].slice(-MAX_BATCHES)).catch(() => undefined)
    return observe($, answered, 'PostToolBatch')
  })
  on('classic.PostToolUseFailure', async ($, e, next) => observe($, await next(e), 'PostToolUseFailure'))
  on('classic.Stop', async ($, e, next) => {
    const answered = await observe($, await next(e), 'Stop')
    // The finish line asked the agent to continue (`toast.gate`): said once per gate, not to the model.
    if (bin !== undefined && interactive && answered.block !== undefined) $.ui.toast('adage: finish line · the agent continues once')
    return answered
  })

  // What the PreToolUse command hook did to a Bash call (a rewrite to the project's interpreter, a
  // receipt delivered beside it): kept for the marker under its row. The answer is unchanged.
  on('classic.PreToolUse', async ($, e, next) => {
    const answered = await next(e)
    if (bin === undefined || e.tool !== 'Bash') return answered
    const rw = rewriteOf(e.command, answered.updatedInput?.command)
    if (rw !== undefined) await mark($, { id: e.tool_use_id, kind: 'rewrite', ...rw }).catch(() => undefined)
    const rc = receiptOf(answered.additionalContext ?? [])
    if (rc !== undefined) await mark($, { id: e.tool_use_id, kind: 'receipt', receipt: rc }).catch(() => undefined)
    return answered
  })

  // The step Adage makes in the model's place (§3.2), and the finish line (§3.2, C8).
  on('turn.step', async function* ($, e, next) {
    return yield* turnStep(e, next, {
      isActive: bin !== undefined,
      isObserving: observing,
      hasSkill: skill => hasSkill($, skill),
      ask: payload => askBinary($, 'TurnStep', payload, TURN_STEP_TIMEOUT_MS),
      context: async () => ({ sessionId: await $.session.id(), cwd: await $.session.cwd() }),
      record: p => record($, p),
    })
  })

  // The dead end that never runs (§3.4, C6): a learned replacement, only on the same verdict.
  on('tool.call', { tool: 'Bash' }, async ($, e, next) =>
    substituteCall(e, next, {
      isActive: bin !== undefined,
      isObserving: observing,
      ask: payload => askBinary($, 'PreToolUse', payload, PRE_TOOL_TIMEOUT_MS),
      context: async () => ({ sessionId: await $.session.id(), cwd: await $.session.cwd() }),
      check: command => checkBash($, command),
      notice: (id, text) => $.ui.notice(id, text),
      made: (id, command) => mark($, { id, kind: 'substituted', command }),
    }),
  )

  // `/adage …` (ux-design §7): one registration (made at activation), the verb parsed here.
  on('command.run', { command: 'adage' }, async ($, e) =>
    runCommand(e.args, {
      card: async () => {
        const r = await readJson(runner($), cli, ['--json', '--for-session'], await $.session.cwd())
        return r.ok ? cardOf(r.json) : undefined
      },
      open: tab => (interactive ? openPane($, tab) : Promise.resolve()),
      why: async () => whyText(await read($, ROWS), await read($, REFLEXES), (await read($, MODE)).ascii),
      off: async () => {
        const said = await say(runner($), cli, ['off'], await $.session.cwd())
        quietly(refresh($, true))
        return said
      },
      copy: command => copy($, command),
      pause: () => togglePause($),
      map: () => explore($),
    }),
  )

  // ---------------------------------------------------------------------------------- drawing
  // Every tree is a function of $.state (§4): a hook draws only for a person at the prompt, on the
  // terminal or the desktop, and returns next(e) whenever it has nothing to add.

  // The opening line (`open.notice`): the binary's own notice, with the glyph and no prefix.
  on('ui.render', { component: 'InfoNotice' }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined) return next(e)
    const text = openNotice(e.props.text, (await read($, MODE)).ascii)
    return text === undefined ? next(e) : next({ ...e, props: { ...e.props, text } })
  })

  // `prompt.fired`: one line per reflex fired at the prompt, under the engine's row. The row is
  // matched to the hook's answer by the prompt's text; the payload carries no message id, and a
  // prompt typed twice draws the later one's lines.
  on('ui.render', { component: 'UserMessage' }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined || e.props.isExpanded) return next(e)
    const fired = (await read($, FIRED)).filter(f => f.event === 'UserPromptSubmit' && f.promptText === e.props.text)
    const n = Math.max(0, ...fired.map(f => f.prompt ?? 0))
    const these = fired.filter(f => f.prompt === n)
    if (these.length === 0) return next(e)
    const ascii = (await read($, MODE)).ascii
    const lines = promptLines(these, await read($, REFLEXES), await read($, ROWS), e.viewport?.columns ?? 80, ascii)
    return lines.length === 0 ? next(e) : under($.ui.resolve(e) as Els, await next(e), lines)
  })

  // The marker under a tool row: a rewrite, a predicted step, a substitution, a dead end headed off.
  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined) return next(e)
    const id = e.props.tool_use_id || e.requestId
    const marks: RowMark[] = (await read($, MARKS)).filter(
      (m): m is Extract<Mark, { kind: 'rewrite' | 'substituted' | 'avoided' }> => m.id === id && (m.kind === 'rewrite' || m.kind === 'substituted' || m.kind === 'avoided'),
    )
    if ((await read($, PREDICTED)).some(p => p.ids.includes(id))) marks.unshift({ kind: 'predicted' })
    if (marks.length === 0) return next(e)
    const ascii = (await read($, MODE)).ascii
    return under($.ui.resolve(e) as Els, await next(e), marks.map(m => markText(m, ascii)))
  })

  // The receipt card in place of a test run's output, or one line beside a run that happened.
  on('ui.render', { component: 'ToolResult' }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined) return next(e)
    const id = e.props.tool_use_id || e.requestId
    const marks = (await read($, MARKS)).filter(m => m.id === id)
    const ascii = (await read($, MODE)).ascii
    const answered = marks.find((m): m is Extract<Mark, { kind: 'answered' }> => m.kind === 'answered')
    if (answered !== undefined) return receipt($.ui.resolve(e) as Els, receiptCard(answered, ascii))
    const besides = marks.find((m): m is Extract<Mark, { kind: 'receipt' }> => m.kind === 'receipt')
    if (besides === undefined) return next(e)
    return under($.ui.resolve(e) as Els, await next(e), [receiptBeside(besides.receipt, ascii)])
  })

  // `spinner.suffix` (`ui.spinner`, off by default): while a reflex was delivered this turn.
  on('ui.render', { component: 'Spinner' }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined) return next(e)
    const mode = await read($, MODE)
    if (!mode.spinner) return next(e)
    const turn = await read($, TURN)
    const n = new Set((await read($, FIRED)).slice(turn.fired).map(f => f.route)).size
    const suffix = spinnerSuffix(n, mode.ascii)
    return suffix === '' ? next(e) : next({ ...e, props: { ...e.props, suffix: `${e.props.suffix}${suffix}` } })
  })

  // `turn.summary` (`ui.turn_line`, off by default; the terminal only): the turn's counts.
  on('ui.render', { component: 'TurnDuration' }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined) return next(e)
    const mode = await read($, MODE)
    if (!mode.turnLine) return next(e)
    const turn = await read($, TURN)
    const marks = (await read($, MARKS)).slice(turn.marks)
    const text = turnSummary(
      (await read($, PREDICTED)).length - turn.predicted,
      marks.filter(m => m.kind === 'answered').length,
      marks.filter(m => m.kind === 'avoided' || m.kind === 'substituted').length,
      mode.ascii,
    )
    return text === '' ? next(e) : beside($.ui.resolve(e) as Els, await next(e), text)
  })

  // The band (§5), unless the person turned it off (`band` in /config).
  if (options.band !== false) {
    on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
      if (!drawable(e) || e.props.hasSurvey) return next(e)
      const mode = await read($, MODE)
      const view = await read($, VIEW)
      const rows = await read($, ROWS)
      const turn = await read($, TURN)
      const marks = await read($, MARKS)
      const learner = await read($, LEARNER)
      const b = band({
        active: mode.active,
        nobinary: mode.nobinary,
        paused: view.paused,
        health: await read($, HEALTH),
        inRepo: learner !== null,
        reflexes: firedThisSession(await read($, FIRED), rows),
        steps: (await read($, PREDICTED)).length,
        reused: marks.filter(m => m.kind === 'answered').length,
        learner,
        now: await $.clock.now(),
        lapsed: (await read($, LAPSED)).length,
        review: waiting(await read($, REFLEXES), await read($, PACKS)).length,
        versionLine: turn.submits === 0 ? mode.versionLine : '',
        columns: e.props.bodyColumns,
        ascii: mode.ascii,
      })
      return b === undefined ? next(e) : bandTree($.ui.resolve(e) as Els, b, mode.ascii ? '-' : '·')
    })
  }

  // The pane (§6): Live, Reflexes, Health.
  on('ui.render', { component: 'Pane', requestId: 'adage' }, async ($, e, next) => {
    if (e.surface !== 'terminal' && e.surface !== 'desktop') return next(e)
    const mode = await read($, MODE)
    const reflexes = await read($, REFLEXES)
    return paneTree(
      $.ui.resolve(e) as Els,
      {
        view: await read($, VIEW),
        rows: await read($, ROWS),
        ticks: await read($, TICKS),
        health: await read($, HEALTH),
        feed: (await read($, FEED)).status,
        reflexes,
        waiting: waiting(reflexes, await read($, PACKS), mode.ascii),
        checks: await read($, CHECKS),
        lapsed: await read($, LAPSED),
        map: await read($, MAP),
        nobinary: mode.nobinary,
        noBridge: e.surface === 'desktop' && !mode.active,
        columns: e.props.bodyColumns,
        ascii: mode.ascii,
      },
      actions($),
    )
  })

  // The inspect pane over the Live tab: the selected row's directives and evidence.
  on('ui.render', { component: 'Pane', requestId: 'adage-inspect' }, async ($, e, next) => {
    if (e.surface !== 'terminal' && e.surface !== 'desktop') return next(e)
    const view = await read($, VIEW)
    const row = (await read($, ROWS)).find(r => r.seq === view.selected)
    if (row === undefined) return next(e)
    return inspectTree($.ui.resolve(e) as Els, inspectText(row, await read($, REFLEXES)))
  })

  // `/adage` and `/adage why` drawn in their output row (§7); every other verb's line as printed.
  on('ui.render', { component: 'CommandOutput', props: { command: 'adage' } }, async ($, e, next) => {
    if (!drawable(e) || e.props.isErrored) return next(e)
    const verb = verbOf(e.props.args)
    if (verb === 'why') return whyTree($.ui.resolve(e) as Els, e.props.text)
    if ((verb === 'card' || verb === 'status' || verb === 'savings') && e.props.text.includes('\n')) {
      return cardTree($.ui.resolve(e) as Els, e.props.text, (command, at) => quietly(copy($, command, at)))
    }
    return next(e)
  })
}
