// What `/adage` and `/adage why` answer, as text (view/card.ts; ux-design §7, §4.8; T-B3-09,
// T-D5-03): the status card and its read-back, `/adage savings`, and the last directive or the
// steps of a skill Adage verified.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, CARD, GRAPH, LINE, SKILL_ANNOTATED, T0, rowsOf as rows } from '../fixtures/kit'
import { cardOf, reflexesOf } from '../reads'
import { skillOf } from '../skill'
import { SAVINGS_ROWS, cardText, parseCardText, whySkill, whyText } from './card'

const REFLEXES = reflexesOf(GRAPH)

describe('T-B3-09 · /adage why', () => {
  test('with nothing fired, in either arm, the one sentence', () => {
    expect(whyText([], REFLEXES, false, undefined)).toBe('No reflex has spoken in this session yet.')
    const control = rows(LINE.controlPrompt, LINE.controlStep, LINE.controlJudged, LINE.otherControl)
    expect(whyText(control, REFLEXES, false, undefined)).toBe('No reflex has spoken in this session yet.')
  })

  test('the last directive: what it tells the agent, the text as read, its route and evidence, followed or not', () => {
    expect(whyText(rows(LINE.firedGen, LINE.followedGen), REFLEXES, false, undefined)).toBe(
      '◠ run make gen after a schema edit\n\nIn this repository, changes to schema/*.json must be followed by running:\n  make gen\n\nroute acme/learned/codegen-after-schema · 13 of 17 sessions ran it after the edit · followed',
    )
    expect(whyText(rows(LINE.firedGen), REFLEXES, false, undefined)).toEndWith('not followed yet')
  })
})

describe('T-D5-03 · /adage why after a verified skill (cmd.why.step)', () => {
  test('the skill\'s steps after the last directive; the directive before it; the one sentence with neither', () => {
    const s = skillOf('acme-work-item', SKILL_ANNOTATED, 7)
    if (s === undefined) throw new Error('skill')
    expect(whySkill(s, false)).toBe('◈ adage verified 1 of 2 steps of the acme-work-item skill here\n\ns1 make test · works · 2026-10-03\ns2 make lint · stale: exit 2: no rule · 2026-10-03')
    expect(whyText(rows(LINE.firedGen), reflexesOf({ routes: [] }), false, { ...s, at: T0 + 5000 })).toContain('acme-work-item skill here')
    expect(whyText(rows(LINE.firedGen), reflexesOf({ routes: [] }), false, { ...s, at: 0 })).toStartWith('◠ codegen-after-schema')
    expect(whyText([], [], false, s)).toContain('acme-work-item skill here')
    expect(whyText([], [], false, undefined)).toBe('No reflex has spoken in this session yet.')
    expect(whySkill(s, false)).not.toMatch(BANNED)
  })
})

describe('T-B3-09 · the status card as text', () => {
  test('the header, the key column and next; read back as the tree draws it', () => {
    const card = cardOf(CARD)
    if (card === undefined) throw new Error('card')
    const text = cardText(card)
    expect(text.split('\n')[0]).toBe('adage 0.1.0 · acme@315465905b6b · ● healthy')
    expect(text).toContain('reflexes    4 active: 3 conditioned, 1 instinct')
    expect(text).toContain('next        /adage watch · live rows, reflexes and health')
    const back = parseCardText(text)
    expect(back.header).toBe(card.header)
    expect(back.rows[0]).toEqual({ key: 'reflexes', text: '4 active: 3 conditioned, 1 instinct' })
    expect(back.rows.find(r => r.text.startsWith('over 30'))?.key).toBe('')
  })

  test('/adage savings: what the binary measured, as the card words it', () => {
    const card = cardOf(CARD)
    if (card === undefined) throw new Error('card')
    expect(cardText(card, SAVINGS_ROWS).split('\n')).toEqual([
      card.header,
      'activity    last 14 days: 9 sessions · 12 directives · 10 followed · 1 dead end headed off',
      'savings     measured: 2.1 fewer model requests per session (1.2–3.0)',
      `${' '.repeat(12)}over 30 treated and 10 control sessions`,
    ])
  })
})
