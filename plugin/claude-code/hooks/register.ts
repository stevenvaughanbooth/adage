// The adage mod (plan B1–B3, the C3–C9 hooks). The plugin ships both binaries and one copy of
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

import { NO_STANDING, STANDING_TIMEOUT_MS, standingOf, subagentOf, withInstruction, withNote, withPreamble, withSection, withoutAttachments } from './attach'
import type { Standing, StandingPayload } from './attach'
import { ask, cliBinary, dataDir, guard, guardSpawn, hookBinary, probe, read as readJson, say } from './bridge'
import type { Asked, Run, Spawn } from './bridge'
import { PRE_TOOL_TIMEOUT_MS, bashCall } from './call'
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
import { answeredMark, shortId } from './receipt'
import { parseReflexes } from './reflex'
import { TURN_STEP_TIMEOUT_MS, routineModelOf, turnStep } from './step'
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
/** The person's routine model (`runtime.routine_model`, from the status card); '' routes nothing. */
let routineModel = ''
/** Whether a session that draws nothing has read the card for it (once, at its first step that could route). */
let settled = false
/**
 * The session's `Standing` answer, asked once per session id (a /clear is a new one): the section,
 * the tool description and the compaction instruction read the same text all session, so the
 * prompt cache keeps it. A failed ask is the empty answer for the session.
 */
let standingFor: { session: string; answer: Promise<Standing> } | undefined

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
    // The opt-in lines, when the binary reports them (`ui.spinner`, `ui.turn_line`, `ui.noticed`);
    // off otherwise.
    const ui = card.json.ui
    const on = (k: string) => typeof ui === 'object' && ui !== null && (ui as Record<string, unknown>)[k] === true
    await update($, MODE, m => ({ ...m, spinner: on('spinner'), turnLine: on('turn_line'), noticed: on('noticed') }))
    routineModel = routineModelOf(card.json)
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

/**
 * What a session that draws nothing still needs from the status card: the routine model. A session
 * that draws reads the card with the rest (`refresh`).
 */
async function settings($: EngineInterface): Promise<void> {
  if (cli === undefined) return
  const card = await readJson(runner($), cli, ['--json', '--for-session'], await $.session.cwd())
  if (isOk(card)) routineModel = routineModelOf(card.json)
}

/** The `Standing` payload for this session, and for a subagent about to start. */
async function standingPayload($: EngineInterface, agent?: StandingPayload['agent']): Promise<StandingPayload> {
  return {
    session_id: await $.session.id(),
    cwd: await $.session.cwd(),
    hook_event_name: 'Standing',
    via: 'mod',
    ...(observing ? { observe: true as const } : {}),
    ...(agent === undefined ? {} : { agent }),
  }
}

/**
 * The session's standing text, asked once per session id; nothing while the mod is inert (the
 * question waits for a binary, so a first session whose link the shim makes later still asks).
 */
async function standing($: EngineInterface): Promise<Standing> {
  if (bin === undefined) return NO_STANDING
  const session = await $.session.id()
  if (standingFor?.session !== session) {
    const answer = standingPayload($)
      .then(p => askBinary($, 'Standing', p, STANDING_TIMEOUT_MS))
      .then(a => (a.ok ? standingOf(a.answer) : NO_STANDING), () => NO_STANDING)
    standingFor = { session, answer }
  }
  return standingFor.answer
}

/** The rewrite under a call's row (`tool.rewrite`), and the same line under its dialog. */
async function rewrote($: EngineInterface, id: string, from: string, to: string): Promise<void> {
  const rw = rewriteOf(from, to)
  if (rw === undefined) return
  const m: Mark = { id, kind: 'rewrite', ...rw }
  await mark($, m)
  try {
    $.ui.notice(id, markText(m, (await read($, MODE)).ascii))
  } catch {
    // No dialog is open for the call: the row's marker says it.
  }
}

