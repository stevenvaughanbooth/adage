// Every site Adage draws in, through the engine (draw/sites.ts; test-plan §6 B2): mounted on the
// terminal and the desktop from the same state (T-X-07), the opening line (T-B2-01), the words a
// default surface never carries and the theme's roles only (T-B2-10). The trees each site returns
// are tested in draw/{rows,band,card,allow,pane}.test.ts.
import { describe, expect, test } from 'claude-code/testing'

import { BAND, BANNED, DELEGATE, LAPSED, LINE, OUTPUT, PREDICT, PREDICTED_ID, STEP, SURFACES, TOOL, USER, block, drain, prompt, start, texts, world } from '../fixtures/kit'
import { ROLES } from '../tokens'

describe('T-B2-01 · the opening line', () => {
  test("T-B2-01: the engine's notice redrawn with the glyph; another notice untouched", async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle] })
    await start($, w)
    const ui = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'InfoNotice', props: { text: 'adage: 3 reflexes and 4 instincts active in this repo. Nothing runs without you.', command: null } })
    expect(await texts(ui)).toEqual(['ENGINE InfoNotice: ◠ adage · 3 reflexes and 4 instincts active in this repo. Nothing runs without you.'])
    const other = await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'InfoNotice', props: { text: 'Using a model from settings', command: null } })
    expect(await texts(other)).toEqual(['ENGINE InfoNotice: Using a model from settings'])
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
    await prompt($, 'fix W-2042')
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
