// Where Adage's standing text is attached, through the module as the engine loads it (plan C5, C9;
// T-C5-02, T-C5-03, T-C5-05's offline half, T-C9-03): the binary's `Standing` answered beneath the
// plugin by the kit's world, and the engine's own composition, tool description and subagent spawn
// by its hooks beneath. The pure placement is checked alone.
import type { On, SessionMessage } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { BASH_TOOL, SECTIONS, start, world } from './fixtures/kit'
import { SECTION_ID, isAdageAttachment, standingOf, subagentOf, withInstruction, withNote, withPreamble, withSection, withoutAttachments } from './attach'

const STANDING = '<adage-reflex track="acme/learned@1" route="dead-end-unittest" strength="inhibit" kind="avoid">\nDo not run python3 -m unittest here; run make test.\n</adage-reflex>'
const ENVIRONMENT = "In this repository, Python runs in the project's environment, .venv/bin/python. Tests run with .venv/bin/python -m pytest."
const COMPACT = 'Keep the repository procedures this session followed: how tests run here, and what was run after which edits.'
const SUBAGENT = '<adage-reflex track="acme/learned@1" route="lookup-work-item" strength="fire" kind="run">\nIn this repository, work item details are looked up with:\n  ./scripts/wi-lookup W-2045 --json\n</adage-reflex>'
const ANSWER = { standing: STANDING, environment: ENVIRONMENT, compact: COMPACT }
const ENGINE = SECTIONS
const BASH = BASH_TOOL
/** A render of the system prompt, as the engine raises it. */
const COMPOSE = { model: 'claude-x', promptModel: 'claude-x', surfaces: [], tools: ['Bash', 'Read'], outputStyle: null, traits: [] } as const

type Opts = {
  version?: string
  /** What `hook claude Standing` answers (a function of the payload: with `agent`, the subagent's). */
  standing?: (payload: Record<string, unknown>) => unknown
  /** The binary prints garbage for Standing. */
  garbage?: boolean
  observe?: string
  session?: () => string
}

/** The kit's world with the binary answering `Standing`; `asked` is what it was asked, `spawned` the subagents started beneath. */
function standingWorld(on: On, o: Opts) {
  const w = world(on, {
    ...(o.version === undefined ? {} : { version: o.version }),
    ...(o.observe === undefined ? {} : { env: { ADAGE_MOD_OBSERVE: o.observe } }),
    ...(o.session === undefined ? {} : { session: o.session }),
    answer: (event, p) => (event !== 'Standing' ? undefined : o.garbage ? '{' : o.standing === undefined ? ANSWER : o.standing(p)),
  })
  return { get asked() { return w.payloads('Standing') }, spawned: w.agents, w }
}

