// What `/adage` and `/adage why` answer, as text (docs/claude-mod/ux-design.md §7, §4.8): the status
// card with its key column and `next`, and the last directive (or the steps of a skill Adage
// verified). The text is the source a headless session and the VS Code panel read; draw/card.tsx
// draws the same text as a tree.
import type { Card } from '../reads'
import type { Reflex, Row, Skill } from '../plugin'
import { g, reflexOf } from './text'

/**
 * `/adage why` after a verified skill (ux-design §4.8, `cmd.why.step`): each step the replay
 * verified here, `s{i} {command} · works | stale: {signature} · {date}`.
 */
export function whySkill(skill: Skill, ascii: boolean): string {
  const sep = ` ${g('sep', ascii)} `
  const works = skill.steps.filter(s => s.result === 'works').length
  const lines = [`${g('skill', ascii)} adage verified ${works} of ${skill.steps.length} steps of the ${skill.skill} skill here`, '']
  skill.steps.forEach((s, k) => {
    const state = s.result === 'works' ? 'works' : `stale${s.signature === '' ? '' : `: ${s.signature}`}`
    lines.push([`s${k + 1} ${s.command}`, state, skill.date].filter(p => p !== '').join(sep))
  })
  return lines.join('\n')
}

/**
 * `/adage why`: the last directive this session, as the model read it, with its route, track,
 * evidence and whether it was followed; after a skill Adage verified in place (`skill`, the last
 * one), its steps; with nothing fired, in either arm, `cmd.why.none`.
 */
export function whyText(rows: readonly Row[], reflexes: readonly Reflex[], ascii: boolean, skill: Skill | undefined): string {
  for (let i = rows.length - 1; i >= 0; i--) {
    const row = rows[i]
    if (row === undefined || !row.mine) continue
    const directed = [...row.acts].reverse().find(a => a.act === 'directed' && a.route)
    if (directed?.route === undefined) continue
    if (skill !== undefined && skill.at >= row.ts) return whySkill(skill, ascii)
    const route = directed.route
    const text = row.texts.find(t => t.route === route)?.text ?? ''
    const r = reflexOf(reflexes, route)
    const followed = rows.slice(i).some(x => x.mine && x.acts.some(a => (a.act === 'followed' || a.act === 'met') && a.route === route))
    const sep = ` ${g('sep', ascii)} `
    const lines = [`${g('reflex', ascii)} ${r?.gist || r?.summary || route}`]
    if (text !== '') lines.push('', text.trim())
    lines.push('', [`route ${r !== undefined ? `${r.track}/` : ''}${route}`, r?.describe ?? '', followed ? 'followed' : 'not followed yet'].filter(p => p !== '').join(sep))
    return lines.join('\n')
  }
  if (skill !== undefined) return whySkill(skill, ascii)
  return 'No reflex has spoken in this session yet.'
}

const KEY_WIDTH = 12

/**
 * The card as text, the form `/adage` answers everywhere (and the only form a headless session or
 * the VS Code panel shows): the header, the key column, `next`. The verbs `/adage` takes close it.
 */
export function cardText(card: Card, only?: readonly string[]): string {
  const lines = [card.header]
  let keep = only === undefined
  for (const r of card.rows) {
    if (only !== undefined && r.key !== undefined) keep = r.key !== '' ? only.includes(r.key) : keep
    if (!keep) continue
    lines.push(r.key === undefined ? r.text : `${r.key.padEnd(KEY_WIDTH)}${r.text}`)
  }
  if (only === undefined) {
    card.next.forEach((n, k) => lines.push(`${(k === 0 ? 'next' : '').padEnd(KEY_WIDTH)}${n.command} · ${n.why}`))
    lines.push(`${''.padEnd(KEY_WIDTH)}/adage graph · watch · reflexes · health · why · map · pause · off · on`)
  }
  return lines.join('\n')
}

/** The card's text read back into its parts, for the `CommandOutput` tree (the inverse of `cardText`). */
export function parseCardText(text: string): { header: string; rows: { key: string; text: string }[] } {
  const [header = '', ...rest] = text.split('\n')
  return {
    header,
    rows: rest.map(l => (l.length > KEY_WIDTH && /^\S/.test(l) && l.slice(0, KEY_WIDTH).trimEnd() !== '' && !l.slice(0, KEY_WIDTH).trimEnd().includes(' ')
      ? { key: l.slice(0, KEY_WIDTH).trimEnd(), text: l.slice(KEY_WIDTH) }
      : { key: '', text: l.startsWith(' '.repeat(KEY_WIDTH)) ? l.slice(KEY_WIDTH) : l })),
  }
}

/** The card rows `/adage savings` shows: what the binary measured, as the card words it. */
export const SAVINGS_ROWS = ['activity', 'rewrites', 'savings']