export const register: Register = (on, options) => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    interactive = e.isInteractive
    surface = e.surface
    await activate($)
    // Asked now, off the hook's path, so the first request's section and tool description find it.
    if (bin !== undefined) quietly(standing($))
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
    if (bin !== undefined) quietly(standing($))
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
    // Off the step's path: a headless session reads its routine model when it first could use it,
    // and routes from the step after the read lands.
    if (bin !== undefined && !interactive && !settled && e.agentId === undefined && e.index > 0) {
      settled = true
      quietly(settings($))
    }
    return yield* turnStep(e, next, {
      isActive: bin !== undefined,
      isObserving: observing,
      hasSkill: skill => hasSkill($, skill),
      ask: payload => askBinary($, 'TurnStep', payload, TURN_STEP_TIMEOUT_MS),
      context: async () => ({ sessionId: await $.session.id(), cwd: await $.session.cwd() }),
      record: p => record($, p),
      routineModel,
    })
  })

  // A Bash call (§3.3, §3.4; C3, C6): a fresh receipt answers it when the rules would have let it
  // run unasked; a learned replacement or a rewrite runs only on the same verdict for both commands.
  on('tool.call', { tool: 'Bash' }, async ($, e, next) =>
    bashCall(e, next, {
      isActive: bin !== undefined,
      isObserving: observing,
      ask: payload => askBinary($, 'PreToolUse', payload, PRE_TOOL_TIMEOUT_MS),
      context: async () => ({ sessionId: await $.session.id(), cwd: await $.session.cwd() }),
      check: command => checkBash($, command),
      notice: (id, text) => $.ui.notice(id, text),
      answered: async (id, a) => {
        const m = answeredMark(id, a)
        if (m !== undefined) await mark($, m)
      },
      beside: (id, operation) => mark($, { id, kind: 'receipt', receipt: shortId(operation) }),
      made: (id, command) => mark($, { id, kind: 'substituted', command }),
      rewrote: (id, from, to) => rewrote($, id, from, to),
      delivered: async (id, context) => {
        const rc = receiptOf(context)
        if (rc !== undefined) await mark($, { id, kind: 'receipt', receipt: rc })
      },
    }),
  )

  // Standing text once, from the cache (§3.6, C5): the procedures as one `session` section of the
  // system prompt, the same text at every request of the session.
  on('prompt.compose', async ($, e, next) => {
    const composed = await next(e)
    if (bin === undefined) return composed
    const text = (await standing($).catch(() => NO_STANDING)).standing
    return text === '' ? composed : { ...composed, sections: withSection(composed.sections, text) }
  })

  // The Bash tool's description carries the environment note once (§1b.3, C9): how Python and the
  // tests run here, where the tool is described rather than at each call.
  on('tool.describe', { tool: 'Bash' }, async ($, e, next) => {
    const described = await next(e)
    if (bin === undefined) return described
    const note = (await standing($).catch(() => NO_STANDING)).environment
    const description = withNote(described.description, note)
    return description === described.description ? described : { ...described, description }
  })

  // Compaction keeps what the session followed (§3.7, C5): the binary's instruction, in both arms;
  // Adage's own attachment rows go only where the engine marks them as the plugin's.
  on('session.compact', async ($, e, next) => {
    if (bin === undefined) return next(e)
    const compact = (await standing($).catch(() => NO_STANDING)).compact
    const instructions = withInstruction(typeof e.instructions === 'string' ? e.instructions : undefined, compact)
    // A transcript that is not a list (a host that hands none) is passed on as it came.
    const messages = Array.isArray(e.messages) ? withoutAttachments(e.messages) : e.messages
    if (instructions === e.instructions && messages === e.messages) return next(e)
    return next({ ...e, messages, ...(instructions === undefined ? {} : { instructions }) })
  })

  // A subagent is told the conventions its task binds before its first request (§3.8, C9). A fork
  // carries the session's own context already, and is left as it is.
  on('agent.spawn', async ($, e, next) => {
    if (bin === undefined || e.fork) return next(e)
    let told = ''
    try {
      // The session's own answer first: the binary keeps the session's standing text from it.
      await standing($)
      const asked = await askBinary($, 'Standing', await standingPayload($, { type: e.subagentType, description: e.description, prompt: e.prompt }), STANDING_TIMEOUT_MS)
      told = asked.ok ? subagentOf(asked.answer) : ''
    } catch {
      // Told nothing: the subagent starts as the model asked.
    }
    return told === '' ? next(e) : next({ ...e, prompt: withPreamble(e.prompt, told) })
  })

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

  // =============================================================================== B4 · D5
  // Asking in the engine's dialog, the press, the pane's presses, noticed, learning and skills
  // (plan B4, D5; architecture §3.10–§3.12; ux-design §4.6–§4.8, §4.17, §6, §9.12). The block ends
  // at the closing brace; its helpers are at the end of the file. Every registration here nests
  // inside the ones above (first outermost), so each calls `next(e)` where its outer does, and the
  // ones on events the outer answers without `next` reach the drawing through `late` instead.

  // The handlers the pane's presses and `/adage on` use are bound once a person is at the prompt.
  on('session.start', { isInteractive: [true, false] }, async ($, e, next) => {
    const started = await next(e)
    bindLate($, e.isInteractive)
    return started
  })

  // The press is asked for after the command hooks' session start (the window opens there) with
  // that payload's session id and source; headless sessions issue no token and ask nothing.
  on('classic.SessionStart', { hook_event_name: 'SessionStart' }, async ($, e, next) => {
    const answered = await next(e)
    if (interactive) quietly(issuePress($, e))
    return answered
  })

  // The ask, at a turn's end, when the model is idle: never inside the hook (the dialog waits for
  // the person), at most one per turn end, one per reflex per day (ask.ts).
  on('turn.complete', { reason: 'answer' }, async ($, e, next) => {
    const done = await next(e)
    if (bin !== undefined && interactive && e.agentId === undefined) quietly(turnEnded($))
    return done
  })

  // A skill's text as the model reads it, with the steps Adage verified here (§3.11, D4).
  on('skill.prompt', async ($, e, next) => {
    if (bin === undefined) return next(e)
    const text = await verifiedSkill($, e.skill, e.text)
    return text === undefined ? next(e) : { text }
  })

  // `noticed.forming` / `noticed.review` under the tool batch that completed a candidate (§3.10).
  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined) return next(e)
    const mode = await read($, MODE)
    if (!mode.noticed) return next(e)
    const ids = e.props.calls.map(c => c.tool_use_id).filter((id): id is string => typeof id === 'string' && id !== '')
    if (ids.length === 0) return next(e)
    const rows = await read($, ROWS)
    const lines = noticedLines(rows, ids, await read($, BATCHES), await read($, CANDIDATES), mode.ascii)
    quietly(wordCandidates($, rows))
    if (lines.length === 0) return next(e)
    return under($.ui.resolve(e) as Els, await next(e), lines)
  })

  // The skill markers (`tool.skill.invoked`, `tool.skill.verified`) under a Skill tool row (§4.8).
  on('ui.render', { component: 'ToolUse', props: { tool: 'Skill' } }, async ($, e, next) => {
    if (!drawable(e) || bin === undefined) return next(e)
    const id = e.props.tool_use_id || e.requestId
    const name = isObject(e.props.input) && typeof e.props.input.skill === 'string' ? e.props.input.skill : ''
    const ascii = (await read($, MODE)).ascii
    const lines: string[] = []
    if ((await read($, PREDICTED)).some(p => p.kind === 'skill' && p.ids.includes(id))) lines.push(invokedMark(name === '' ? 'this' : name, ascii))
    const verified = (await read($, SKILLS)).find(s => s.skill === name)
    if (verified !== undefined) lines.push(verifiedMark(verified, ascii))
    if (lines.length === 0) return next(e)
    return skillRow($.ui.resolve(e) as Els, await next(e), lines)
  })

  // The grant pane (`see what it would do`, §4.17): the facts between the two acts.
  on('ui.render', { component: 'Pane', requestId: 'adage-allow' }, async ($, e, next) => {
    if (e.surface !== 'terminal' && e.surface !== 'desktop') return next(e)
    const allow = await read($, ALLOW)
    if (allow === null) return next(e)
    return allowTree($.ui.resolve(e) as Els, allow, e.props.bodyColumns, (await read($, MODE)).ascii, allowActions($))
  })

  // Whether the pane is open: the band's `1 pattern noticed` cell shows only while it is closed.
  on('ui.open', { id: 'adage' }, async ($, e, next) => {
    late.paneOpen = true
    return next(e)
  })
  on('ui.close', { id: 'adage' }, async ($, e, next) => {
    late.paneOpen = false
    return next(e)
  })
}

