// A skill with the steps Adage verified here (skill.ts; plan D4, D5; ux-design §4.8; T-D4-04,
// T-D5-03): the answer read back, the markers' words, and `skill.prompt` and the markers through
// the engine.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, LINE, PREDICTED_SKILL_ID, PREDICT_SKILL, SESSION, SKILL_ANNOTATED, SKILL_TEXT, SKILL_TOOL, STEP, SURFACES, drain, start, texts, world } from './fixtures/kit'
import { invokedMark, skillOf, verifiedMark, withSkill } from './skill'

describe('T-D5-03 · skills, as words', () => {
  test('a SkillPrompt answer read back: the date and one step per line', () => {
    const s = skillOf('acme-work-item', SKILL_ANNOTATED, 7)
    expect(s).toEqual({ skill: 'acme-work-item', at: 7, date: '2026-10-03', steps: [{ command: 'scripts/wi-lookup {id}', result: 'works', signature: '' }, { command: 'pytest', result: 'stale', signature: 'exit 1: command not found' }, { command: 'make test', result: 'works', signature: '' }] })
    expect(skillOf('x', SKILL_TEXT, 0)).toBeUndefined()
    expect(skillOf('x', 'Adage verified these steps in this repository on 2026-10-03:\n- make suite: stale\n', 0)?.steps).toEqual([{ command: 'make suite', result: 'stale', signature: '' }])
    if (s === undefined) throw new Error('skill')
    expect(withSkill([s], { ...s, at: 9 })).toEqual([{ ...s, at: 9 }])
  })

  test('tool.skill.invoked and tool.skill.verified', () => {
    const s = skillOf('acme-work-item', SKILL_ANNOTATED, 7)
    if (s === undefined) throw new Error('skill')
    expect(invokedMark('release-checklist', false)).toBe('◈ adage loaded the release-checklist skill · no model request')
    expect(verifiedMark(s, false)).toBe('◈ adage verified 2 of 3 steps here · step 2 stale · /adage why')
    expect(verifiedMark({ ...s, steps: s.steps.filter(x => x.result === 'works') }, true)).toBe('<> adage verified 2 of 2 steps here - /adage why')
    for (const l of [invokedMark('x', false), verifiedMark(s, false)]) expect(l).not.toMatch(BANNED)
  })
})

describe('T-D5-03 · the skill markers through the engine, and T-D4-04 · skill.prompt', () => {
  test('a predicted skill carries one marker; a verified skill its steps; /adage why lists them', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], hook: { TurnStep: PREDICT_SKILL, SkillPrompt: JSON.stringify({ text: SKILL_ANNOTATED }) } })
    on('command.list', () => ({ value: [{ name: 'acme-work-item', description: '', source: 'plugin' as const }] }))
    await start($, w)
    await drain($.turn.step(STEP))
    expect(await $.skill.prompt({ skill: 'acme-work-item', text: SKILL_TEXT })).toEqual({ text: SKILL_ANNOTATED })
    const asked = w.ran.find(r => r.argv[3] === 'SkillPrompt')
    expect(JSON.parse(asked?.init?.stdin ?? '')).toEqual({ session_id: SESSION, cwd: '/w', hook_event_name: 'SkillPrompt', skill: 'acme-work-item', text: SKILL_TEXT, via: 'mod' })
    expect(asked?.init?.timeoutMs).toBe(2000)
    for (const surface of SURFACES) {
      const row = async (id: string, skill?: string) => texts(await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', requestId: id, props: SKILL_TOOL(id, skill) }))
      expect(await row(PREDICTED_SKILL_ID)).toEqual(['ENGINE ToolUse', '◈ adage loaded the acme-work-item skill · no model request', '◈ adage verified 2 of 3 steps here · step 2 stale · /adage why'])
      expect(await row('toolu_9')).toEqual(['ENGINE ToolUse', '◈ adage verified 2 of 3 steps here · step 2 stale · /adage why'])
      expect(await row('toolu_8', 'other-skill')).toEqual(['ENGINE ToolUse'])
      const bash = texts(await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', requestId: 'toolu_7', props: { tool_use_id: 'toolu_7', tool: 'Bash', input: { command: 'ls' }, isRunning: false, isErrored: false, isInterrupted: false } }))
      expect(await bash).toEqual(['ENGINE ToolUse'])
    }
    expect((await $.command.run({ command: 'adage', args: 'why' } as never)).text).toBe('◈ adage verified 2 of 3 steps of the acme-work-item skill here\n\ns1 scripts/wi-lookup {id} · works · 2026-10-03\ns2 pytest · stale: exit 1: command not found · 2026-10-03\ns3 make test · works · 2026-10-03')
  })

  test('T-D4-04: with no answer the text is as computed; inert, nothing is asked', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle] })
    await start($, w)
    expect(await $.skill.prompt({ skill: 'acme-work-item', text: SKILL_TEXT })).toEqual({ text: SKILL_TEXT })
    expect(w.ran.some(r => r.argv[3] === 'SkillPrompt')).toBe(true)
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'ToolUse', requestId: 't1', props: SKILL_TOOL('t1') }))).toEqual(['ENGINE ToolUse'])
  })

  test('T-D4-04: the mod inert (no binary): skill.prompt is next(e), nothing runs', async ($, on) => {
    const w = world(on, { version: '' })
    await start($, w)
    expect(await $.skill.prompt({ skill: 'acme-work-item', text: SKILL_TEXT })).toEqual({ text: SKILL_TEXT })
    expect(w.ran.map(r => r.argv[1])).toEqual(['version'])
  })
})
