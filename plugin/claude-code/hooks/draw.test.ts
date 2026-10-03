// Every render site and the band, drawn through the engine from the state the hooks and the feed
// leave (test-plan §6, B2): mounted on the terminal and the desktop from the same state (T-X-07),
// the words a default surface never carries (T-B2-10), the arm never drawn (T-X-06).
import type { TurnStepChunk, TurnStepResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'

import { BAND, BANNED, CARD, DELEGATE, LAPSED, LINE, OUTPUT, PREDICT, PREDICTED_ID, RESULT, STEP, SURFACES, TOOL, USER, block, start, world } from './fixtures/kit'
import type { Surface } from './fixtures/kit'
import { ROLES } from './tokens'

type Mounted = { findAll: (q: { type?: string; key?: string; text?: string | RegExp }) => Promise<{ type: string; key: string | undefined; text: string; props: Record<string, unknown> }[]> }

/** The texts a drawing shows, in order. */
async function texts(ui: Mounted): Promise<string[]> {
  return (await ui.findAll({ type: 'Text' })).map(t => t.text)
}

async function drain(stream: AsyncGenerator<TurnStepChunk, TurnStepResult>) {
  for (;;) if ((await stream.next()).done) return
}

/** A prompt as the hooks see it (the request id is the row's, which the payload does not carry). */
async function prompt($: Engine, _row: string, text: string) {
  await $.classic.UserPromptSubmit({ prompt: text, source: 'user' } as never)
}

describe('B2 · the opening line', () => {
  test("T-B2-01: the engine's notice redrawn with the glyph; another notice untouched", async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle] })
    await start($, w)
    const ui = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'InfoNotice', props: { text: 'adage: 3 reflexes and 4 instincts active in this repo. Nothing runs without you.', command: null } })
    expect(await texts(ui)).toEqual(['ENGINE InfoNotice: ◠ adage · 3 reflexes and 4 instincts active in this repo. Nothing runs without you.'])
    const other = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'InfoNotice', props: { text: 'Using a model from settings', command: null } })
    expect(await texts(other)).toEqual(['ENGINE InfoNotice: Using a model from settings'])
  })
})

describe('B2 · the line under a prompt', () => {
  test('T-B2-02: three reflexes, three lines under the engine row, no strength word; none: the engine alone', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], classic: { UserPromptSubmit: { additionalContext: [block('lookup-work-item') + block('codegen-after-schema') + block('dead-end-unittest', 'inhibit')] } } })
    await start($, w)
    await prompt($, 'u1', 'fix W-2042')
    for (const surface of SURFACES) {
      const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042'), viewport: { columns: 120, rows: 40 } })
      const lines = await texts(ui)
      expect(lines).toEqual(['ENGINE UserMessage: fix W-2042', '◠ look up the work item with scripts/wi-lookup', '◠ run make gen after a schema edit', '◠ avoid python3 -m unittest here; use make test'])
      for (const l of lines.slice(1)) expect(l).not.toMatch(BANNED)
      await ui.unmount()
      const expanded = await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042', true) })
      expect(await texts(expanded)).toEqual(['ENGINE UserMessage: fix W-2042'])
      await expanded.unmount()
    }
  })

  test('T-B2-02: a prompt nothing fired at draws the engine row alone; each prompt its own lines', async ($, on) => {
    let answer: unknown = { additionalContext: [block('codegen-after-schema')] }
    const w = world(on, { feed: [LINE.learnerIdle], classic: { UserPromptSubmit: () => answer } })
    await start($, w)
    await prompt($, 'u1', 'first')
    answer = {}
    await prompt($, 'u2', 'second')
    for (const surface of SURFACES) {
      expect(await texts(await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u2', props: USER('second') }))).toEqual(['ENGINE UserMessage: second'])
      expect(await texts(await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u1', props: USER('first') }))).toHaveLength(2)
    }
  })

  test('T-B2-03: at 40 columns the command slot goes first and the Text truncates at its end', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.firedLookup], classic: { UserPromptSubmit: { additionalContext: [block('lookup-work-item')] } } })
    await start($, w)
    await prompt($, 'u1', 'fix W-2042')
    const wide = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042'), viewport: { columns: 120, rows: 40 } })
    expect((await texts(wide))[1]).toBe('◠ look up the work item with scripts/wi-lookup · scripts/wi-lookup W-2042 --json')
    await wide.unmount()
    const narrow = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042'), viewport: { columns: 40, rows: 40 } })
    const [, line] = await narrow.findAll({ type: 'Text' })
    expect(line?.text).toBe('◠ look up the work item with scripts/wi…')
    expect(line?.props.wrap).toBe('truncate-end')
  })
})

