// The band above the prompt through the engine (draw/band.tsx and its site; ux-design §5; test-plan
// §6 B2: T-B2-07 … T-B2-11): the cells as the session goes, narrowing, quiet, off, paused, no
// binary, the attention line, the lamp's roles, ASCII and reduced motion.
import { describe, expect, test } from 'claude-code/testing'

import { BAND, DELEGATE, LAPSED, LINE, PREDICT, STEP, SURFACES, TOOL, bandText, drain, prompt, start, texts, world } from '../fixtures/kit'
import { ROLES } from '../tokens'

describe('T-B2-07 … T-B2-11 · the band', () => {
  test('T-B2-07: the cells as the session goes; at 60 columns the lowest go first, the lamp word stays', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerWaiting, LINE.healthy, LINE.firedGen], hook: { TurnStep: PREDICT }, graph: { routes: [] } })
    await start($, w)
    await drain($.turn.step(STEP))
    await w.clock.advance(1000)
    for (const surface of SURFACES) {
      expect(await bandText($, surface)).toBe('◠ adage  1 reflex this session · 1 step without a model request · ● healthy · learning in 37 s')
      expect(await bandText($, surface, 60)).toBe('◠ adage  1 reflex this session · ● healthy')
    }
  })

  test('T-B2-07: a quiet session leaves the band to the engine; so does a survey and a session outside a repository', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], graph: { routes: [] } })
    await start($, w)
    for (const surface of SURFACES) {
      expect(await bandText($, surface)).toBe('ENGINE AbovePrompt')
      const survey = await $.ui.mount({ plugin: 'adage', surface, component: 'AbovePrompt', props: { ...BAND(), hasSurvey: true } })
      expect(await texts(survey)).toEqual(['ENGINE AbovePrompt'])
    }
  })

  test('T-B2-07: outside a repository (no learner line) the band is the engine', async ($, on) => {
    const w = world(on, { feed: [LINE.healthy, LINE.firedGen], graph: { routes: [] } })
    await start($, w)
    for (const surface of SURFACES) expect(await bandText($, surface)).toBe('ENGINE AbovePrompt')
  })

  test('T-B2-07: off, paused, no binary', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.off] })
    await start($, w)
    for (const surface of SURFACES) expect(await bandText($, surface)).toBe('◠ adage ○ off in this repository · /adage on')
    await $.command.run({ command: 'adage', args: 'pause' } as never)
    for (const surface of SURFACES) expect(await bandText($, surface)).toBe('◠ adage  paused · /adage pause')
  })

  test('T-B2-07, §9.1: no binary: the band says so, the status says why, once; nothing else is drawn', async ($, on) => {
    const w = world(on, { version: '', status: 'no binary for darwin-arm64; reinstall with /plugin install adage@adage, or npm i -g @adage-sh/cli' })
    await start($, w)
    await $.classic.SessionStart({ source: 'startup' } as never)
    for (const surface of SURFACES) expect(await bandText($, surface)).toBe('◠ adage ○ off · run adage doctor')
    expect(w.statuses).toEqual(['no binary for darwin-arm64; reinstall with /plugin install adage@adage, or npm i -g @adage-sh/cli'])
    expect(w.spawned).toEqual([])
    expect(await texts(await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'ToolUse', requestId: 't1', props: TOOL('t1') }))).toEqual(['ENGINE ToolUse'])
  })

  test('T-B2-08: a lapsed grant on the second line, over a review waiting; its toast once', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], delegate: { ...DELEGATE, lapsed: LAPSED } })
    await start($, w)
    for (const surface of SURFACES) expect(await bandText($, surface)).toBe('◠ adage  ● healthy» 1 grant lapsed · /adage health')
    expect(w.toasts).toEqual(['adage\nacme/learned/codegen-after-schema is no longer allowed: argv_templates changed · /adage review'])
  })

  test('T-B2-08: review waiting, then a problem over it', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], later: [LINE.problem] })
    await start($, w)
    expect(await bandText($, 'terminal')).toBe('◠ adage  ● healthy» Adage has 1 suggestion · /adage review')
    await w.deliver()
    expect(await bandText($, 'terminal')).toBe('◠ adage  ● problem: hook latency» problem: hook latency · /adage health')
  })

  test('T-B2-08, §9.6: a newer harness, until the first prompt, once per version', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], harness: '2.1.300', graph: { routes: [] } })
    await start($, w)
    expect(await bandText($, 'terminal')).toBe('◠ adage  ● healthy» Claude Code 2.1.300 is newer than this plugin was tested with (2.1.288) · run adage doctor')
    await prompt($, 'go')
    expect(await bandText($, 'terminal')).toBe('ENGINE AbovePrompt')
  })

  test('L8: a harness version the store cannot keep still clears the line, and the prompt hook answers as beneath', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy], harness: '2.1.300', graph: { routes: [] }, storeFails: ['adage.harness'], classic: { UserPromptSubmit: { additionalContext: ['beneath'] } } })
    await start($, w)
    expect(await $.classic.UserPromptSubmit({ prompt: 'go', source: 'user' } as never)).toEqual({ additionalContext: ['beneath'] })
    await w.clock.settle()
    expect(w.store['adage.harness']).toBeUndefined()
    // The hook went on past the failed write: the line is cleared for the session.
    expect(w.writes.filter(x => x.key === 'mode').map(x => (x.value as { versionLine?: string }).versionLine).at(-1)).toBe('')
    expect(await bandText($, 'terminal')).toBe('ENGINE AbovePrompt')
  })

  test('T-B2-09: the lamp in the fire, prime and inhibit roles, its word beside it', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.check], later: [LINE.problem], graph: { routes: [] } })
    await start($, w)
    const lamp = async () => (await (await $.ui.mount({ plugin: 'adage', surface: 'terminal', component: 'AbovePrompt', props: BAND() })).findAll({ type: 'Text', text: /●/ }))[0]?.props.color
    expect(await bandText($, 'terminal')).toBe('◠ adage  ● check: hooks: claude-code')
    expect(await lamp()).toBe(ROLES.prime)
    await w.deliver()
    expect(await lamp()).toBe(ROLES.inhibit)
  })

  test('T-B2-11: ADAGE_ASCII picks every twin; reduced motion schedules no redraw', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle, LINE.healthy, LINE.firedGen], env: { ADAGE_ASCII: '1', ADAGE_REDUCED_MOTION: '1' }, graph: { routes: [] } })
    await start($, w)
    expect(await bandText($, 'terminal')).toBe('~ adage  1 reflex this session - * healthy')
    expect(w.invalidated).toBe(0)
  })
})
