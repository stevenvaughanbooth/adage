// What D5 draws (plan D5; ux-design §4.7, §4.8, §10): the noticed line under a tool batch, the
// skill markers, and the readers of what the binary says about them (`status --json`'s
// candidates, a `SkillPrompt` answer). Pure: register.ts reads the state and these word the lines.
import type { Candidate, Skill } from './kinds'
import { MAX_SKILLS } from './kinds'
import type { Act, Batch, Row } from './plugin'
import { g } from './view'

const isRecord = (v: unknown): v is Record<string, unknown> => v !== null && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown): string => (typeof v === 'string' ? v : '')

/**
 * The candidates of `adage status --json` (plan D1), as the noticed line words them: what the
 * sequence would tell the agent (the gist), never the id; and whether it is a sequence (a context
 * of two or more events), the only kind the line is drawn for.
 */
export function candidatesOf(status: Record<string, unknown>): Candidate[] {
  const list = Array.isArray(status.candidates) ? status.candidates.filter(isRecord) : []
  return list
    .map(c => {
      const trigger = isRecord(c.trigger) ? c.trigger : {}
      return { id: str(c.id), say: str(c.gist) || str(c.summary), sequence: str(trigger.kind) === 'context' }
    })
    .filter(c => c.id !== '' && c.say !== '')
}

/** `noticed.forming` / `noticed.review`: plain counts, never a forecast of when a reflex forms. */
export function noticedLine(a: Act, say: string, ascii: boolean): string {
  const sep = g('sep', ascii)
  const head = `${g('off', ascii)} adage noticed ${sep} ${say}`
  if (a.review === true) return `${head} ${sep} waits for your review ${sep} /adage review`
  const n = a.support ?? 0
  return `${head} ${sep} seen in ${n} earlier ${n === 1 ? 'session' : 'sessions'}`
}

/** Whether a feed row's batch is the group drawn: by the calls it names, else by when its hooks ran. */
function belongs(row: Row, ids: readonly string[], batches: readonly Batch[]): boolean {
  if (row.ids !== undefined && row.ids.length > 0) return row.ids.some(id => ids.includes(id))
  const b = [...batches].reverse().find(x => row.ts >= x.from - 50 && row.ts <= x.to + 2000)
  return b !== undefined && b.ids.some(id => ids.includes(id))
}

/**
 * The noticed lines under one tool group (`ToolGroup`'s calls): one per candidate first noticed in
 * a batch of this group, worded from `status --json`'s candidates. A candidate already noticed in
 * an earlier batch, one the read does not know yet, or one that is not a sequence draws nothing.
 */
export function noticedLines(rows: readonly Row[], ids: readonly string[], batches: readonly Batch[], candidates: readonly Candidate[], ascii: boolean): string[] {
  const seen = new Set<string>()
  const lines: string[] = []
  for (const row of rows) {
    if (!row.mine) continue
    const here = belongs(row, ids, batches)
    for (const a of row.acts) {
      if (a.act !== 'noticed' || a.candidate === undefined || seen.has(a.candidate)) continue
      seen.add(a.candidate)
      if (!here) continue
      const c = candidates.find(x => x.id === a.candidate)
      if (c === undefined || !c.sequence) continue
      lines.push(noticedLine(a, c.say, ascii))
    }
  }
  return lines
}

/** The candidates noticed in this session's rows: the ids, each once. */
export function noticedIds(rows: readonly Row[]): string[] {
  const ids = new Set<string>()
  for (const row of rows) if (row.mine) for (const a of row.acts) if (a.act === 'noticed' && a.candidate !== undefined) ids.add(a.candidate)
  return [...ids]
}

/** The heading `SkillPrompt` writes above the verified steps (`adage_hook::skill::HEADING`). */
const HEADING = /^Adage verified these steps in this repository on (\d{4}-\d{2}-\d{2}):$/m

/**
 * The steps a `SkillPrompt` answer lists (plan D4): one line per step the replay verified
 * (`- make test: works`) or found stale (`- make suite: stale (exit 2: …)`), after the heading.
 * Undefined when the text carries no heading.
 */
export function skillOf(skill: string, text: string, now: number): Skill | undefined {
  const m = HEADING.exec(text)
  if (m === null || m.index === undefined) return undefined
  const date = m[1] ?? ''
  const steps: Skill['steps'] = []
  for (const line of text.slice(m.index + m[0].length).split('\n')) {
    if (!line.startsWith('- ')) {
      if (steps.length > 0) break
      continue
    }
    const step = /^- (.+?): (works|stale)(?: \((.*)\))?$/.exec(line)
    if (step === null) break
    steps.push({ command: step[1] ?? '', result: step[2] === 'works' ? 'works' : 'stale', signature: step[3] ?? '' })
  }
  return steps.length === 0 ? undefined : { skill, at: now, date, steps }
}

/** The skills kept for the markers and `/adage why`: the latest answer per skill, at most `MAX_SKILLS`. */
export function withSkill(skills: readonly Skill[], s: Skill): Skill[] {
  return [...skills.filter(x => x.skill !== s.skill), s].slice(-MAX_SKILLS)
}

/** `tool.skill.verified`: `◈ adage verified {k} of {n} steps here{ · step {i} stale} · /adage why`. */
export function verifiedMark(s: Skill, ascii: boolean): string {
  const sep = ` ${g('sep', ascii)} `
  const works = s.steps.filter(x => x.result === 'works').length
  const stale = s.steps.map((x, k) => (x.result === 'stale' ? `step ${k + 1} stale` : '')).filter(x => x !== '')
  return `${g('skill', ascii)} adage verified ${works} of ${s.steps.length} steps here${stale.length === 0 ? '' : sep + stale.join(sep)}${sep}/adage why`
}

/** `tool.skill.invoked`: the Skill call Adage made in the model's place. */
export function invokedMark(skill: string, ascii: boolean): string {
  return `${g('skill', ascii)} adage loaded the ${skill} skill ${g('sep', ascii)} no model request`
}