describe('B2 · the markers under a tool row', () => {
  test('T-B2-04: a rewrite, a predicted step and a dead end headed off, each under its own row; another row untouched', async ($, on) => {
    const w = world(on, {
      feed: [LINE.learnerIdle, LINE.avoidUnittest],
      later: [LINE.avoidedUnittest],
      pretool: { updatedInput: { command: '.venv/bin/python -m pytest -q' } },
      hook: { TurnStep: PREDICT },
    })
    await start($, w)
    await $.tool.call({ tool: 'Bash', command: 'pytest -q', tool_use_id: 't1' } as never)
    await drain($.turn.step(STEP))
    await $.classic.PostToolBatch({ tool_calls: [{ tool_name: 'Bash', tool_input: { command: 'make test' }, tool_use_id: 't3' }] } as never)
    await w.deliver()
    for (const surface of SURFACES) {
      const row = async (id: string) => texts(await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', requestId: id, props: TOOL(id) }))
      expect(await row('t1')).toEqual(['ENGINE ToolUse', '↪ adage: pointed this test run at .venv/bin/python'])
      expect(await row(PREDICTED_ID)).toEqual(['ENGINE ToolUse', '◠ adage chose this step · no model request'])
      expect(await row('t3')).toEqual(['ENGINE ToolUse', '◠ adage: dead end headed off'])
      expect(await row('unmatched')).toEqual(['ENGINE ToolUse'])
    }
  })

  test('T-B2-05: a receipt delivered beside a run that happened: one dim line under the output', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], pretool: { additionalContext: ['<adage-receipt operation="a1f2c3d4e5f6">\nAdage ran it.\n</adage-receipt>'] } })
    await start($, w)
    await $.tool.call({ tool: 'Bash', command: 'make test', tool_use_id: 't2' } as never)
    for (const surface of SURFACES) {
      const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'ToolResult', requestId: 't2', props: RESULT('t2') })
      const found = await ui.findAll({ type: 'Text' })
      expect(found.map(t => t.text)).toEqual(['ENGINE ToolResult', '◠ adage receipt a1f2 covers this run · /adage why'])
      expect(found[1]?.props.dimColor).toBe(true)
      expect(await texts(await $.ui.mount({ plugin: 'adage', surface, component: 'ToolResult', requestId: 'other', props: RESULT('other') }))).toEqual(['ENGINE ToolResult'])
    }
  })
})

describe('B2 · the spinner and the turn line', () => {
  test('T-B2-06: off by default; on, a suffix while a reflex fired this turn and the counts at the turn end', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], card: { ...CARD, ui: { spinner: true, turn_line: true } }, hook: { TurnStep: PREDICT }, classic: { UserPromptSubmit: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w)
    const spin = { word: 'Thinking', message: null, suffix: '…', mode: 'requesting' } as const
    const turnLine = { word: 'Worked', durationMs: 108_000 }
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'Spinner', props: spin }))).toEqual(['ENGINE Spinner: Thinking…'])
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'TurnDuration', props: turnLine }))).toEqual(['ENGINE TurnDuration: Worked'])
    await prompt($, 'u1', 'go')
    await drain($.turn.step(STEP))
    for (const surface of SURFACES) {
      expect(await texts(await $.ui.mount({ plugin: 'adage', surface, component: 'Spinner', props: spin }))).toEqual(['ENGINE Spinner: Thinking… · adage ◠ 1…'])
    }
    const done = await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'TurnDuration', props: turnLine }))
    expect(done).toEqual(['ENGINE TurnDuration: Worked', ' · adage: 1 step without a model request'])
    expect(done.join(' ')).not.toMatch(BANNED)
  })

  test('T-B2-06: without the binary reporting the lines on, nothing is added', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], hook: { TurnStep: PREDICT }, classic: { UserPromptSubmit: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w)
    await prompt($, 'u1', 'go')
    await drain($.turn.step(STEP))
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'Spinner', props: { word: 'Thinking', message: null, suffix: '…', mode: 'requesting' } }))).toEqual(['ENGINE Spinner: Thinking…'])
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'TurnDuration', props: { word: 'Worked', durationMs: 1000 } }))).toEqual(['ENGINE TurnDuration: Worked'])
  })
})

