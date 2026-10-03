// Noticed, learning and skills (plan D5; ux-design §4.6–§4.8; T-D5-01 … T-D5-03, T-D4-04): the pure
// words, then each row through the engine on both surfaces.
import type { TurnStepChunk, TurnStepResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'

import { BAND, BANNED, CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE, CARD, GROUP, LINE, PREDICTED_SKILL_ID, PREDICT_SKILL, SESSION, SKILL_ANNOTATED, SKILL_TEXT, SKILL_TOOL, STEP, SURFACES, T0, start, world } from './fixtures/kit'
import type { Surface } from './fixtures/kit'
import { parseLine } from './feed'
import { candidatesOf, invokedMark, noticedIds, noticedLine, noticedLines, skillOf, verifiedMark, withSkill } from './notice'
import type { Row } from './plugin'
import { reflexesOf, whySkill, whyText } from './view'

type Mounted = { findAll: (q: { type?: string; key?: string }) => Promise<{ type: string; key: string | undefined; text: string; props: Record<string, unknown> }[]> }
const texts = async (ui: Mounted) => (await ui.findAll({ type: 'Text' })).map(t => t.text)
async function drain(stream: AsyncGenerator<TurnStepChunk, TurnStepResult>) {
  for (;;) if ((await stream.next()).done) return
}
const rows = (...lines: string[]): Row[] =>
  lines.map((l, k) => {
    const p = parseLine(l, SESSION)
    if (p?.kind !== 'decision') throw new Error('not a decision')
    return { ...p.row, seq: k + 1 }
  })

const CANDIDATES = candidatesOf({ candidates: [CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE] })
const NOTICED_CARD = { ...CARD, ui: { noticed: true } }
const FEED = [LINE.learnerIdle, LINE.healthy, LINE.noticed(9, 'run-a54d7c', ['t1', 't2']), LINE.noticed(10, 'run-a54d7c', ['t3']), LINE.noticed(11, 'lookup-then-read', ['t4'], { review: true, support: 4, s_min: 5 }), LINE.noticed(12, 'run-single', ['t5'])]

describe('noticed, as words', () => {
  test('candidates as status --json lists them: the gist, never the id; a sequence is a context', () => {
    expect(CANDIDATES).toEqual([
      { id: 'run-a54d7c', say: 'after make test, run make lint', sequence: true },
      { id: 'lookup-then-read', say: 'scripts/wi-lookup then reading the files it names', sequence: true },
      { id: 'run-single', say: 'run make lint', sequence: false },
    ])
    expect(candidatesOf({})).toEqual([])
  })

  test('noticed.forming and noticed.review: plain counts, never a forecast', () => {
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 3, s_min: 5, review: false }, 'make test after editing acme/**', false)).toBe('○ adage noticed · make test after editing acme/** · seen in 3 earlier sessions')
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 1, s_min: 5, review: false }, 'x', false)).toBe('○ adage noticed · x · seen in 1 earlier session')
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 4, s_min: 5, review: true }, 'scripts/wi-lookup then reading the files it names', false)).toBe('○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review')
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 3, review: false }, 'x', true)).toBe('o adage noticed - x - seen in 3 earlier sessions')
  })

  test('T-D5-01 (pure): under the first batch only, by the calls the row names, else by when its hooks ran; a one-event record draws nothing', () => {
    const r = rows(...FEED.slice(2))
    expect(noticedLines(r, ['t1', 't2'], [], CANDIDATES, false)).toEqual(['○ adage noticed · after make test, run make lint · seen in 2 earlier sessions'])
    expect(noticedLines(r, ['t3'], [], CANDIDATES, false)).toEqual([])
    expect(noticedLines(r, ['t4'], [], CANDIDATES, false)).toEqual(['○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review'])
    expect(noticedLines(r, ['t5'], [], CANDIDATES, false)).toEqual([])
    expect(noticedLines(r, ['t1'], [], [], false)).toEqual([])
    // A row that names no call: the batch whose hooks ran when it was written.
    const unnamed = rows(JSON.stringify({ kind: 'decision', cursor: 'c', ts: T0 + 500, session: SESSION, harness: 'claude', event: 'tools', acts: [{ act: 'noticed', candidate: 'run-a54d7c', support: 2, s_min: 3, review: false }], texts: [] }))
    expect(noticedLines(unnamed, ['t9'], [{ ids: ['t8', 't9'], from: T0, to: T0 + 400 }], CANDIDATES, false)).toHaveLength(1)
    expect(noticedLines(unnamed, ['t9'], [{ ids: ['t8', 't9'], from: T0 + 5000, to: T0 + 6000 }], CANDIDATES, false)).toEqual([])
    expect(noticedIds(r)).toEqual(['run-a54d7c', 'lookup-then-read', 'run-single'])
  })
})