// ================================================================================= B4 · D5 helpers
// Imports are hoisted; these sit with the block they serve so the file above is untouched.
import { ON_ANSWERS, ON_QUESTION, answerOf, askedOf, doneToast, keptOf, nextAsk, record as recordAsk, whatOf } from './ask'
import type { AskItem } from './ask'
import { allowTree } from './draw/allow'
import type { AllowActions } from './draw/allow'
import { skillRow } from './draw/rows'
import { confirmCommand, confirmOf, routeOf, stageOf } from './grant'
import { ALLOW_PANE, KEPT_MAX_AGE_MS, PRESS0 } from './kinds'
import type { Allow, Kept, Press } from './kinds'
import { late } from './late'
import type { PressHandlers } from './late'
import { candidatesOf, invokedMark, noticedIds, noticedLines, skillOf, verifiedMark, withSkill } from './notice'
import { ANSWER, BUTTON, KEPT, guardPress, isToken, objectOf, press, pressArgv, tampered } from './press'
import type { Gesture, Pressed } from './press'
import { VERB_WORDS, reviewCommand } from './view'
import type { Waiting } from './view'

const PRESS = atom({ plugin: 'adage', key: 'press' } as const, PRESS0)
const ASKED = atom({ plugin: 'adage', key: 'asked' } as const, [])
const ALLOW = atom({ plugin: 'adage', key: 'allow' } as const, null)
const SKILLS = atom({ plugin: 'adage', key: 'skills' } as const, [])
const CANDIDATES = atom({ plugin: 'adage', key: 'candidates' } as const, [])

