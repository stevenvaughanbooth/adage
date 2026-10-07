// The adage mod's registration (plan B1–B4, C3–C9, D5; docs/claude-mod/impl/mod-structure.md).
//
// `claude plugin validate` follows `$` only into functions declared in this file and refuses `$`,
// or any noun of it, passed across an import; it reads a state reference only as literals in a
// const of this file, and an environment variable only by a literal name. So this file holds the
// three things that need `$` and nothing else: the state table's atoms, `mod($, options)` (every
// call the mod makes, as a closure over `$`), and the `on(...)` lines, one per hook in a fixed
// order (registrations nest first outermost, so the order is behaviour), each delegating to the
// module that does the work. Everything else is in modules that take a `Mod` and never see `$`.
import { atom, read, update } from 'claude-code'
import type { EngineInterface, ProcessSpawnRequest, Register } from 'claude-code'

import * as actions from './actions'
import * as attach from './attach'
import { guard, guardSpawn } from './bridge'
import * as call from './call'
import * as commands from './commands'
import * as dialog from './dialog'
import type { Els } from './draw/els'
import * as sites from './draw/sites'
import * as ledger from './ledger'
import type { Mod, Options } from './mod'
import { runtime } from './mod'
import * as observe from './observe'
import { STATE0 } from './plugin'
import { guardPress } from './press'
import * as session from './session'
import * as skill from './skill'
import * as step from './step'

// The state table (plugin.ts `STATE0`, types/index.d.ts): one atom per `$.state` key.
const FIRED = atom({ plugin: 'adage', key: 'fired' } as const, STATE0.fired)
const PREDICTED = atom({ plugin: 'adage', key: 'predicted' } as const, STATE0.predicted)
const MARKS = atom({ plugin: 'adage', key: 'marks' } as const, STATE0.marks)
const BATCHES = atom({ plugin: 'adage', key: 'batches' } as const, STATE0.batches)
const FEED = atom({ plugin: 'adage', key: 'feed' } as const, STATE0.feed)
const ROWS = atom({ plugin: 'adage', key: 'rows' } as const, STATE0.rows)
const TICKS = atom({ plugin: 'adage', key: 'ticks' } as const, STATE0.ticks)
const HEALTH = atom({ plugin: 'adage', key: 'health' } as const, STATE0.health)
const LEARNER = atom({ plugin: 'adage', key: 'learner' } as const, STATE0.learner)
const REFLEXES = atom({ plugin: 'adage', key: 'reflexes' } as const, STATE0.reflexes)
const CHECKS = atom({ plugin: 'adage', key: 'checks' } as const, STATE0.checks)
const LAPSED = atom({ plugin: 'adage', key: 'lapsed' } as const, STATE0.lapsed)
const PACKS = atom({ plugin: 'adage', key: 'packs' } as const, STATE0.packs)
const MODE = atom({ plugin: 'adage', key: 'mode' } as const, STATE0.mode)
const VIEW = atom({ plugin: 'adage', key: 'view' } as const, STATE0.view)
const MAP = atom({ plugin: 'adage', key: 'map' } as const, STATE0.map)
const TURN = atom({ plugin: 'adage', key: 'turn' } as const, STATE0.turn)
const PRESS = atom({ plugin: 'adage', key: 'press' } as const, STATE0.press)
const ASKED = atom({ plugin: 'adage', key: 'asked' } as const, STATE0.asked)
const ALLOW = atom({ plugin: 'adage', key: 'allow' } as const, STATE0.allow)
const SKILLS = atom({ plugin: 'adage', key: 'skills' } as const, STATE0.skills)
const CANDIDATES = atom({ plugin: 'adage', key: 'candidates' } as const, STATE0.candidates)

/** What one load of the module keeps between hooks (mod.ts `Runtime`). */
const rt = runtime()