/** The band's text, one string per line ('' when the engine's own). */
async function band($: Engine, surface: Surface, columns = 120): Promise<string> {
  const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: BAND(columns) })
  return (await texts(ui)).join('')
}

describe('B2 · the band', () => {
  test('T-B2-07: the cells as the session goes; at 60 columns the lowest go first, the lamp word stays', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerWaiting, LINE.healthy, LINE.firedGen], hook: { TurnStep: PREDICT }, graph: { routes: [] } })
    await start($, w)
    await drain($.turn.step(STEP))
    await w.clock.advance(1000)
    for (const surface of SURFACES) {
      expect(await band($, surface)).toBe('◠ adage  1 reflex this session · 1 step without a model request · ● healthy · learning in 37 s')
      expect(await band($, surface, 60)).toBe('◠ adage  1 reflex this session · ● healthy')
    }
  })

  test('T-B2-07: a quiet session leaves the band to the engine; so does a survey and a session outside a repository', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], graph: { routes: [] } })
    await start($, w)
    for (const surface of SURFACES) {
      expect(await band($, surface)).toBe('ENGINE AbovePrompt')
      const survey = await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: { ...BAND(), hasSurvey: true } })
      expect(await texts(survey)).toEqual(['ENGINE AbovePrompt'])
    }
  })

  test('T-B2-07: outside a repository (no learner line) the band is the engine', async ($, on) => {
    const w = world(on, { feed: [LINE.healthy, LINE.firedGen], graph: { routes: [] } })
    await start($, w)
    for (const surface of SURFACES) expect(await band($, surface)).toBe('ENGINE AbovePrompt')
  })

  test('T-B2-07: off, paused, no binary', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.off] })
    await start($, w)
    for (const surface of SURFACES) expect(await band($, surface)).toBe('◠ adage ○ off in this repository · /adage on')
    await $.command.run({ command: 'adage', args: 'pause' } as never)
    for (const surface of SURFACES) expect(await band($, surface)).toBe('◠ adage  paused · /adage pause')
  })

  test('T-B2-07, §9.1: no binary: the band says so, the status says why, once; nothing else is drawn', async ($, on) => {
    const w = world(on, { version: '', status: 'no binary for darwin-arm64; install with brew install adage' })
    await start($, w)
    await $.classic.SessionStart({ source: 'startup' } as never)
    for (const surface of SURFACES) expect(await band($, surface)).toBe('◠ adage ○ off · run adage doctor')
    expect(w.statuses).toEqual(['no binary for darwin-arm64; install with brew install adage'])
    expect(w.spawned).toEqual([])
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'ToolUse', requestId: 't1', props: TOOL('t1') }))).toEqual(['ENGINE ToolUse'])
  })

  test('T-B2-08: a lapsed grant on the second line, over a review waiting; its toast once', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], delegate: { ...DELEGATE, lapsed: LAPSED } })
    await start($, w)
    for (const surface of SURFACES) expect(await band($, surface)).toBe('◠ adage  ● healthy» 1 grant lapsed · /adage health')
    expect(w.toasts).toEqual(['adage\nacme/learned/codegen-after-schema is no longer allowed: argv_templates changed · /adage review'])
  })

  test('T-B2-08: review waiting, then a problem over it', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], later: [LINE.problem] })
    await start($, w)
    expect(await band($, 'terminal')).toBe('◠ adage  ● healthy» 1 need review · /adage review')
    await w.deliver()
    expect(await band($, 'terminal')).toBe('◠ adage  ● problem: hook latency» problem: hook latency · /adage health')
  })

  test('T-B2-08, §9.6: a newer harness, until the first prompt, once per version', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], harness: '2.1.300', graph: { routes: [] } })
    await start($, w)
    expect(await band($, 'terminal')).toBe('◠ adage  ● healthy» Claude Code 2.1.300 is newer than this plugin was tested with (2.1.288) · run adage doctor')
    await prompt($, 'u1', 'go')
    expect(await band($, 'terminal')).toBe('ENGINE AbovePrompt')
  })

  test('T-B2-09: the lamp in the fire, prime and inhibit roles, its word beside it', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.check], later: [LINE.problem], graph: { routes: [] } })
    await start($, w)
    const lamp = async () => (await (await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'AbovePrompt', props: BAND() })).findAll({ type: 'Text', text: /●/ }))[0]?.props.color
    expect(await band($, 'terminal')).toBe('◠ adage  ● check: hooks: claude-code')
    expect(await lamp()).toBe(ROLES.prime)
    await w.deliver()
    expect(await lamp()).toBe(ROLES.inhibit)
  })

  test('T-B2-11: ADAGE_ASCII picks every twin; reduced motion schedules no redraw', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy, LINE.firedGen], env: { ADAGE_ASCII: '1', ADAGE_REDUCED_MOTION: '1' }, graph: { routes: [] } })
    await start($, w)
    expect(await band($, 'terminal')).toBe('~ adage  1 reflex this session - * healthy')
    expect(w.invalidated).toBe(0)
  })
})