describe('skills, as words', () => {
  test('a SkillPrompt answer read back: the date and one step per line', () => {
    const s = skillOf('acme-work-item', SKILL_ANNOTATED, 7)
    expect(s).toEqual({ skill: 'acme-work-item', at: 7, date: '2026-10-03', steps: [{ command: 'scripts/wi-lookup {id}', result: 'works', signature: '' }, { command: 'pytest', result: 'stale', signature: 'exit 1: command not found' }, { command: 'make test', result: 'works', signature: '' }] })
    expect(skillOf('x', SKILL_TEXT, 0)).toBeUndefined()
    expect(skillOf('x', 'Adage verified these steps in this repository on 2026-10-03:\n- make suite: stale\n', 0)?.steps).toEqual([{ command: 'make suite', result: 'stale', signature: '' }])
    if (s === undefined) throw new Error('skill')
    expect(withSkill([s], { ...s, at: 9 })).toEqual([{ ...s, at: 9 }])
  })

  test('tool.skill.invoked, tool.skill.verified, and /adage why after a verified skill (cmd.why.step)', () => {
    const s = skillOf('acme-work-item', SKILL_ANNOTATED, 7)
    if (s === undefined) throw new Error('skill')
    expect(invokedMark('release-checklist', false)).toBe('◈ adage loaded the release-checklist skill · no model request')
    expect(verifiedMark(s, false)).toBe('◈ adage verified 2 of 3 steps here · step 2 stale · /adage why')
    expect(verifiedMark({ ...s, steps: s.steps.filter(x => x.result === 'works') }, true)).toBe('<> adage verified 2 of 2 steps here - /adage why')
    expect(whySkill(s, false)).toBe('◈ adage verified 2 of 3 steps of the acme-work-item skill here\n\ns1 scripts/wi-lookup {id} · works · 2026-10-03\ns2 pytest · stale: exit 1: command not found · 2026-10-03\ns3 make test · works · 2026-10-03')
    // After the last directive of the session the skill's steps are the answer; before it, the directive.
    expect(whyText(rows(LINE.firedGen), reflexesOf({ routes: [] }), false, { ...s, at: T0 + 5000 })).toContain('acme-work-item skill here')
    expect(whyText(rows(LINE.firedGen), reflexesOf({ routes: [] }), false, { ...s, at: 0 })).toStartWith('◠ codegen-after-schema')
    expect(whyText([], [], false, s)).toContain('acme-work-item skill here')
    expect(whyText([], [], false, undefined)).toBe('No reflex has spoken in this session yet.')
    for (const l of [invokedMark('x', false), verifiedMark(s, false), whySkill(s, false)]) expect(l).not.toMatch(BANNED)
  })
})

describe('T-D5-01 · the noticed line through the engine', () => {
  async function group($: Engine, surface: Surface, ids: string[]) {
    const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'ToolGroup', requestId: ids[0], props: GROUP(ids) })
    const t = await texts(ui)
    await ui.unmount()
    return t
  }

  test('once under the first batch of a sequence, worded from status --json, in both arms; nothing with ui.noticed off', async ($, on) => {
    const w = world(on, { feed: FEED, card: NOTICED_CARD, graph: { routes: [] }, status_json: { tracks: [], candidates: [CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE] } })
    await start($, w)
    // The first drawing finds the candidate's words not read yet, reads them, and redraws.
    await group($, 'terminal', ['t1', 't2'])
    await w.clock.settle()
    for (const surface of SURFACES) {
      expect(await group($, surface, ['t1', 't2'])).toEqual(['ENGINE ToolGroup', '○ adage noticed · after make test, run make lint · seen in 2 earlier sessions'])
      expect(await group($, surface, ['t3'])).toEqual(['ENGINE ToolGroup'])
      expect(await group($, surface, ['t4'])).toEqual(['ENGINE ToolGroup', '○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review'])
      expect(await group($, surface, ['t5'])).toEqual(['ENGINE ToolGroup'])
      expect(await group($, surface, ['unmatched'])).toEqual(['ENGINE ToolGroup'])
    }
    expect(w.ran.filter(r => r.argv[1] === 'status').length).toBeGreaterThanOrEqual(2)
    // The band counts what was noticed while the pane is closed.
    const band = async () => texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'AbovePrompt', props: BAND() }))
    expect((await band()).join('')).toBe('◠ adage  ● healthy · 3 patterns noticed')
    await $.command.run({ command: 'adage', args: 'watch' } as never)
    expect((await band()).join('')).toBe('ENGINE AbovePrompt')
  })

  test('off by default: the setting the binary reports draws the line', async ($, on) => {
    const w = world(on, { feed: FEED, status_json: { tracks: [], candidates: [CANDIDATE] } })
    await start($, w)
    await group($, 'terminal', ['t1', 't2'])
    await w.clock.settle()
    expect(await group($, 'terminal', ['t1', 't2'])).toEqual(['ENGINE ToolGroup'])
    expect(w.ran.filter(r => r.argv[1] === 'status').length).toBe(1)
  })
})

describe('T-D5-02 · the learner and its toasts through the engine', () => {
  test('learning in 38 s, then learning…; a strength change from this session or recent sessions; a candidate that formed; no change, nothing; a failed learn once', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerWaiting, LINE.healthy, LINE.firedGen], later: [LINE.learnStrength(T0 + 10_000, [SESSION]), LINE.learnStrength(T0 + 10_001), LINE.learnNew(T0 + 10_002, [SESSION]), LINE.learnEvidence(T0 + 10_003), LINE.learnFailed(T0 + 10_004), LINE.learnFailed(T0 + 10_005), LINE.learnerRunning] })
    await start($, w)
    const band = async () => texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'AbovePrompt', props: BAND() }))
    expect((await band()).join('')).toContain('learning in 38 s')
    await w.deliver()
    expect((await band()).join('')).toContain('learning…')
    expect(w.toasts).toEqual([
      'adage learned from this session\n◠ run make gen after a schema edit · now a direct instruction',
      'adage learned from recent sessions\n◠ run make gen after a schema edit · now a direct instruction',
      'adage learned from this session\n◠ run make test after editing acme/** · new, from this session',
      'adage could not learn from this session (sandbox) · run adage doctor',
    ])
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