/** The capabilities over this `$`, the runtime and the options: what every hook function is handed. */
function mod($: EngineInterface, options: Options): Mod {
  return {
    rt,
    options,
    state: {
      fired: { read: () => read($, FIRED), update: f => update($, FIRED, f) },
      predicted: { read: () => read($, PREDICTED), update: f => update($, PREDICTED, f) },
      marks: { read: () => read($, MARKS), update: f => update($, MARKS, f) },
      batches: { read: () => read($, BATCHES), update: f => update($, BATCHES, f) },
      feed: { read: () => read($, FEED), update: f => update($, FEED, f) },
      rows: { read: () => read($, ROWS), update: f => update($, ROWS, f) },
      ticks: { read: () => read($, TICKS), update: f => update($, TICKS, f) },
      health: { read: () => read($, HEALTH), update: f => update($, HEALTH, f) },
      learner: { read: () => read($, LEARNER), update: f => update($, LEARNER, f) },
      reflexes: { read: () => read($, REFLEXES), update: f => update($, REFLEXES, f) },
      checks: { read: () => read($, CHECKS), update: f => update($, CHECKS, f) },
      lapsed: { read: () => read($, LAPSED), update: f => update($, LAPSED, f) },
      packs: { read: () => read($, PACKS), update: f => update($, PACKS, f) },
      mode: { read: () => read($, MODE), update: f => update($, MODE, f) },
      view: { read: () => read($, VIEW), update: f => update($, VIEW, f) },
      map: { read: () => read($, MAP), update: f => update($, MAP, f) },
      turn: { read: () => read($, TURN), update: f => update($, TURN, f) },
      press: { read: () => read($, PRESS), update: f => update($, PRESS, f) },
      asked: { read: () => read($, ASKED), update: f => update($, ASKED, f) },
      allow: { read: () => read($, ALLOW), update: f => update($, ALLOW, f) },
      skills: { read: () => read($, SKILLS), update: f => update($, SKILLS, f) },
      candidates: { read: () => read($, CANDIDATES), update: f => update($, CANDIDATES, f) },
    },
    // The store table (plugin.ts `STORE_KEYS`): one cell per `$.store` key.
    store: {
      tab: { get: () => $.store.get('adage.tab'), set: v => $.store.set('adage.tab', v) },
      harness: { get: () => $.store.get('adage.harness'), set: v => $.store.set('adage.harness', v) },
      asked: { get: () => $.store.get('adage.asked'), set: v => $.store.set('adage.asked', v) },
      kept: { get: () => $.store.get('adage.kept'), set: v => $.store.set('adage.kept', v) },
      latest: { get: () => $.store.get('adage.latest'), set: v => $.store.set('adage.latest', v) },
    },
    // The only ways to a process, each behind its allow-list and CLAUDECODE=1.
    run: guard((argv, init) => $.process.run(argv, init)),
    spawn: guardSpawn((request: ProcessSpawnRequest) => $.process.spawn(request)),
    pressRun: guardPress((argv, init) => $.process.run(argv, init)),
    env: {
      pluginData: () => $.env.get('CLAUDE_PLUGIN_DATA'),
      configDir: () => $.env.get('CLAUDE_CONFIG_DIR'),
      home: () => $.env.get('HOME'),
      userProfile: () => $.env.get('USERPROFILE'),
      observe: () => $.env.get('ADAGE_MOD_OBSERVE'),
      ascii: () => $.env.get('ADAGE_ASCII'),
      reducedMotion: () => $.env.get('ADAGE_REDUCED_MOTION'),
      os: () => $.env.get('OS'),
    },
    plugin: { root: () => $.plugin.root, name: () => $.plugin.name },
    fs: { read: path => $.fs.read(path), exists: path => $.fs.exists(path) },
    command: { register: c => $.command.register(c), list: () => $.command.list() },
    session: { id: () => $.session.id(), cwd: () => $.session.cwd(), repo: () => $.session.repo(), usage: a => $.session.usage(a), version: () => $.session.version() },
    clock: { now: () => $.clock.now(), every: (ms, fn) => $.clock.every(ms, fn), after: (ms, fn) => $.clock.after(ms, fn) },
    checkBash: async command => (await $.tool.check({ tool: 'Bash', input: { command } })).decision,
    ui: {
      status: text => $.ui.status(text),
      toast: text => $.ui.toast(text),
      notice: (id, text) => $.ui.notice(id, text),
      open: pane => $.ui.open(pane),
      close: pane => $.ui.close(pane),
      ask: (question, answers) => $.ui.ask(question, answers),
      copy: args => $.ui.copy(args),
      scroll: args => $.ui.scroll(args),
      invalidate: () => $.ui.invalidate('ui.render'),
      resolve: e => $.ui.resolve(e) as Els,
    },
  }
}

