// Pure: where Adage's standing text is attached (docs/claude-mod/architecture.md §1b.3, §3.6–§3.8;
// plan C5, C9). The binary answers `Standing` once per session (`{ standing, environment, compact,
// conventions }`, and `subagent` when asked about a subagent); the mod places each part where the
// engine keeps it: the procedures as a `session` section of the system prompt (cached for the
// session), the environment note and the learned lookup conventions in the Bash tool's
// description, the compaction instruction at compaction, and a subagent's text before its prompt. Every part is the binary's words; an empty part attaches
// nothing, and a failed ask is an empty answer. The answer is asked once per session id and kept
// in the runtime, so every request of the session reads the same text (the prompt cache keeps it).
import type { PromptComposeSection } from 'claude-code'

import { askBinary } from './binary'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import { isRecord, str } from './shape'

/** The session's answer, kept for the session: the same text at every request (the cache holds it). */
export type Standing = { standing: string; environment: string; compact: string; conventions: string }

const NO_STANDING: Standing = { standing: '', environment: '', compact: '', conventions: '' }

/** The binary's `Standing` payload (C5, C9), exactly. */
type StandingPayload = {
  session_id: string
  cwd: string
  hook_event_name: 'Standing'
  via: 'mod'
  /** Observe mode: the binary answers as for the control arm (only `compact`). */
  observe?: true
  /** A subagent about to start (`agent.spawn`): what it is and what it is asked. */
  agent?: { type: string; description: string; prompt: string }
  /**
   * The session's ask: the binary adds `conventions`, one sentence per learned lookup convention
   * (O0-fixes-notes.md §6); without it the answer is as before.
   */
  conventions?: true
}

/** `Standing` is read-only and asked off the hot path (once a session, once a spawn). */
const STANDING_TIMEOUT_MS = 2000

/** The section's id: a plugin's own is `<plugin>:<name>` (the engine's rule). */
export const SECTION_ID = 'adage:procedures'

/** The answer's four session parts; anything missing or malformed is empty. */
export function standingOf(answer: unknown): Standing {
  if (!isRecord(answer)) return NO_STANDING
  return { standing: str(answer.standing), environment: str(answer.environment), compact: str(answer.compact), conventions: str(answer.conventions) }
}

/** What the Bash tool's description carries: the environment note, then the lookup conventions. */
export const describedNote = (s: Standing): string => [s.environment, s.conventions].filter(t => t !== '').join('\n\n')

/** What a subagent is told before it starts (the answer's `subagent`), or empty. */
export const subagentOf = (answer: unknown): string => (isRecord(answer) ? str(answer.subagent) : '')

/**
 * The system prompt's sections with the procedures appended as one `session` section (after every
 * `shared` one, as the engine requires), or unchanged when there is none. A section of the same id
 * already in the list is replaced, never doubled.
 */
export function withSection(sections: readonly PromptComposeSection[], standing: string): readonly PromptComposeSection[] {
  if (standing === '') return sections
  return [...sections.filter(s => s.id !== SECTION_ID), { id: SECTION_ID, text: standing, scope: 'session' }]
}

/** The Bash tool's description with the environment note after it, once; unchanged without one. */
export function withNote(description: string, note: string): string {
  if (note === '' || description.includes(note)) return description
  return description === '' ? note : `${description}\n\n${note}`
}

/** What compaction is told to keep: the person's own words first, then Adage's line, once. */
export function withInstruction(instructions: string | undefined, compact: string): string | undefined {
  if (compact === '' || (instructions ?? '').includes(compact)) return instructions
  return [instructions, compact].filter((s): s is string => s !== undefined && s !== '').join('\n')
}

/** A subagent's prompt with what it is told before it: the marked blocks, then its task. */
export const withPreamble = (prompt: string, subagent: string): string => (subagent === '' ? prompt : `${subagent}\n\n${prompt}`)

/**
 * Whether a transcript row is an attachment Adage's plugin made, by structure: the row's own
 * `origin` names this plugin. Never by its text (a row quoting `<adage-reflex` is the person's or
 * the model's own). Claude Code 2.1.288's rows carry no origin, so nothing is removed there (Q5).
 */
export function isAdageAttachment(message: unknown): boolean {
  if (!isRecord(message) || !isRecord(message.origin)) return false
  const o = message.origin
  return o.kind === 'plugin' && o.plugin === 'adage'
}