/** `Press` and `SkillPrompt` are asked off the hot path, with a command hook's budget. */
const PRESS_TIMEOUT_MS = 2000
const SKILL_PROMPT_TIMEOUT_MS = 2000
/** The note above the grant pane's facts when the repository moved between the two acts. */
const MOVED = 'the repository moved meanwhile; this is what it would do now'
/** The note when no press is held: the stage and the confirm cannot be kept for later. */
const NO_PRESS = 'no press is held in this session; the command below runs it at a terminal'

const isObject = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)

/** One ask at a time: a second turn end while the dialog waits asks nothing. */
let asking = false
/** When `status --json` was last read for the candidates' words. */
let lastCandidates = -Infinity

/** `$.process.run` behind the press's own allow-list (press.ts): person-only verbs, the token on stdin. */
function pressRunner($: EngineInterface): Run {
  return guardPress((argv, init) => $.process.run(argv, init))
}

/** Binds the pane's presses and `/adage on` to this `$`; nothing is bound headless. */
function bindLate($: EngineInterface, isInteractive: boolean): void {
  late.press = isInteractive ? pressHandlers($) : undefined
  late.skill = undefined
  late.noticed = 0
  if (isInteractive) quietly(loadStore($))
}

/** What `$.store` keeps across sessions: the asks (one per reflex per day) and the kept answers. */
async function loadStore($: EngineInterface): Promise<void> {
  const asked = askedOf(await $.store.get('adage.asked').catch(() => undefined))
  if (asked.length > 0) await update($, ASKED, cur => (cur.length === 0 ? asked : cur))
  late.kept = keptOf(await $.store.get('adage.kept').catch(() => undefined)).length
}