describe('T-C5-02 · the section', () => {
  test('appended once as a session section after the engine\'s, the same text at every request, from one ask', async ($, on) => {
    const w = standingWorld(on, {})
    await start($, w.w, null, false)
    const composed = [await $.prompt.compose(COMPOSE), await $.prompt.compose(COMPOSE), await $.prompt.compose(COMPOSE)]
    for (const c of composed) {
      expect(c.sections).toEqual([...ENGINE, { id: SECTION_ID, text: STANDING, scope: 'session' }])
    }
    expect(w.asked).toEqual([{ session_id: 'sess-1', cwd: '/w', hook_event_name: 'Standing', via: 'mod' }])
  })

  test('empty standing text: the engine\'s sections, unchanged', async ($, on) => {
    const w = standingWorld(on, { standing: () => ({ ...ANSWER, standing: '' }) })
    await start($, w.w, null, false)
    expect((await $.prompt.compose(COMPOSE)).sections).toEqual(ENGINE)
  })

  for (const [name, o] of [
    ['nothing', { standing: () => undefined }],
    ['garbage', { garbage: true }],
  ] as [string, Opts][]) {
    test(`the binary answers ${name}: the sections and the description are unchanged, and the session is not asked again`, async ($, on) => {
      const w = standingWorld(on, o)
      await start($, w.w, null, false)
      expect((await $.prompt.compose(COMPOSE)).sections).toEqual(ENGINE)
      expect((await $.prompt.compose(COMPOSE)).sections).toEqual(ENGINE)
      expect((await $.tool.describe(BASH)).description).toBe(BASH.description)
      expect(w.asked).toHaveLength(1)
    })
  }

  test('inert: nothing is asked and nothing is attached', async ($, on) => {
    const w = standingWorld(on, { version: '' })
    await start($, w.w, null, false)
    expect((await $.prompt.compose(COMPOSE)).sections).toEqual(ENGINE)
    expect((await $.tool.describe(BASH)).description).toBe(BASH.description)
    await $.agent.spawn({ prompt: 'Find W-2045', description: 'Look up', subagentType: 'Explore' } as never)
    expect(w.spawned.map(s => s.prompt)).toEqual(['Find W-2045'])
    expect(w.asked).toEqual([])
  })

  test('observe mode: the payload says so (the binary answers as for the control arm)', async ($, on) => {
    const w = standingWorld(on, { observe: '1' })
    await start($, w.w, null, false)
    await $.prompt.compose(COMPOSE)
    expect(w.asked).toEqual([{ session_id: 'sess-1', cwd: '/w', hook_event_name: 'Standing', via: 'mod', observe: true }])
  })

  test('a /clear is a new session: asked once for each id', async ($, on) => {
    let id = 'sess-1'
    const w = standingWorld(on, { session: () => id })
    await start($, w.w, null, false)
    await $.prompt.compose(COMPOSE)
    id = 'sess-2'
    await $.classic.SessionStart({ source: 'clear' } as never)
    await $.prompt.compose(COMPOSE)
    expect(w.asked.map(p => p.session_id)).toEqual(['sess-1', 'sess-2'])
  })
})

describe('T-C9-03 · the Bash tool\'s description and the subagent\'s prompt', () => {
  test('tool.describe { tool: Bash } appends the environment note once; another tool is untouched', async ($, on) => {
    const w = standingWorld(on, {})
    await start($, w.w, null, false)
    const first = await $.tool.describe(BASH)
    expect(first).toEqual({ description: `${BASH.description}\n\n${ENVIRONMENT}` })
    expect(await $.tool.describe(BASH)).toEqual(first)
    expect(await $.tool.describe({ ...BASH, description: first.description })).toEqual(first)
    expect(await $.tool.describe({ ...BASH, tool: 'Read', description: 'Reads a file.' })).toEqual({ description: 'Reads a file.' })
  })

  test('no environment bound: the description as the engine computed it', async ($, on) => {
    const w = standingWorld(on, { standing: () => ({ ...ANSWER, environment: '' }) })
    await start($, w.w, null, false)
    expect(await $.tool.describe(BASH)).toEqual({ description: BASH.description })
  })

  test('agent.spawn asks Standing with the subagent, after the session\'s own, and prepends what it answers', async ($, on) => {
    const w = standingWorld(on, { standing: p => (p.agent === undefined ? ANSWER : { ...ANSWER, subagent: SUBAGENT }) })
    await start($, w.w, null, false)
    await $.agent.spawn({ prompt: 'Find what W-2045 asks for', description: 'Look up the work item', subagentType: 'Explore' } as never)
    expect(w.spawned).toEqual([{ prompt: `${SUBAGENT}\n\nFind what W-2045 asks for`, description: 'Look up the work item', subagentType: 'Explore' }])
    expect(w.asked.map(p => p.agent)).toEqual([undefined, { type: 'Explore', description: 'Look up the work item', prompt: 'Find what W-2045 asks for' }])
    expect(w.asked[1]).toMatchObject({ session_id: 'sess-1', cwd: '/w', hook_event_name: 'Standing', via: 'mod' })
  })

  for (const [name, o] of [
    ['nothing applies to the subagent', { standing: () => ANSWER }],
    ['the binary fails', { garbage: true }],
  ] as [string, Opts][]) {
    test(`${name}: next(e) unchanged`, async ($, on) => {
      const w = standingWorld(on, o)
      await start($, w.w, null, false)
      await $.agent.spawn({ prompt: 'Find W-2045', description: 'Look up', subagentType: 'Explore' } as never)
      expect(w.spawned.map(s => s.prompt)).toEqual(['Find W-2045'])
    })
  }
})

