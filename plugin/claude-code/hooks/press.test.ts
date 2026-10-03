// The press (press.ts; plan B4; architecture §3.12; T-B4-04, T-B4-05, T-X-04): the person-only verbs
// in their exact shapes, the guarded runner (the token on stdin, never argv; CLAUDECODE=1), what a
// press came to, and the tamper signal. The press is reached only through a gesture.
import { describe, expect, test } from 'claude-code/testing'

import type { Run } from './bridge'
import { NEXT_TOKEN, TOKEN, exited } from './fixtures/kit'
import type { Ran } from './fixtures/kit'
import { ANSWER, BUTTON, PRESS_VERBS, guardPress, isPressAllowed, isToken, objectOf, press, pressArgv, tampered } from './press'
import type { Gesture } from './press'

const CLI = '/data/bin/adage'

/** A guarded runner answering `stdout` (exit `code`), recording what reached it. */
function runner(stdout: string, code = 0, stderr = ''): { run: Run; seen: Ran[] } {
  const seen: Ran[] = []
  return { run: guardPress(async (argv, init) => (seen.push({ argv, init }), exited(code, stdout, { stderr }))), seen }
}

describe('T-X-04, T-B4-04 · the person-only verbs', () => {
  test('each verb in its exact shapes; never a flag or a path out; install, config and the rest never', () => {
    expect(PRESS_VERBS).toEqual(['review', 'delegate', 'on', 'add'])
    for (const ok of [
      ['review', 'approve', 'acme/learned', 'test-after-acme-edit'],
      ['review', 'pin', 'acme/learned', 'r'],
      ['review', 'delegate', 'acme/learned', 'r', '--level', 'e2', '--stage', '--json'],
      ['review', 'delegate', 'acme/learned', 'r', '--level', 'e2', '--confirm'],
      ['review', 'delegate', 'acme/learned', 'r', '--predict'],
      ['delegate', 'on'],
      ['on'],
      ['add', '.adage/packs/acme', '--trust'],
    ]) expect(isPressAllowed([CLI, ...ok]), ok.join(' ')).toBe(true)
    for (const no of [
      ['install', 'claude-code'],
      ['config', 'set', 'x'],
      ['review', 'approve', '--all', 'r'],
      ['review', 'approve', 'acme/learned'],
      ['review', 'delegate', 'acme/learned', 'r', '--level', 'e3', '--confirm'],
      ['add', '/etc', '--trust'],
      ['add', '../out', '--trust'],
      ['add', '.', '--trust'],
      ['on', 'now'],
      ['delegate', 'off'],
      [],
    ]) expect(isPressAllowed([CLI, ...no]), no.join(' ')).toBe(false)
  })

  test('a terminal command as the argv a press runs; anything not an adage person-only command is none', () => {
    expect(pressArgv(CLI, 'adage review approve acme/learned r')).toEqual([CLI, 'review', 'approve', 'acme/learned', 'r'])
    expect(pressArgv(CLI, '  adage   on ')).toEqual([CLI, 'on'])
    expect(pressArgv(CLI, 'adage install claude-code --write')).toBeUndefined()
    expect(pressArgv(CLI, 'rm -rf /')).toBeUndefined()
  })
})

describe('T-B4-05 · the guarded runner and what a press came to', () => {
  test('the token and the session on stdin with CLAUDECODE=1; the verb\'s next token and output read back', async () => {
    const r = runner(`${JSON.stringify({ next: NEXT_TOKEN })}\nrecorded approve for r\n`)
    const p = await press(BUTTON, r.run, [CLI, 'on'], TOKEN, 'sess-1', '/w', { expect: 'd1' })
    expect(p).toEqual({ ok: true, next: NEXT_TOKEN, lines: ['recorded approve for r'], stdout: 'recorded approve for r\n' })
    expect(r.seen[0]?.init).toMatchObject({ cwd: '/w', env: { CLAUDECODE: '1' } })
    expect(JSON.parse(r.seen[0]?.init?.stdin ?? '{}')).toEqual({ expect: 'd1', press: TOKEN, session: 'sess-1' })
    expect(r.seen[0]?.argv.some(a => isToken(a))).toBe(false)
  })

  test('refused before the spend (no next line), failed after it (a next line, a non-zero exit), the reason from stderr', async () => {
    expect(await press(ANSWER, runner('', 1, 'error: a decision for a person\n').run, [CLI, 'on'], TOKEN, 's', '')).toEqual({ ok: false, refused: true, next: undefined, error: 'a decision for a person' })
    expect(await press(ANSWER, runner(`${JSON.stringify({ next: NEXT_TOKEN })}\n`, 2, 'error: nothing to do\n').run, [CLI, 'on'], TOKEN, 's', '')).toEqual({ ok: false, refused: false, next: NEXT_TOKEN, error: 'nothing to do' })
  })

  test('no gesture, no token, an argv off the list or no press on stdin: nothing runs', async () => {
    const r = runner('')
    expect(await press({ from: 'button' } as Gesture, r.run, [CLI, 'on'], TOKEN, 's', '')).toMatchObject({ ok: false, refused: true, error: 'not a press' })
    expect(await press(ANSWER, r.run, [CLI, 'on'], 'abc', 's', '')).toMatchObject({ ok: false, error: 'no press is held' })
    expect(await press(ANSWER, r.run, [CLI, 'on'], TOKEN, '', '')).toMatchObject({ ok: false, error: 'no press is held' })
    expect(await press(ANSWER, r.run, [CLI, 'install', 'x'], TOKEN, 's', '')).toMatchObject({ ok: false, refused: false })
    await expect(r.run([CLI, 'on'], { stdin: '{}' })).rejects.toThrow('no press on stdin')
    expect(r.seen).toEqual([])
  })

  test('the object a stage or confirm printed after the next line; tokens are 64 hex; more issues than tokens is tampering', () => {
    expect(objectOf(['prose', '{"ok":true}', '[1]'])).toEqual({ ok: true })
    expect(objectOf(['prose', '[1]'])).toBeUndefined()
    expect([isToken(TOKEN), isToken('A'.repeat(64)), isToken(7)]).toEqual([true, false, false])
    expect([tampered(2, 1), tampered(1, 1), tampered(0, 0)]).toEqual([true, false, false])
  })
})
