// The active binary, asked (binary.ts; ux-design §9.2; T-X-02): every hook event through
// `askBinary`, which counts the binary's failed answers in a row and warns once at three.
import { describe, expect, test } from 'claude-code/testing'

import { UNANSWERING, askBinary } from './binary'
import { exited, fake, wire, wireLine } from './fixtures/kit'

describe('T-X-02 · askBinary', () => {
  test('the answer, tagged; the run is `<bin> hook claude <Event>` with the payload on stdin', async () => {
    const f = fake({ run: () => exited(0, wireLine('predict')) })
    f.m.rt.bin = '/data/bin/hook'
    expect(await askBinary(f.m, 'TurnStep', { cwd: '/w' }, 250)).toEqual({ ok: true, answer: wire('predict') })
    expect(f.ran[0]?.argv).toEqual(['/data/bin/hook', 'hook', 'claude', 'TurnStep'])
    expect(f.ran[0]?.init).toMatchObject({ stdin: '{"cwd":"/w"}', timeoutMs: 250, cwd: '/w' })
  })

  test('three failures in a row warn once, to a person at the prompt; an answer in between starts the count again', async () => {
    let ok = false
    const f = fake({ run: () => exited(ok ? 0 : 1, '') })
    f.m.rt.bin = '/data/bin/hook'
    f.m.rt.interactive = true
    for (let k = 0; k < UNANSWERING - 1; k++) await askBinary(f.m, 'TurnStep', {}, 250)
    ok = true
    await askBinary(f.m, 'TurnStep', {}, 250)
    expect(f.m.rt.failures).toBe(0)
    ok = false
    for (let k = 0; k < UNANSWERING + 2; k++) expect(await askBinary(f.m, 'TurnStep', {}, 250)).toEqual({ ok: false })
    expect(f.statuses).toEqual(['hooks are not answering; run adage doctor'])
  })

  test('inert: a failure that runs nothing and counts nothing', async () => {
    const f = fake()
    expect(await askBinary(f.m, 'TurnStep', {}, 250)).toEqual({ ok: false })
    expect(f.ran).toEqual([])
    expect(f.m.rt.failures).toBe(0)
  })
})