/**
 * Asks the binary for the session's press once the command hooks' session start has run (the
 * window is open until the agent's first act), and holds the token in `$.state` only. A start
 * the shim linked during this very hook is asked at the link, before the activation above sees it.
 */
async function issuePress($: EngineInterface, e: { session_id?: string; source?: string; cwd?: string }): Promise<void> {
  if (late.press === undefined) late.press = pressHandlers($)
  await loadStore($)
  const source = typeof e.source === 'string' ? e.source : 'startup'
  if (source === 'compact') return
  const session = typeof e.session_id === 'string' && e.session_id !== '' ? e.session_id : await $.session.id()
  const was = await read($, PRESS)
  const same = was.session === session
  const hook = bin ?? hookBinary(await $.env.get('CLAUDE_PLUGIN_DATA'), $.plugin.root)
  const cwd = typeof e.cwd === 'string' && e.cwd !== '' ? e.cwd : await $.session.cwd()
  const asked = await ask(runner($), hook, 'Press', { session_id: session, cwd, hook_event_name: 'Press', source, via: 'mod' }, PRESS_TIMEOUT_MS)
  const token = asked.ok && isObject(asked.answer) && isToken(asked.answer.press) ? asked.answer.press : ''
  // A resume reissues once for the same id: the spool then holds two issues at most.
  const issued = token === '' ? (same ? was.issued : 0) : source === 'resume' ? 2 : (same ? was.issued : 0) + 1
  const next: Press =
    token !== ''
      ? { session, token, state: same && was.state === 'off' ? 'off' : 'held', issued, chain: same ? was.chain : 0 }
      : { session, token: same ? was.token : '', state: same ? was.state : 'none', issued, chain: same ? was.chain : 0 }
  await update($, PRESS, () => next)
  if (next.state === 'held') await applyKept($)
}

/**
 * The tamper signal (§3.12 "Hold"): more `press_issued` records for this session than tokens the
 * mod was handed means something else claimed the window; presses stop for the session.
 */
async function checkTamper($: EngineInterface): Promise<void> {
  const p = await read($, PRESS)
  if (p.state === 'off' || p.session === '') return
  const seen = (await read($, ROWS)).filter(r => r.mine && r.event === 'press_issued').length
  if (!tampered(seen, p.issued)) return
  await update($, PRESS, cur => ({ ...cur, state: 'off' as const, token: '' }))
  await warnOnce($, 'press-tampered', "something else claimed this session's press; presses are off for this session · run adage doctor")
}

/** Keeps an answer for the next session start (ux-design §9.12); the band says so. */
async function keep($: EngineInterface, k: Kept): Promise<void> {
  const kept = keptOf(await $.store.get('adage.kept').catch(() => undefined)).filter(x => x.command !== k.command)
  await $.store.set('adage.kept', [...kept, k].slice(-50)).catch(() => undefined)
  late.kept = kept.length + 1
}

/** Runs the answers kept while no press was held, now that one is; a refused one stays kept. */
async function applyKept($: EngineInterface): Promise<void> {
  const kept = keptOf(await $.store.get('adage.kept').catch(() => undefined))
  if (kept.length === 0) {
    late.kept = 0
    return
  }
  const now = await $.clock.now()
  const left: Kept[] = []
  for (const k of kept) {
    if (now - k.at > KEPT_MAX_AGE_MS) continue
    const pressed = await pressCommand($, KEPT, k.command, k.say, k.key, whatOf(k.command), undefined, false)
    if (pressed === undefined) left.push(k)
  }
  await $.store.set('adage.kept', left).catch(() => undefined)
  late.kept = left.length
  quietly(refresh($, true))
}

/** Records a decision on a waiting item, so the ask does not return (`$.state`, and the store across sessions). */
async function noteAnswer($: EngineInterface, key: string, answer: string | undefined): Promise<void> {
  const now = await $.clock.now()
  const asked = await update($, ASKED, cur => recordAsk(cur, key, answer, now))
  await $.store.set('adage.asked', asked).catch(() => undefined)
}