// The test engine raises `session.compact` with no transcript and refuses any `next` whose
// `messages` is not a list, so the hook is checked by its parts here (the instruction, the rows)
// and the whole in a live session (C3-C9-mod-notes).
describe('T-C5-03 · compaction', () => {
  test('the instruction is appended to the person\'s words, once', () => {
    expect(withInstruction('keep the todo list', COMPACT)).toBe(`keep the todo list\n${COMPACT}`)
    expect(withInstruction(undefined, COMPACT)).toBe(COMPACT)
    expect(withInstruction(`already said\n${COMPACT}`, COMPACT)).toBe(`already said\n${COMPACT}`)
  })

  test('without a structural handle (2.1.288: rows carry no origin) no row is removed, never by its text', () => {
    const rows = [
      { role: 'user', text: 'fix W-2042', toolUses: [] },
      { role: 'user', text: '<adage-reflex track="x" route="y">a quote</adage-reflex>', toolUses: [] },
      { role: 'assistant', text: 'Done.', toolUses: [] },
    ] as unknown as SessionMessage[]
    expect(withoutAttachments(rows)).toBe(rows)
    expect(rows.map(isAdageAttachment)).toEqual([false, false, false])
  })

  test('with the handle, only rows whose origin names the plugin go', () => {
    const mine = { role: 'user', text: '<adage-reflex …>', toolUses: [], origin: { kind: 'plugin', plugin: 'adage' } }
    const theirs = { role: 'user', text: '<adage-reflex …>', toolUses: [], origin: { kind: 'plugin', plugin: 'other' } }
    const person = { role: 'user', text: 'fix <adage-reflex', toolUses: [], origin: { kind: 'composer' } }
    const rows = [person, mine, theirs] as unknown as SessionMessage[]
    expect(withoutAttachments(rows)).toEqual([person, theirs])
    expect(rows.map(isAdageAttachment)).toEqual([false, true, false])
    expect(isAdageAttachment('text')).toBe(false)
  })
})

describe('T-C5-02, T-C9-03 · the placement, alone', () => {
  test('withSection: appended as session, a same-id section replaced, never doubled, empty leaves the list', () => {
    const added = withSection(ENGINE, STANDING)
    expect(added).toEqual([...ENGINE, { id: SECTION_ID, text: STANDING, scope: 'session' }])
    expect(withSection(added, 'other')).toEqual([...ENGINE, { id: SECTION_ID, text: 'other', scope: 'session' }])
    expect(withSection(ENGINE, '')).toBe(ENGINE)
    expect(SECTION_ID).toBe('adage:procedures')
  })

  test('withNote, withInstruction, withPreamble: once, after a blank line, nothing on empty', () => {
    expect(withNote('d', 'n')).toBe('d\n\nn')
    expect(withNote('d\n\nn', 'n')).toBe('d\n\nn')
    expect(withNote('', 'n')).toBe('n')
    expect(withNote('d', '')).toBe('d')
    expect(withInstruction(undefined, 'c')).toBe('c')
    expect(withInstruction('', 'c')).toBe('c')
    expect(withInstruction('i', 'c')).toBe('i\nc')
    expect(withInstruction('i\nc', 'c')).toBe('i\nc')
    expect(withInstruction('i', '')).toBe('i')
    expect(withInstruction(undefined, '')).toBeUndefined()
    expect(withPreamble('p', 's')).toBe('s\n\np')
    expect(withPreamble('p', '')).toBe('p')
  })

  test('standingOf and subagentOf: the binary\'s words, or nothing', () => {
    expect(standingOf(ANSWER)).toEqual(ANSWER)
    expect(standingOf({ standing: 7, compact: COMPACT })).toEqual({ standing: '', environment: '', compact: COMPACT })
    expect(standingOf(undefined)).toEqual({ standing: '', environment: '', compact: '' })
    expect(standingOf([])).toEqual({ standing: '', environment: '', compact: '' })
    expect(subagentOf({ ...ANSWER, subagent: SUBAGENT })).toBe(SUBAGENT)
    expect(subagentOf(ANSWER)).toBe('')
    expect(subagentOf('x')).toBe('')
  })
})
