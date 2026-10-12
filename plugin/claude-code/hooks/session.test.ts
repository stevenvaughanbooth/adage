// The session's life through the engine (session.ts; test-plan §6 B1, §4.9): the probe at
// session.start and again once the shim ran, inert with no binary, the Windows entries, and a
// /clear (a new session id) that starts the counts again and follows the new id.
import { describe, expect, test } from 'claude-code/testing'

import { GRAPH, LINE, PREDICT, STEP, bandText, block, drain, mountPane, shown, start, world } from './fixtures/kit'
import { GRAPH_EMPTY } from './view/graph'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.firedGen, LINE.firedLookup]

describe('T-B1-02, T-B1-03 · activation', () => {
  test('active: the presence probe runs `<data>/bin/hook version` with CLAUDECODE=1', async ($, on) => {
    const w = world(on, { model: 'read' })
    await start($, w, null, false)
    expect(w.ran[0]).toEqual({ argv: ['/data/bin/hook', 'version'], init: { timeoutMs: 1000, env: { CLAUDECODE: '1' } } })
    // Then the session's standing text, off the hook's path (attach.ts).
    expect(w.ran.slice(1).map(r => r.argv.slice(1).join(' '))).toEqual(['hook claude Standing'])
  })

  test('active whichever binary the shim linked: adage-exec answers the same presence probe', async ($, on) => {
    const w = world(on, { version: 'adage-exec 0.1.0\n', hook: { TurnStep: PREDICT }, model: 'read' })
    await start($, w, null, false)
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks.map(c => c.kind)).toEqual(['tool', 'input', 'stop'])
    expect(w.models.map(x => x.index)).toEqual([])
    // The probe, the session's Standing and the step's two phases (the dry run, the verdict) ask
    // the hook binary; the routine model is read from `adage` itself.
    expect(w.ran.filter(r => r.argv[1] !== '--json').map(r => r.argv[0])).toEqual(['/data/bin/hook', '/data/bin/hook', '/data/bin/hook', '/data/bin/hook'])
    expect(w.ran.filter(r => r.argv[1] === '--json').map(r => r.argv)).toEqual([['/data/bin/adage', '--json', '--for-session']])
  })

  test('inert with no binary (the link is scripts/quiet: version prints nothing): every hook forwards, no TurnStep asked', async ($, on) => {
    const w = world(on, { version: '', hook: { TurnStep: PREDICT }, model: 'read', classic: { PostToolBatch: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w, null, false)
    expect(await $.classic.PostToolBatch({ tool_calls: [] } as never)).toEqual({ additionalContext: [block('codegen-after-schema')] })
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks.map(c => c.kind)).toEqual(['text', 'tool', 'input', 'stop'])
    expect(w.models.map(x => x.index)).toEqual([2])
    expect(w.ran.map(r => r.argv[1])).toEqual(['version'])
    expect(w.sets).toEqual([])
  })

  test('no CLAUDE_PLUGIN_DATA, a checkout root and nowhere to derive a data directory from: nothing runs, inert, no PATH fallback', async ($, on) => {
    const w = world(on, { data: undefined, hook: { TurnStep: PREDICT }, model: 'read' })
    await start($, w, null, false)
    const { chunks } = await drain($.turn.step(STEP))
    expect(chunks).toHaveLength(4)
    expect(w.ran).toEqual([])
  })

  test('a --plugin-dir plugin (no CLAUDE_PLUGIN_DATA in the engine\'s process): the data directory the engine gives it, once the shim linked bin/hook there', async ($, on) => {
    // `claude --plugin-dir <dir>` with CLAUDE_CONFIG_DIR=/cfg: the engine keys the plugin
    // `adage@inline` and hands its command hooks /cfg/plugins/data/adage-inline (the live log).
    const LINK = '/cfg/plugins/data/adage-inline/bin/hook'
    let linked = false
    const shim = () => {
      linked = true
      return {}
    }
    const w = world(on, { data: undefined, env: { CLAUDE_CONFIG_DIR: '/cfg', HOME: '/home/me' }, present: p => linked && p === LINK, hook: { TurnStep: PREDICT }, model: 'read', classic: { SessionStart: shim } })
    await start($, w, null, false)
    expect(w.ran).toEqual([]) // a first session: no link yet, nothing run
    expect((await drain($.turn.step(STEP))).chunks).toHaveLength(4) // inert: forwarded
    await $.classic.SessionStart({ source: 'startup' } as never)
    expect(w.ran[0]).toEqual({ argv: [LINK, 'version'], init: { timeoutMs: 1000, env: { CLAUDECODE: '1' } } })
    expect((await drain($.turn.step(STEP))).chunks).toHaveLength(3) // active: predicted
    expect(w.ran.find(r => r.argv[3] === 'TurnStep')?.argv[0]).toBe(LINK)
    expect(w.models.map(x => x.index)).toEqual([2])
  })

  test('a --plugin-dir plugin with no CLAUDE_CONFIG_DIR: `~/.claude/plugins/data/adage-inline`, by HOME', async ($, on) => {
    const LINK = '/home/me/.claude/plugins/data/adage-inline/bin/hook'
    const w = world(on, { data: undefined, env: { HOME: '/home/me' }, present: p => p === LINK, hook: { TurnStep: PREDICT }, model: 'read' })
    await start($, w, null, false)
    expect(w.ran.map(r => r.argv.slice(0, 2))).toEqual([[LINK, 'version'], [LINK, 'hook']])
    expect((await drain($.turn.step(STEP))).chunks.map(c => c.kind)).toEqual(['tool', 'input', 'stop'])
  })

  test('a link the SessionStart shim makes after session.start is probed again once the shim ran', async ($, on) => {
    let linked = false
    const shim = () => {
      linked = true
      return {}
    }
    const w = world(on, { hook: { TurnStep: PREDICT }, linked: () => linked, model: 'read', classic: { SessionStart: shim } })
    await start($, w, null, false)
    expect((await drain($.turn.step(STEP))).chunks).toHaveLength(4) // inert: forwarded
    await $.classic.SessionStart({ source: 'startup' } as never)
    expect((await drain($.turn.step(STEP))).chunks).toHaveLength(3) // active: predicted
    expect(w.models.map(x => x.index)).toEqual([2])
  })

  test('L4 · on Windows the probe and the verbs use the .exe entries the shim copies', async ($, on) => {
    const w = world(on, { data: 'C:/Users/me/.claude/plugins/data/adage-adage', hook: { TurnStep: PREDICT }, env: { OS: 'Windows_NT' }, card: {}, model: 'read' })
    await start($, w, null, false)
    await drain($.turn.step(STEP))
    expect(w.ran[0]?.argv).toEqual(['C:/Users/me/.claude/plugins/data/adage-adage/bin/hook.exe', 'version'])
    expect(w.ran.find(r => r.argv[3] === 'TurnStep')?.argv[0]).toBe('C:/Users/me/.claude/plugins/data/adage-adage/bin/hook.exe')
    await w.clock.settle()
    expect(w.ran.find(r => r.argv[1] === '--json')?.argv[0]).toBe('C:/Users/me/.claude/plugins/data/adage-adage/bin/adage.exe')
  })
})