describe('the finish line', () => {
  test('toast.gate: a Stop the binary blocked says once that the agent continues', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], classic: { Stop: { block: 'Run make gen before finishing.' } } })
    await start($, w)
    expect(await $.classic.Stop({ stop_hook_active: false } as never)).toEqual({ block: 'Run make gen before finishing.' })
    expect(w.toasts).toEqual(['adage: finish line · the agent continues once'])
  })
})

describe('T-B2-10 · theme keys only, no banned words, no status prefix', () => {
  test('every Text colour is a role, outside the pane no strength, lifecycle, approval or saved', async ($, on) => {
    const w = world(on, {
      feed: [LINE.learnerWaiting, LINE.problem, LINE.firedGen, LINE.firedLookup, LINE.avoidUnittest],
      hook: { TurnStep: PREDICT },
      pretool: { updatedInput: { command: '.venv/bin/python -m pytest -q' } },
      classic: { UserPromptSubmit: { additionalContext: [block('lookup-work-item'), block('dead-end-unittest', 'inhibit')] } },
      delegate: { ...DELEGATE, lapsed: LAPSED },
      version: 'adage 0.1.0',
    })
    await start($, w)
    await prompt($, 'u1', 'fix W-2042')
    await $.tool.call({ tool: 'Bash', command: 'pytest -q', tool_use_id: 't1' } as never)
    await drain($.turn.step(STEP))
    const roles = new Set<string>(Object.values(ROLES))
    for (const surface of SURFACES) {
      const sites = [
        await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: BAND() }),
        await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042') }),
        await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', requestId: 't1', props: TOOL('t1') }),
        await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', requestId: PREDICTED_ID, props: TOOL(PREDICTED_ID) }),
        await $.ui.mount({ plugin: 'adage', surface, component: 'CommandOutput', props: OUTPUT('', (await $.command.run({ command: 'adage', args: 'status' } as never)).text ?? '') }),
      ]
      for (const ui of sites) {
        for (const t of await ui.findAll({ type: 'Text' })) {
          if (t.props.color !== undefined) expect(roles.has(String(t.props.color)), String(t.props.color)).toBe(true)
          if (!t.text.startsWith('ENGINE')) expect(t.text).not.toMatch(BANNED)
        }
      }
    }
    for (const s of w.statuses) expect(s ?? '').not.toStartWith('adage:')
    for (const t of w.toasts) expect(t).not.toMatch(/\b(saved|approval)\b/)
  })
})