export const register: Register = (on, options) => {
  on('session.start', async ($, e, next) => session.onStart(mod($, options), e, next))
  on('classic.SessionStart', async ($, e, next) => session.onShimStart(mod($, options), e, next))
  on('classic.UserPromptSubmit', async ($, e, next) => observe.onPrompt(mod($, options), e, next))
  on('classic.PostToolBatch', async ($, e, next) => observe.onToolBatch(mod($, options), e, next))
  on('classic.PostToolUseFailure', async ($, e, next) => observe.onToolFailure(mod($, options), e, next))
  on('classic.Stop', async ($, e, next) => observe.onStop(mod($, options), e, next))
  on('classic.PreToolUse', async ($, e, next) => observe.onPreToolUse(mod($, options), e, next))
  on('turn.step', async function* ($, e, next) { return yield* step.onStep(mod($, options), e, next) })
  on('turn.complete', async ($, e, next) => ledger.onTurnEnd(mod($, options), e, next))
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => call.onBashCall(mod($, options), e, next))
  on('tool.call', async ($, e, next) => call.onPredictedCall(mod($, options), e, next))
  on('prompt.compose', async ($, e, next) => attach.onCompose(mod($, options), e, next))
  on('tool.describe', { tool: 'Bash' }, async ($, e, next) => attach.onDescribe(mod($, options), e, next))
  on('session.compact', async ($, e, next) => attach.onCompact(mod($, options), e, next))
  on('agent.spawn', async ($, e, next) => attach.onSpawn(mod($, options), e, next))
  on('command.run', { command: 'adage' }, async ($, e) => commands.onCommand(mod($, options), e))
  on('ui.render', { component: 'InfoNotice' }, async ($, e, next) => sites.onInfoNotice(mod($, options), e, next))
  on('ui.render', { component: 'UserMessage' }, async ($, e, next) => sites.onUserMessage(mod($, options), e, next))
  on('ui.render', { component: 'ToolUse' }, async ($, e, next) => sites.onToolUse(mod($, options), e, next))
  on('ui.render', { component: 'ToolResult' }, async ($, e, next) => sites.onToolResult(mod($, options), e, next))
  on('ui.render', { component: 'Spinner' }, async ($, e, next) => sites.onSpinner(mod($, options), e, next))
  on('ui.render', { component: 'TurnDuration' }, async ($, e, next) => sites.onTurnDuration(mod($, options), e, next))
  if (options.band !== false) on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => sites.onAbovePrompt(mod($, options), e, next))
  on('ui.render', { component: 'Pane', requestId: 'adage' }, async ($, e, next) => sites.onPane(mod($, options), e, next))
  on('ui.render', { component: 'Pane', requestId: 'adage-inspect' }, async ($, e, next) => sites.onInspectPane(mod($, options), e, next))
  on('ui.render', { component: 'CommandOutput', props: { command: 'adage' } }, async ($, e, next) => sites.onCommandOutput(mod($, options), e, next))
  on('session.start', { isInteractive: [true, false] }, async ($, e, next) => dialog.onStart(mod($, options), e, next))
  on('classic.SessionStart', { hook_event_name: 'SessionStart' }, async ($, e, next) => dialog.onShimStart(mod($, options), e, next))
  on('turn.complete', { reason: 'answer' }, async ($, e, next) => dialog.onTurnEnd(mod($, options), e, next))
  on('skill.prompt', async ($, e, next) => skill.onSkillPrompt(mod($, options), e, next))
  on('ui.render', { component: 'ToolGroup' }, async ($, e, next) => sites.onToolGroup(mod($, options), e, next))
  on('ui.render', { component: 'ToolUse', props: { tool: 'Skill' } }, async ($, e, next) => sites.onSkillToolUse(mod($, options), e, next))
  on('ui.render', { component: 'Pane', requestId: 'adage-allow' }, async ($, e, next) => sites.onAllowPane(mod($, options), e, next))
  on('ui.open', { id: 'adage' }, async ($, e, next) => actions.onPaneOpen(mod($, options), e, next))
  on('ui.close', { id: 'adage' }, async ($, e, next) => actions.onPaneClose(mod($, options), e, next))
}
