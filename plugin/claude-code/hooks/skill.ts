// A skill as the model reads it, with the steps Adage verified here (architecture §3.11; plan D4,
// D5; ux-design §4.8): `skill.prompt` asks the binary (`SkillPrompt`) for the skill's text with the
// verified steps appended, keeps the steps for the markers under the Skill row and `/adage why`,
// and words those markers.
import { askBinary } from './binary'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import type { Skill } from './plugin'
import { MAX_SKILLS } from './plugin'
import { isRecord } from './shape'
import { g } from './view/text'

/** `SkillPrompt` is asked off the hot path, with a command hook's budget. */
const SKILL_PROMPT_TIMEOUT_MS = 2000

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

/** The skill's text with the steps Adage verified here (`SkillPrompt`), or undefined to leave it as computed. */
async function verifiedSkill(m: Mod, skill: string, text: string): Promise<string | undefined> {
  const payload = {
    session_id: await m.session.id(),
    cwd: await m.session.cwd(),
    hook_event_name: 'SkillPrompt',
    skill,
    text,
    via: 'mod',
    ...(m.rt.observing ? { observe: true as const } : {}),
  }
  const asked = await askBinary(m, 'SkillPrompt', payload, SKILL_PROMPT_TIMEOUT_MS)
  const answered = asked.ok && isRecord(asked.answer) && typeof asked.answer.text === 'string' && asked.answer.text.trim() !== '' ? asked.answer.text : undefined
  if (answered === undefined) return undefined
  const s = skillOf(skill, answered, await m.clock.now())
  if (s !== undefined) {
    m.rt.skill = s
    await m.state.skills.update(cur => withSkill(cur, s)).catch(() => undefined)
  }
  return answered
}

/** `skill.prompt` (§3.11, D4): the binary's text when it verified steps here; the computed text otherwise. */
export async function onSkillPrompt(m: Mod, e: EventAt<'skill.prompt'>, next: NextAt<'skill.prompt'>): AnswerAt<'skill.prompt'> {
  if (m.rt.bin === undefined) return next(e)
  const text = await verifiedSkill(m, e.skill, e.text)
  return text === undefined ? next(e) : { text }
}