/**
 * Runs one person-only command through the press (press.ts): the one path that reaches a verb a
 * person alone may run, from a dialog answer, a pane Button or an answer kept from one. With no
 * press held the answer is kept for the next session start when it can be (one act), and nothing
 * runs. Resolves to what the press came to, or undefined when nothing ran.
 */
async function pressCommand(
  $: EngineInterface,
  gesture: Gesture,
  command: string,
  say: string,
  key: string,
  what: string,
  extra?: Readonly<Record<string, unknown>>,
  keepable = true,
): Promise<Pressed | undefined> {
  if (cli === undefined) return undefined
  const argv = pressArgv(cli, command)
  if (argv === undefined) return undefined
  await checkTamper($)
  const p = await read($, PRESS)
  const session = await $.session.id()
  if (p.state !== 'held' || p.token === '' || p.session !== session) {
    if (keepable && gesture !== KEPT) {
      await keep($, { command, say, key, session, at: await $.clock.now() })
      await noteAnswer($, key, what)
    }
    return undefined
  }
  const pressed = await press(gesture, pressRunner($), argv, p.token, p.session, await $.session.cwd(), extra)
  // Refused before the spend (a wrong, spent or stale token): nothing was spent; otherwise the
  // token went with the verb, and the next one (if the binary printed it) is the one held.
  const refused = !pressed.ok && pressed.refused
  await update($, PRESS, cur => ({
    ...cur,
    token: pressed.next ?? '',
    state: pressed.next !== undefined ? ('held' as const) : ('none' as const),
    chain: refused ? cur.chain : cur.chain + 1,
  }))
  if (pressed.ok) {
    if (key !== '') await noteAnswer($, key, what)
    const toast = doneToast(what, say, VERB_WORDS[what])
    if (toast !== undefined && interactive) $.ui.toast(toast)
    if (what !== 'stage' && what !== 'confirm') quietly(refresh($, true))
  } else if (interactive && what !== 'stage' && what !== 'confirm') {
    $.ui.toast(`not recorded: ${pressed.error === '' ? 'the command did not succeed' : pressed.error}`)
  }
  return pressed
}

/** The pane's presses and `/adage on`, bound to `$` (late.ts). */
function pressHandlers($: EngineInterface): PressHandlers {
  return {
    verb: (verb, r) => quietly(pressCommand($, BUTTON, reviewCommand(verb, r), r.gist || r.summary, `${r.reference}/${r.id}`, verb)),
    yes: item => quietly(answerYes($, item, BUTTON)),
    no: item => quietly(answerNo($, item, BUTTON)),
    askOn: () => quietly(askOn($)),
  }
}

/** The first answer to a waiting item: enable it, stage the action, allow the prediction, use the pack. */
async function answerYes($: EngineInterface, item: Waiting, gesture: Gesture): Promise<void> {
  switch (item.kind) {
    case 'enable':
      await pressCommand($, gesture, item.command, item.say, item.key, 'approve')
      return
    case 'allow':
      if (item.offer === 'predict') await pressCommand($, gesture, item.command, item.say, item.key, 'predict')
      else await stageAllow($, item, gesture)
      return
    case 'trust':
      await pressCommand($, gesture, item.command, item.say, item.key, 'trust')
  }
}

/** `not this one` records a dismiss through the press; `not now` records only that it was asked. */
async function answerNo($: EngineInterface, item: Waiting, gesture: Gesture): Promise<void> {
  if (item.kind === 'enable' && item.no !== undefined) await pressCommand($, gesture, item.no, item.say, item.key, 'dismiss')
  else await noteAnswer($, item.key, 'not now')
}

/** `why`: the pane, focused, on that reflex (or pack), with `e enable` and `n not this one`. */
async function showReflex($: EngineInterface, key: string): Promise<void> {
  await update($, VIEW, v => ({ ...v, tab: 'reflexes' as const, expanded: key, rfilter: '', strength: 'all', lifecycle: 'all' }))
  await openPane($, 'reflexes')
}