describe('T-B3-08, T-B3-13, §4.9 · /clear', () => {
  test('a new session id: the follower follows it, its counts start again, its graph starts empty, the old rows are no longer its own', async ($, on) => {
    let id = 'sess-1'
    const w = world(on, { feed: FEED, graph: { routes: GRAPH.routes.slice(0, 3) }, session: () => id, classic: { UserPromptSubmit: { additionalContext: [block('codegen-after-schema')] } } })
    await start($, w)
    await $.classic.UserPromptSubmit({ prompt: 'go', source: 'user' } as never)
    const band = () => bandText($, 'terminal', 120, `b-${id}`)
    expect(await band()).toBe('◠ adage  2 reflexes this session · ● healthy')
    const pane = await mountPane($, 'terminal')
    expect(await shown(pane)).not.toContain(GRAPH_EMPTY)
    id = 'sess-2'
    await $.classic.SessionStart({ source: 'clear', session_id: 'sess-2', cwd: '/w' } as never)
    await w.clock.settle()
    expect(w.spawned.filter(a => a[1] === 'watch').at(-1)?.slice(8, 10)).toEqual(['--session', 'sess-2'])
    expect(await band()).toBe('ENGINE AbovePrompt')
    expect(await shown(pane)).toContain(GRAPH_EMPTY)
    expect((await $.command.run({ command: 'adage', args: 'why' } as never)).text).toBe('No reflex has spoken in this session yet.')
    // The new session's press was asked for with its id.
    expect(JSON.parse(w.ran.find(r => r.argv[3] === 'Press')?.init?.stdin ?? '{}')).toMatchObject({ session_id: 'sess-2', source: 'clear' })
  })
})
