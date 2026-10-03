// Pure: where Adage's standing text is attached (docs/claude-mod/architecture.md §1b.3, §3.6–§3.8;
// plan C5, C9). The binary answers `Standing` once per session (`{ standing, environment, compact }`,
// and `subagent` when asked about a subagent); the mod places each part where the engine keeps it:
// the procedures as a `session` section of the system prompt (cached for the session), the
// environment note in the Bash tool's description, the compaction instruction at compaction, and a
// subagent's text before its prompt. Every part is the binary's words; an empty part attaches
// nothing, and a failed ask is an empty answer.
import type { PromptComposeSection } from 'claude-code'

/** The session's answer, kept for the session: the same text at every request (the cache holds it). */
export type Standing = { standing: string; environment: string; compact: string }

export const NO_STANDING: Standing = { standing: '', environment: '', compact: '' }

/** The binary's `Standing` payload (C5, C9), exactly. */
export type StandingPayload = {
  session_id: string
  cwd: string
  hook_event_name: 'Standing'
  via: 'mod'
  /** Observe mode: the binary answers as for the control arm (only `compact`). */
  observe?: true
  /** A subagent about to start (`agent.spawn`): what it is and what it is asked. */
  agent?: { type: string; description: string; prompt: string }
}

/** `Standing` is read-only and asked off the hot path (once a session, once a spawn). */
export const STANDING_TIMEOUT_MS = 2000

/** The section's id: a plugin's own is `<plugin>:<name>` (the engine's rule). */
export const SECTION_ID = 'adage:procedures'

const isRecord = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown): string => (typeof v === 'string' ? v : '')

/** The answer's three session parts; anything missing or malformed is empty. */
export function standingOf(answer: unknown): Standing {
  if (!isRecord(answer)) return NO_STANDING
  return { standing: str(answer.standing), environment: str(answer.environment), compact: str(answer.compact) }
}

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
