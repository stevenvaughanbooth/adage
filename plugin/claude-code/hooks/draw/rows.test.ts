// The lines Adage adds under the transcript's rows (draw/rows.tsx, through their sites; test-plan
// §6 B2: T-B2-02 … T-B2-06): under a prompt, the markers under a tool row, the receipt beside a
// run, the spinner suffix and the turn line, on both surfaces.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, CARD, GROUP, LINE, PREDICT, PREDICTED_ID, RESULT, STEP, SURFACES, TOOL, USER, block, drain, prompt, start, texts, world } from '../fixtures/kit'

describe('T-B2-02, T-B2-03 · the line under a prompt', () => {
  test('T-B2-02: three reflexes, three lines under the engine row, no strength word; none: the engine alone', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], classic: { UserPromptSubmit: { additionalContext: [block('lookup-work-item') + block('codegen-after-schema') + block('dead-end-unittest', 'inhibit')] } } })
    await start($, w)
    await prompt($, 'fix W-2042')
    for (const surface of SURFACES) {
      const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042'), viewport: { columns: 120, rows: 40 } })
      const lines = await texts(ui)
      expect(lines).toEqual(['ENGINE UserMessage: fix W-2042', '◠ adage: look up the work item with scripts/wi-lookup', '◠ adage: run make gen after a schema edit', '◠ adage: avoid python3 -m unittest here; use make test'])
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
    await prompt($, 'first')
    answer = {}
    await prompt($, 'second')
    for (const surface of SURFACES) {
      expect(await texts(await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u2', props: USER('second') }))).toEqual(['ENGINE UserMessage: second'])
      expect(await texts(await $.ui.mount({ plugin: 'adage', surface, component: 'UserMessage', requestId: 'u1', props: USER('first') }))).toHaveLength(2)
    }
  })

  test('T-B2-03: at 40 columns the command slot goes first and the Text truncates at its end', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.firedLookup], classic: { UserPromptSubmit: { additionalContext: [block('lookup-work-item')] } } })
    await start($, w)
    await prompt($, 'fix W-2042')
    const wide = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042'), viewport: { columns: 120, rows: 40 } })
    expect((await texts(wide))[1]).toBe('◠ adage: look up the work item with scripts/wi-lookup · scripts/wi-lookup W-2042 --json')
    await wide.unmount()
    const narrow = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'UserMessage', requestId: 'u1', props: USER('fix W-2042'), viewport: { columns: 40, rows: 40 } })
    const [, line] = await narrow.findAll({ type: 'Text' })
    expect(line?.text).toBe('◠ adage: look up the work item with scr…')
    expect(line?.props.wrap).toBe('truncate-end')
  })
})

describe('T-B2-12 · what Adage told the agent after a tool batch (tool.fired)', () => {
  test('one line per reflex, under the batch\'s last call, or its group\'s line while folded; never twice', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], classic: { PostToolBatch: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w)
    const call = (id: string) => ({ tool_name: 'Edit', tool_input: {}, tool_use_id: id })
    await $.classic.PostToolBatch({ tool_calls: [call('t4'), call('t5')] } as never)
    for (const surface of SURFACES) {
      const row = async (id: string) => {
        const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'ToolUse', requestId: id, props: TOOL(id) })
        const lines = await texts(ui)
        await ui.unmount()
        return lines
      }
      expect(await row('t5')).toEqual(['ENGINE ToolUse', '◠ adage: run make gen after a schema edit'])
      expect(await row('t4')).toEqual(['ENGINE ToolUse'])
      const group = async (expanded: boolean) => {
        const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'ToolGroup', requestId: 't4', props: { ...GROUP(['t4', 't5']), isExpanded: expanded } })
        const lines = await texts(ui)
        await ui.unmount()
        return lines
      }
      expect(await group(false)).toEqual(['ENGINE ToolGroup', '◠ adage: run make gen after a schema edit'])
      expect(await group(true)).toEqual(['ENGINE ToolGroup'])
    }
  })
})

describe('T-B2-04, T-B2-05 · the markers under a tool row', () => {
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

describe('T-B2-06 · the spinner and the turn line', () => {
  test('T-B2-06: off by default; on, a suffix while a reflex fired this turn and the counts at the turn end', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], card: { ...CARD, ui: { spinner: true, turn_line: true } }, hook: { TurnStep: PREDICT }, classic: { UserPromptSubmit: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w)
    const spin = { word: 'Thinking', message: null, suffix: '…', mode: 'requesting' } as const
    const turnLine = { word: 'Worked', durationMs: 108_000 }
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'Spinner', props: spin }))).toEqual(['ENGINE Spinner: Thinking…'])
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'TurnDuration', props: turnLine }))).toEqual(['ENGINE TurnDuration: Worked'])
    await prompt($, 'go')
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
    await prompt($, 'go')
    await drain($.turn.step(STEP))
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'Spinner', props: { word: 'Thinking', message: null, suffix: '…', mode: 'requesting' } }))).toEqual(['ENGINE Spinner: Thinking…'])
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'TurnDuration', props: { word: 'Worked', durationMs: 1000 } }))).toEqual(['ENGINE TurnDuration: Worked'])
  })
})