/** The ask at a turn's end (ux-design §4.17): one item, the dialog's answer, the press. */
async function turnEnded($: EngineInterface): Promise<void> {
  await recountNoticed($)
  if (asking) return
  asking = true
  try {
    await checkTamper($)
    await refresh($)
    const items = waiting(await read($, REFLEXES), await read($, PACKS))
    const q = nextAsk(items, await read($, ASKED), await $.clock.now())
    if (q === undefined) return
    const said = await $.ui.ask(q.question, q.answers).catch(() => undefined)
    const answer = answerOf(q, said)
    await noteAnswer($, q.key, answer)
    await act($, q, answer)
  } finally {
    asking = false
  }
}

/** What an answer in the dialog does; a dismissed dialog (`later`) does nothing. */
async function act($: EngineInterface, q: AskItem, answer: string | undefined): Promise<void> {
  if (answer === undefined) return
  if (answer === 'why') return showReflex($, q.item.reflex ?? q.key)
  if (answer === 'not now') return
  if (answer === 'not this one') return answerNo($, q.item, ANSWER)
  return answerYes($, q.item, ANSWER)
}

/** `/adage on`: the one ask a command opens; the answer is the press (ux-design §7). */
async function askOn($: EngineInterface): Promise<void> {
  if (asking) return
  asking = true
  try {
    const said = await $.ui.ask(ON_QUESTION, ON_ANSWERS).catch(() => undefined)
    if (said === ON_ANSWERS[0]) await pressCommand($, ANSWER, 'adage on', 'adage', 'on', 'on')
  } finally {
    asking = false
  }
}

/** Whether a press is held for this session now. */
async function held($: EngineInterface): Promise<boolean> {
  const p = await read($, PRESS)
  return p.state === 'held' && p.token !== '' && p.session === (await $.session.id())
}

/**
 * The first act of an execution grant: stage it (`--stage --json`, recording nothing) and open the
 * grant pane, focused, on the facts. With no press held the pane shows the terminal command.
 */
async function stageAllow($: EngineInterface, item: Waiting, gesture: Gesture): Promise<void> {
  const where = routeOf(item.command)
  const allow: Allow = { key: item.key, say: item.say, kind: 'execute', track: where?.track ?? '', route: where?.route ?? '', command: item.command, stage: null, details: false, note: '', busy: true }
  await update($, ALLOW, () => allow)
  await $.ui.open({ id: ALLOW_PANE, title: 'adage · allow', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 }).catch(() => undefined)
  const same = (cur: Allow | null): cur is Allow => cur !== null && cur.key === item.key
  if (!(await held($))) {
    await update($, ALLOW, cur => (same(cur) ? { ...cur, busy: false, note: NO_PRESS, details: true } : cur))
    return
  }
  const pressed = await pressCommand($, gesture, item.command, item.say, item.key, 'stage', undefined, false)
  const stage = pressed?.ok ? stageOf(objectOf(pressed.lines)) : undefined
  const note =
    pressed === undefined ? NO_PRESS
    : !pressed.ok ? `not staged: ${pressed.error === '' ? 'the command did not succeed' : pressed.error}`
    : stage === undefined ? 'the binary answered nothing the pane can show; the command below runs it at a terminal'
    : stage.ok ? ''
    : stage.why
  await update($, ALLOW, cur => (same(cur) ? { ...cur, stage: stage ?? null, busy: false, note, details: stage === undefined } : cur))
}

/**
 * The second act: `--confirm` against the digest the stage produced. Recorded: the toast and the
 * pane closes; the repository moved: the fresh diff, asked again; refused: the binary's reason.
 */
