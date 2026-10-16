// The observers of the command hooks (observe.ts; architecture §3.1): the command hooks' answer
// returned unchanged, and what it delivered kept for drawing. Through the engine, and over the
// fake `Mod` for what needs no engine.
import { describe, expect, test } from 'claude-code/testing'

import { LINE, block, fake, start, world } from './fixtures/kit'
import { onPreToolUse, onPrompt, onStop, onToolBatch, onToolFailure } from './observe'

const answered = <A>(a: A) => async () => a

describe('T-B2-02 · the observers through the engine', () => {
  test('active: a classic observer returns the command hooks\' answer unchanged and keeps what fired', async ($, on) => {
    const w = world(on, { classic: { PostToolBatch: { additionalContext: [block('codegen-after-schema')] }, Stop: { block: 'finish line' } } })
    await start($, w, null, false)
    expect(await $.classic.PostToolBatch({ tool_calls: [] } as never)).toEqual({ additionalContext: [block('codegen-after-schema')] })
    expect(await $.classic.Stop({ stop_hook_active: false })).toEqual({ block: 'finish line' })
    expect(w.sets).toEqual([
      {
        key: 'fired',
        value: [{ event: 'PostToolBatch', route: 'codegen-after-schema', track: 'acme/learned@1', kind: 'run', strength: 'fire' }],
      },
    ])
  })

  test('toast.gate: a Stop the binary blocked says once that the agent continues', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerIdle], classic: { Stop: { block: 'Run make gen before finishing.' } } })
    await start($, w)
    expect(await $.classic.Stop({ stop_hook_active: false } as never)).toEqual({ block: 'Run make gen before finishing.' })
    expect(w.toasts).toEqual(['adage: finish line · the agent continues once'])
  })
})

