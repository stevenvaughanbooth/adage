// The adage mod (POC: plan B1, the B2 observers, C4, C7, C8). One copy ships in each plugin; at
// session.start each probes the hook binary its shim linked, and the copy whose link is
// scripts/quiet stays inert: every hook then answers what next(e) answered, unchanged.
//
// Every `$` call of the mod is in this file: `claude plugin validate` follows `$` only into
// functions declared in the same file, so bridge.ts, step.ts, predict.ts and reflex.ts take plain
// values and closures and are pure enough to test without the engine.
import { update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import { ask, guard, hookBinary, probe } from './bridge'
import type { Asked, Run } from './bridge'
import type { Predicted } from './plugin'
import { parseReflexes } from './reflex'
import { TURN_STEP_TIMEOUT_MS, turnStep } from './step'

// The $.state values this copy keeps (types/index.d.ts). `claude plugin validate` reads a state
// reference only as literals in a const of this file, so the plugin name is spelled here (and is
// the one line of this file the adage-exec copy changes).
const FIRED = { plugin: 'adage', key: 'fired' } as const
const PREDICTED = { plugin: 'adage', key: 'predicted' } as const

type Answered = { additionalContext?: string[]; block?: string }

/** The hook binary that answered the probe; undefined while this copy is inert. */
let bin: string | undefined

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
  bin = await probe(runner($), hookBinary(pluginData, $.plugin.root))
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
      ask: payload => askBinary($, 'TurnStep', payload, TURN_STEP_TIMEOUT_MS),
      context: async () => ({ sessionId: await $.session.id(), cwd: await $.session.cwd() }),
      record: p => record($, p),
    })
  })
}