async function confirmAllow($: EngineInterface): Promise<void> {
  const a = await read($, ALLOW)
  if (a === null || a.stage === null || !a.stage.ok || a.busy) return
  const same = (cur: Allow | null): cur is Allow => cur !== null && cur.key === a.key
  await update($, ALLOW, cur => (same(cur) ? { ...cur, busy: true, note: '' } : cur))
  if (!(await held($))) {
    await update($, ALLOW, cur => (same(cur) ? { ...cur, busy: false, note: NO_PRESS, details: true } : cur))
    return
  }
  const pressed = await pressCommand($, BUTTON, confirmCommand(a.command), a.say, a.key, 'confirm', { expect: a.stage.digest }, false)
  const c = pressed?.ok ? confirmOf(objectOf(pressed.lines)) : undefined
  if (c?.recorded === true) {
    await noteAnswer($, a.key, 'confirm')
    if (interactive) $.ui.toast(`allowed: ${a.say} · from your next session`)
    await closeAllow($)
    quietly(refresh($, true))
    return
  }
  const note =
    pressed === undefined ? NO_PRESS
    : !pressed.ok ? `not recorded: ${pressed.error === '' ? 'the command did not succeed' : pressed.error}`
    : c?.recorded === false && c.stage !== undefined ? (c.stage.ok ? MOVED : c.stage.why)
    : 'not recorded: the binary answered nothing the pane can show'
  await update($, ALLOW, cur => (same(cur) ? { ...cur, busy: false, note, stage: c?.recorded === false && c.stage !== undefined ? c.stage : cur.stage } : cur))
}

/** `not now`, or the grant recorded: the pane goes; nothing else runs. */
async function closeAllow($: EngineInterface): Promise<void> {
  await update($, ALLOW, () => null)
  await $.ui.close({ id: ALLOW_PANE }).catch(() => undefined)
}

/** The grant pane's handlers, bound to `$` (draw/allow.tsx calls them from its Buttons). */
function allowActions($: EngineInterface): AllowActions {
  return {
    allow: () => quietly(confirmAllow($)),
    notNow: () => quietly(closeAllow($)),
    details: () => quietly(update($, ALLOW, cur => (cur === null ? cur : { ...cur, details: !cur.details }))),
    copy: surface => {
      quietly((async () => {
        const a = await read($, ALLOW)
        if (a !== null) await copy($, a.command, surface)
      })())
    },
  }
}

/** The skill's text with the steps Adage verified here (`SkillPrompt`), or undefined to leave it as computed. */
async function verifiedSkill($: EngineInterface, skill: string, text: string): Promise<string | undefined> {
  const payload = {
    session_id: await $.session.id(),
    cwd: await $.session.cwd(),
    hook_event_name: 'SkillPrompt',
    skill,
    text,
    via: 'mod',
    ...(observing ? { observe: true as const } : {}),
  }
  const asked = await askBinary($, 'SkillPrompt', payload, SKILL_PROMPT_TIMEOUT_MS)
  const answered = asked.ok && isObject(asked.answer) && typeof asked.answer.text === 'string' && asked.answer.text.trim() !== '' ? asked.answer.text : undefined
  if (answered === undefined) return undefined
  const s = skillOf(skill, answered, await $.clock.now())
  if (s !== undefined) {
    late.skill = s
    await update($, SKILLS, cur => withSkill(cur, s)).catch(() => undefined)
  }
  return answered
}

/** The band's `1 pattern noticed`: distinct candidates noticed in this session's rows. */
async function recountNoticed($: EngineInterface): Promise<void> {
  const n = noticedIds(await read($, ROWS)).length
  if (n === late.noticed) return
  late.noticed = n
  $.ui.invalidate('ui.render')
}

/** Reads `status --json` for the words of a noticed candidate the drawing does not know yet. */
async function wordCandidates($: EngineInterface, rows: readonly Row[]): Promise<void> {
  await recountNoticed($)
  const known = new Set((await read($, CANDIDATES)).map(c => c.id))
  if (!noticedIds(rows).some(id => !known.has(id))) return
  const now = await $.clock.now()
  if (cli === undefined || now - lastCandidates < REFRESH_MS) return
  lastCandidates = now
  const status = await readJson(runner($), cli, ['status', '--json'], await $.session.cwd())
  if (!isOk(status)) return
  await update($, CANDIDATES, () => candidatesOf(status.json))
  $.ui.invalidate('ui.render')
}