describe('T-B2-02, T-B2-04 · the observers over the fake', () => {
  test('a prompt from the person starts a turn: the counts begin where the lists stand, the reflexes are kept with the prompt', async () => {
    const f = fake({ state: { predicted: [{ turnId: 't', index: 1, route: 'r', kind: 'run', ids: ['x'], isEnding: false }] } })
    f.m.rt.bin = '/data/bin/hook'
    const a = { additionalContext: [block('codegen-after-schema')] }
    expect(await onPrompt(f.m, { prompt: 'fix W-2042', source: 'user' } as never, answered(a) as never)).toBe(a)
    expect(f.state.turn).toEqual({ submits: 1, fired: 0, predicted: 1, marks: 0 })
    expect(f.state.fired).toEqual([{ event: 'UserPromptSubmit', route: 'codegen-after-schema', track: 'acme/learned@1', kind: 'run', strength: 'fire', prompt: 1, promptText: 'fix W-2042' }])
  })

  test('a subagent or a hook-made prompt is not a turn; inert, nothing is kept', async () => {
    const f = fake()
    f.m.rt.bin = '/data/bin/hook'
    const a = { additionalContext: [block('x')] }
    await onPrompt(f.m, { prompt: 'p', source: 'user', agent_id: 'a1' } as never, answered(a) as never)
    await onPrompt(f.m, { prompt: 'p', source: 'hook' } as never, answered(a) as never)
    expect(f.state.turn.submits).toBe(0)
    f.m.rt.bin = undefined
    expect(await onToolFailure(f.m, {} as never, answered(a) as never)).toBe(a)
    expect(f.state.fired).toEqual([])
  })

  test('the version line is cleared for good at the first prompt, and the harness version kept', async () => {
    const f = fake({ harness: '2.1.300' })
    f.state.mode = { ...f.state.mode, versionLine: '» newer' }
    f.m.rt.bin = '/data/bin/hook'
    await onPrompt(f.m, { prompt: 'p', source: 'user' } as never, answered({}) as never)
    expect(f.state.mode.versionLine).toBe('')
    expect(f.store['adage.harness']).toBe('2.1.300')
  })

  test('a tool batch keeps its calls and when the hooks beneath ran; one with no ids keeps nothing', async () => {
    const f = fake()
    f.m.rt.bin = '/data/bin/hook'
    await onToolBatch(f.m, { tool_calls: [{ tool_use_id: 't1' }, { tool_use_id: '' }] } as never, answered({}) as never)
    await onToolBatch(f.m, { tool_calls: [] } as never, answered({}) as never)
    expect(f.state.batches).toHaveLength(1)
    expect(f.state.batches[0]?.ids).toEqual(['t1'])
  })

  test('a Stop the binary blocked is toasted only to a person at the prompt', async () => {
    const f = fake()
    f.m.rt.bin = '/data/bin/hook'
    await onStop(f.m, {} as never, answered({ block: 'go on' }) as never)
    expect(f.toasts).toEqual([])
    f.m.rt.interactive = true
    await onStop(f.m, {} as never, answered({ block: 'go on' }) as never)
    expect(f.toasts).toEqual(['adage: finish line · the agent continues once'])
  })

  test('T-B3-13: the graph grows as the hooks fire: the prompt, its reflexes, a batch and the reflex after it, a failure, the stop before its reflexes; a subagent\'s calls are not drawn', async () => {
    const f = fake()
    f.m.rt.bin = '/data/bin/hook'
    f.m.rt.interactive = true
    await onPrompt(f.m, { prompt: 'fix W-2042', source: 'user' } as never, answered({ additionalContext: [block('lookup-work-item')] }) as never)
    await onToolBatch(f.m, { tool_calls: [{ tool_name: 'Edit', tool_input: { file_path: '/w/schema/invoice.json' }, tool_use_id: 'e1' }] } as never, answered({ additionalContext: [block('codegen-after-schema')] }) as never)
    await onToolBatch(f.m, { agent_id: 'a1', tool_calls: [{ tool_name: 'Read', tool_use_id: 'r1' }] } as never, answered({ additionalContext: [block('x')] }) as never)
    await onToolFailure(f.m, { tool_name: 'Bash', tool_use_id: 'b1' } as never, answered({ additionalContext: [block('dead-end-unittest', 'inhibit')] }) as never)
    await onToolBatch(f.m, { tool_calls: [{ tool_name: 'Bash', tool_input: { command: 'python3 -m unittest' }, tool_use_id: 'b1' }] } as never, answered({}) as never)
    await onStop(f.m, {} as never, answered({ block: block('codegen-after-schema') }) as never)
    expect(f.state.dag.map(n => (n.kind === 'reflex' ? `reflex ${n.event} ${n.route} [${n.ids.join()}]` : n.kind === 'tools' ? `tools ${n.tools.join()} ${n.detail}` : n.kind === 'stop' ? `stop gate=${n.gate}` : n.kind === 'failed' ? `failed ${n.id}` : `prompt ${n.n} ${n.text}`))).toEqual([
      'prompt 1 fix W-2042',
      'reflex UserPromptSubmit lookup-work-item []',
      'tools Edit schema/invoice.json',
      'reflex PostToolBatch codegen-after-schema [e1]',
      'failed b1',
      'reflex PostToolUseFailure dead-end-unittest [b1]',
      'tools Bash python3 -m unittest',
      'stop gate=true',
      'reflex Stop codegen-after-schema []',
    ])
    // The subagent's reflex is still counted where the band counts (`fired`), just not drawn in the graph.
    expect(f.state.fired.map(x => x.route)).toContain('x')
  })

  test('PreToolUse: a rewrite and a receipt beside a Bash call are kept as its markers; another tool is not looked at', async () => {
    const f = fake()
    f.m.rt.bin = '/data/bin/hook'
    const a = { updatedInput: { command: '.venv/bin/python -m pytest -q' }, additionalContext: ['<adage-receipt operation="3f9c1a2b">x</adage-receipt>'] }
    expect(await onPreToolUse(f.m, { tool: 'Bash', command: 'pytest -q', tool_use_id: 't1' } as never, answered(a) as never)).toBe(a)
    await onPreToolUse(f.m, { tool: 'Read', command: 'x', tool_use_id: 't2' } as never, answered(a) as never)
    expect(f.state.marks).toEqual([
      { id: 't1', kind: 'rewrite', interpreter: '.venv/bin/python', what: 'test' },
      { id: 't1', kind: 'receipt', receipt: '3f9c' },
    ])
  })
})
