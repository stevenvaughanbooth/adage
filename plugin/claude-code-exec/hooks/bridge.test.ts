// The bridge's outcomes, with a fake `$.process.run` (bridge.ts takes the runner as a value).
import type { ProcessRunInit, ProcessRunResult } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { ALLOWED_VERBS, ask, dataDirFromRoot, guard, hookBinary, isAllowed, probe } from './bridge'
import type { Run } from './bridge'

type Seen = { argv: readonly string[]; init: ProcessRunInit | undefined }

const exited = (exitCode: number, stdout: string, extra: Partial<ProcessRunResult> = {}): ProcessRunResult => ({
  exitCode,
  stdout,
  stderr: '',
  isStdoutTruncated: false,
  isStderrTruncated: false,
  ...extra,
})

/** A guarded fake runner answering `answer`, recording every call that reached it. */
function fake(answer: (argv: readonly string[]) => ProcessRunResult | Error): { run: Run; seen: Seen[] } {
  const seen: Seen[] = []
  const raw: Run = async (argv, init) => {
    seen.push({ argv, init })
    const a = answer(argv)
    if (a instanceof Error) throw a
    return a
  }
  return { run: guard(raw), seen }
}

const BIN = '/data/bin/adage'
const PAYLOAD = { session_id: 's', cwd: '/w', hook_event_name: 'TurnStep', turn_id: 't', index: 1, model: 'm', ending: false }

describe('ask', () => {
  test('exit 0 with a JSON object is the answer', async () => {
    const { run } = fake(() => exited(0, '{"predict":{"kind":"run"}}\n'))
    expect(await ask(run, BIN, 'TurnStep', PAYLOAD, 250)).toEqual({ ok: true, answer: { predict: { kind: 'run' } } })
  })

  test('exit 0 with empty stdout is nothing to do', async () => {
    const { run } = fake(() => exited(0, '  \n'))
    expect(await ask(run, BIN, 'TurnStep', PAYLOAD, 250)).toEqual({ ok: true, answer: undefined })
  })

  test('a non-zero exit is a failure, whatever it printed', async () => {
    const { run } = fake(() => exited(1, '{"predict":{}}'))
    expect(await ask(run, BIN, 'TurnStep', PAYLOAD, 250)).toEqual({ ok: false })
  })

  test('a run that throws (cannot start, timed out) is a failure', async () => {
    const { run } = fake(() => new Error('timed out'))
    expect(await ask(run, BIN, 'TurnStep', PAYLOAD, 250)).toEqual({ ok: false })
  })

  test('non-JSON, a JSON non-object and a truncated stdout are failures', async () => {
    for (const r of [exited(0, 'predict: yes'), exited(0, '[1,2]'), exited(0, '42'), exited(0, 'null'), exited(0, '{}', { isStdoutTruncated: true })]) {
      const { run } = fake(() => r)
      expect(await ask(run, BIN, 'TurnStep', PAYLOAD, 250)).toEqual({ ok: false })
    }
  })

  test('no binary is a failure and runs nothing', async () => {
    const { run, seen } = fake(() => exited(0, '{}'))
    expect(await ask(run, undefined, 'TurnStep', PAYLOAD, 250)).toEqual({ ok: false })
    expect(seen).toHaveLength(0)
  })

  test('runs `<bin> hook claude <Event>` with the payload on stdin, the cwd, the timeout and CLAUDECODE=1', async () => {
    const { run, seen } = fake(() => exited(0, ''))
    await ask(run, BIN, 'TurnStep', PAYLOAD, 250)
    expect(seen).toEqual([
      { argv: [BIN, 'hook', 'claude', 'TurnStep'], init: { stdin: JSON.stringify(PAYLOAD), timeoutMs: 250, cwd: '/w', env: { CLAUDECODE: '1' } } },
    ])
  })
})

describe('guard: the allow-list and the environment', () => {
  test('the allow-list is hook and version, nothing else', () => {
    expect(ALLOWED_VERBS).toEqual(['hook', 'version'])
    expect(isAllowed([BIN, 'version'])).toBe(true)
    expect(isAllowed([BIN, 'hook', 'claude', 'TurnStep'])).toBe(true)
    for (const argv of [
      [BIN],
      [BIN, 'version', 'extra'],
      [BIN, 'hook', 'claude'],
      [BIN, 'hook', 'opencode', 'Stop'],
      [BIN, 'hook', 'claude', 'Turn Step'],
      [BIN, 'hook', 'claude', 'TurnStep', 'more'],
      [BIN, 'delegate', 'on'],
      [BIN, 'review', 'approve'],
      [BIN, 'install'],
      [BIN, 'config'],
      [BIN, 'on'],
      [BIN, 'run', 'make', 'gen'],
    ]) {
      expect(isAllowed(argv), argv.join(' ')).toBe(false)
    }
  })

  test('an argv off the list is refused before any process starts', async () => {
    const { run, seen } = fake(() => exited(0, 'ok'))
    await expect(run([BIN, 'delegate', 'on'])).rejects.toThrow('not allowed')
    expect(seen).toHaveLength(0)
  })

  test('CLAUDECODE=1 is always set and wins over the caller', async () => {
    const { run, seen } = fake(() => exited(0, 'adage 1'))
    await run([BIN, 'version'], { env: { CLAUDECODE: '0', OTHER: 'x' } })
    expect(seen[0]?.init?.env).toEqual({ CLAUDECODE: '1', OTHER: 'x' })
  })
})

describe('probe', () => {
  test('a binary that prints its version serves', async () => {
    const { run, seen } = fake(() => exited(0, 'adage 0.1.0\n'))
    expect(await probe(run, BIN)).toBe(BIN)
    expect(seen).toEqual([{ argv: [BIN, 'version'], init: { timeoutMs: 1000, env: { CLAUDECODE: '1' } } }])
  })

  test('scripts/quiet (exit 0, nothing printed), a failure, a throw and no binary: inert', async () => {
    expect(await probe(fake(() => exited(0, '')).run, BIN)).toBeUndefined()
    expect(await probe(fake(() => exited(127, 'adage 1')).run, BIN)).toBeUndefined()
    expect(await probe(fake(() => new Error('ENOENT')).run, BIN)).toBeUndefined()
    expect(await probe(fake(() => exited(0, 'adage 1')).run, undefined)).toBeUndefined()
  })
})

describe('where the binary is', () => {
  test('CLAUDE_PLUGIN_DATA first, no PATH fallback', () => {
    expect(hookBinary('/d/', '/anywhere')).toBe('/d/bin/adage-exec')
    expect(hookBinary(undefined, '/home/me/src/adage/plugin/claude-code')).toBeUndefined()
    expect(hookBinary('', '/home/me/src/adage/plugin/claude-code')).toBeUndefined()
  })

  test('derived from an installed plugin root as Claude Code lays out plugins/data', () => {
    expect(dataDirFromRoot('/home/me/.claude/plugins/cache/adage/adage/0.1.0')).toBe('/home/me/.claude/plugins/data/adage-adage')
    expect(dataDirFromRoot('/c/plugins/cache/my.market/adage-exec/1.2.3/')).toBe('/c/plugins/data/adage-exec-my-market')
    expect(hookBinary(undefined, '/c/plugins/cache/adage/adage/0.1.0')).toBe('/c/plugins/data/adage-adage/bin/adage-exec')
  })
})
