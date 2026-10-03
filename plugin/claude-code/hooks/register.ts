// The adage mod (POC: plan B1, the B2 observers, C4, C6, C7, C8). One copy ships in each plugin; at
// session.start each probes the hook binary its shim linked, and the copy whose link is
// scripts/quiet stays inert: every hook then answers what next(e) answered, unchanged.
//
// Every `$` call of the mod is in this file: `claude plugin validate` follows `$` only into
// functions declared in the same file, so bridge.ts, step.ts, substitute.ts, predict.ts and reflex.ts take plain
// values and closures and are pure enough to test without the engine.
import { update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import { ask, guard, hookBinary, probe } from './bridge'
import type { Asked, Run } from './bridge'
import type { Predicted } from './plugin'
import { parseReflexes } from './reflex'
import { TURN_STEP_TIMEOUT_MS, turnStep } from './step'
import { PRE_TOOL_TIMEOUT_MS, substituteCall } from './substitute'

// The $.state values this copy keeps (types/index.d.ts). `claude plugin validate` reads a state
// reference only as literals in a const of this file, so the plugin name is spelled here (and is
// the one line of this file the adage-exec copy changes).
const FIRED = { plugin: 'adage', key: 'fired' } as const
const PREDICTED = { plugin: 'adage', key: 'predicted' } as const

type Answered = { additionalContext?: string[]; block?: string }

/** The hook binary that answered the probe; undefined while this copy is inert. */
let bin: string | undefined

/**
 * Observe mode (ADAGE_MOD_OBSERVE set and non-empty; the demo's `mod-observe` arm): the binary is
 * still asked at every step and Bash call, with `observe`, and records what it would have done as
 * the control arm does; the mod never yields a predicted call and never replaces a command.
 */
let observing = false

/** `$.process.run` behind the allow-list and CLAUDECODE=1 (bridge.ts `guard`). */
function runner($: EngineInterface): Run {
  return guard((argv, init) => $.process.run(argv, init))
}

/** `ask($, event, payload, timeoutMs)`: one question to the active binary, tagged. */
function askBinary($: EngineInterface, event: string, payload: { cwd?: string }, timeoutMs: number): Promise<Asked> {
  return ask(runner($), bin, event, payload, timeoutMs)
}

async function observe<A extends Answered>($: EngineInterface, answered: A, event: string): Promise<A> {
  if (bin === undefined) return answered
  const fired = parseReflexes([...(answered.additionalContext ?? []), answered.block], event)
  if (fired.length > 0) await update($, FIRED, cur => [...(cur ?? []), ...fired]).catch(() => undefined)
  return answered
}

/** Probes the binary the shim linked; the copy is active only while it answers. */
async function activate($: EngineInterface): Promise<void> {
  const pluginData = await $.env.get('CLAUDE_PLUGIN_DATA')
  observing = ((await $.env.get('ADAGE_MOD_OBSERVE')) ?? '') !== ''
  bin = await probe(runner($), hookBinary(pluginData, $.plugin.root))
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
  await update($, PREDICTED, cur => [...(cur ?? []), p])
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await activate($)
    return started
  })

  // The link is made by the shim, the SessionStart command hook beneath this observer: probe
  // again once it has run, so a first session (no link yet at session.start) and a /clear (the
  // shim relinks) see the binary the shim chose.
  on('classic.SessionStart', async ($, e, next) => {
    const answered = await next(e)
    await activate($)
    return answered
  })

  // Observers of the command hooks (§3.1): the command hooks beneath answer once; the answer is
  // returned unchanged, and the reflexes it delivered are kept for drawing.
  on('classic.UserPromptSubmit', async ($, e, next) => observe($, await next(e), 'UserPromptSubmit'))
  on('classic.PostToolBatch', async ($, e, next) => observe($, await next(e), 'PostToolBatch'))
  on('classic.PostToolUseFailure', async ($, e, next) => observe($, await next(e), 'PostToolUseFailure'))
  on('classic.Stop', async ($, e, next) => observe($, await next(e), 'Stop'))

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
    }),
  )
}