/** The messages compaction runs over, without Adage's own attachment rows when they are addressable. */
export function withoutAttachments<M>(messages: readonly M[]): readonly M[] {
  return messages.some(isAdageAttachment) ? messages.filter(m => !isAdageAttachment(m)) : messages
}

/** The `Standing` payload for this session, and for a subagent about to start. */
async function standingPayload(m: Mod, agent?: StandingPayload['agent']): Promise<StandingPayload> {
  return {
    session_id: await m.session.id(),
    cwd: await m.session.cwd(),
    hook_event_name: 'Standing',
    via: 'mod',
    ...(m.rt.observing ? { observe: true as const } : {}),
    ...(agent === undefined ? { conventions: true as const } : { agent }),
  }
}

/**
 * The session's standing text, asked once per session id; nothing while the mod is inert (the
 * question waits for a binary, so a first session whose link the shim makes later still asks).
 */
export async function standing(m: Mod): Promise<Standing> {
  if (m.rt.bin === undefined) return NO_STANDING
  const session = await m.session.id()
  if (m.rt.standingFor?.session !== session) {
    const answer = standingPayload(m)
      .then(p => askBinary(m, 'Standing', p, STANDING_TIMEOUT_MS))
      .then(a => (a.ok ? standingOf(a.answer) : NO_STANDING), () => NO_STANDING)
    m.rt.standingFor = { session, answer }
  }
  return m.rt.standingFor.answer
}

/** Standing text once, from the cache (§3.6, C5): the procedures as one `session` section of the system prompt. */
export async function onCompose(m: Mod, e: EventAt<'prompt.compose'>, next: NextAt<'prompt.compose'>): AnswerAt<'prompt.compose'> {
  const composed = await next(e)
  if (m.rt.bin === undefined) return composed
  const text = (await standing(m).catch(() => NO_STANDING)).standing
  return text === '' ? composed : { ...composed, sections: withSection(composed.sections, text) }
}

/**
 * The Bash tool's description carries the environment note and the lookup conventions once
 * (§1b.3, C9): how Python and the tests run here, and how work items are looked up, where the tool
 * is described rather than at each call.
 */
export async function onDescribe(m: Mod, e: EventAt<'tool.describe', { tool: 'Bash' }>, next: NextAt<'tool.describe', { tool: 'Bash' }>): AnswerAt<'tool.describe', { tool: 'Bash' }> {
  const described = await next(e)
  if (m.rt.bin === undefined) return described
  const note = describedNote(await standing(m).catch(() => NO_STANDING))
  const description = withNote(described.description, note)
  return description === described.description ? described : { ...described, description }
}

/**
 * Compaction keeps what the session followed (§3.7, C5): the binary's instruction, in both arms;
 * Adage's own attachment rows go only where the engine marks them as the plugin's.
 */
export async function onCompact(m: Mod, e: EventAt<'session.compact'>, next: NextAt<'session.compact'>): AnswerAt<'session.compact'> {
  if (m.rt.bin === undefined) return next(e)
  const compact = (await standing(m).catch(() => NO_STANDING)).compact
  const instructions = withInstruction(typeof e.instructions === 'string' ? e.instructions : undefined, compact)
  // A transcript that is not a list (a host that hands none) is passed on as it came.
  const messages = Array.isArray(e.messages) ? withoutAttachments(e.messages) : e.messages
  if (instructions === e.instructions && messages === e.messages) return next(e)
  return next({ ...e, messages, ...(instructions === undefined ? {} : { instructions }) })
}

/**
 * A subagent is told the conventions its task binds before its first request (§3.8, C9). A fork
 * carries the session's own context already, and is left as it is.
 */
export async function onSpawn(m: Mod, e: EventAt<'agent.spawn'>, next: NextAt<'agent.spawn'>): AnswerAt<'agent.spawn'> {
  if (m.rt.bin === undefined || e.fork) return next(e)
  let told = ''
  try {
    // The session's own answer first: the binary keeps the session's standing text from it.
    await standing(m)
    const asked = await askBinary(m, 'Standing', await standingPayload(m, { type: e.subagentType, description: e.description, prompt: e.prompt }), STANDING_TIMEOUT_MS)
    told = asked.ok ? subagentOf(asked.answer) : ''
  } catch {
    // Told nothing: the subagent starts as the model asked.
  }
  return told === '' ? next(e) : next({ ...e, prompt: withPreamble(e.prompt, told) })
}
