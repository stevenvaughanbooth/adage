// Observers of the command hooks (docs/claude-mod/architecture.md §3.1): the command hooks beneath
// answer once; each observer returns that answer unchanged and keeps, for drawing, what it
// delivered: the `<adage-reflex>` blocks (reflex.ts reads their tags), the tool batch the hooks saw,
// the rewrite and the receipt the PreToolUse hook made of a Bash call; and, for the Graph tab, each
// prompt, main-loop batch, failed call, reflex and stop as a node of this session's graph (dag.ts).
// Nothing is kept while the mod is inert.
import { keepNodes, toolsNode } from './dag'
import { keepBatch, keepFired, mark } from './marks'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import type { Fired } from './plugin'
import { parseReflexes } from './reflex'
import { receiptOf, rewriteOf } from './view/lines'
import { hintsOf } from './world/hints'

type Answered = { additionalContext?: readonly string[]; block?: string }

/**
 * The answer, unchanged, with the reflexes it delivered kept (each with `extra`) and, on the main
 * loop (`graph`), drawn as nodes of the session's graph after the calls they followed.
 */
async function kept<A extends Answered>(m: Mod, answered: A, event: string, extra: Partial<Fired> = {}, graph = true): Promise<A> {
  if (m.rt.bin === undefined) return answered
  const fired = parseReflexes([...(answered.additionalContext ?? []), answered.block], event).map(f => ({ ...f, ...extra }))
  if (fired.length === 0) return answered
  await keepFired(m, fired).catch(() => undefined)
  if (graph) {
    const at = await m.clock.now()
    await keepNodes(m, fired.map(f => ({ kind: 'reflex' as const, at, event, route: f.route, track: f.track, strength: f.strength, ids: f.ids ?? [] }))).catch(() => undefined)
  }
  return answered
}

/**
 * A prompt the person (or the SDK) submitted on the main loop starts a turn: the turn's counts begin
 * where the lists stand, and the version line, shown until the first prompt, is cleared for good.
 */
export async function onPrompt(m: Mod, e: EventAt<'classic.UserPromptSubmit'>, next: NextAt<'classic.UserPromptSubmit'>): AnswerAt<'classic.UserPromptSubmit'> {
  const answered = await next(e)
  if (m.rt.bin === undefined || e.agent_id !== undefined || (e.source !== undefined && e.source !== 'user' && e.source !== 'sdk')) return answered
  const fired = await m.state.fired.read()
  const predicted = await m.state.predicted.read()
  const marks = await m.state.marks.read()
  const turn = await m.state.turn.update(t => ({ submits: t.submits + 1, fired: fired.length, predicted: predicted.length, marks: marks.length }))
  // The world's hint, when the command hook delivered one: kept for the World tab, never changed.
  const hints = hintsOf(answered.additionalContext ?? [], await m.clock.now(), turn.submits)
  if (hints.length > 0) await m.state.world.update(w => ({ ...w, hints: [...w.hints, ...hints].slice(-50) })).catch(() => undefined)
  await keepNodes(m, [{ kind: 'prompt', at: await m.clock.now(), n: turn.submits, text: e.prompt }]).catch(() => undefined)
  if ((await m.state.mode.read()).versionLine !== '') {
    const { version } = await m.session.version()
    await m.store.harness.set(version).catch(() => undefined)
    await m.state.mode.update(x => ({ ...x, versionLine: '' }))
  }
  return kept(m, answered, 'UserPromptSubmit', { prompt: turn.submits, promptText: e.prompt })
}

/** A tool batch: its calls and when the hooks beneath ran, for matching a feed judgement to its row. */
export async function onToolBatch(m: Mod, e: EventAt<'classic.PostToolBatch'>, next: NextAt<'classic.PostToolBatch'>): AnswerAt<'classic.PostToolBatch'> {
  const from = m.rt.bin === undefined ? 0 : await m.clock.now()
  const answered = await next(e)
  if (m.rt.bin === undefined) return answered
  const to = await m.clock.now()
  const ids = e.tool_calls.map(c => c.tool_use_id).filter(id => typeof id === 'string' && id !== '')
  if (ids.length > 0) await keepBatch(m, { ids, from, to }).catch(() => undefined)
  // A subagent's own calls are its transcript's, not this one's: the graph draws the main loop.
  const main = e.agent_id === undefined
  const node = main ? toolsNode(e.tool_calls, from, to) : undefined
  if (node !== undefined) await keepNodes(m, [node]).catch(() => undefined)
  return kept(m, answered, 'PostToolBatch', ids.length > 0 ? { ids } : {}, main)
}

export async function onToolFailure(m: Mod, e: EventAt<'classic.PostToolUseFailure'>, next: NextAt<'classic.PostToolUseFailure'>): AnswerAt<'classic.PostToolUseFailure'> {
  const answered = await next(e)
  const main = e.agent_id === undefined
  if (m.rt.bin !== undefined && main && e.tool_use_id) await keepNodes(m, [{ kind: 'failed', at: await m.clock.now(), id: e.tool_use_id }]).catch(() => undefined)
  return kept(m, answered, 'PostToolUseFailure', e.tool_use_id ? { ids: [e.tool_use_id] } : {}, main)
}

/**
 * The finish line asked the agent to continue (`toast.gate`): said once per gate, not to the model.
 * The stop is a node of the graph (the turn's end, or the gate), before the reflexes it delivered.
 */
export async function onStop(m: Mod, e: EventAt<'classic.Stop'>, next: NextAt<'classic.Stop'>): AnswerAt<'classic.Stop'> {
  const said = await next(e)
  if (m.rt.bin !== undefined) await keepNodes(m, [{ kind: 'stop', at: await m.clock.now(), gate: said.block !== undefined }]).catch(() => undefined)
  const answered = await kept(m, said, 'Stop')
  if (m.rt.bin !== undefined && m.rt.interactive && answered.block !== undefined) m.ui.toast('adage: finish line · the agent continues once')
  return answered
}

/**
 * What the PreToolUse command hook did to a Bash call (a rewrite to the project's interpreter, a
 * receipt delivered beside it): kept for the marker under its row. The answer is unchanged.
 */
export async function onPreToolUse(m: Mod, e: EventAt<'classic.PreToolUse'>, next: NextAt<'classic.PreToolUse'>): AnswerAt<'classic.PreToolUse'> {
  const answered = await next(e)
  if (m.rt.bin === undefined || e.tool !== 'Bash') return answered
  const rw = rewriteOf(e.command, answered.updatedInput?.command)
  if (rw !== undefined) await mark(m, { id: e.tool_use_id, kind: 'rewrite', ...rw }).catch(() => undefined)
  const rc = receiptOf(answered.additionalContext ?? [])
  if (rc !== undefined) await mark(m, { id: e.tool_use_id, kind: 'receipt', receipt: rc }).catch(() => undefined)
  return answered
}
