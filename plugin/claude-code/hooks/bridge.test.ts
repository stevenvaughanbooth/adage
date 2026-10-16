// The bridge's outcomes, with a fake `$.process.run` (bridge.ts takes the runner as a value).
import type { ProcessRunInit, ProcessRunResult, ProcessSpawnRequest } from 'claude-code'
import { describe, expect, test } from 'claude-code/testing'

import { ALLOWED_VERBS, NOTE_TIMEOUT_MS, ask, cliBinary, configDirs, dataDirCandidates, dataDirFromRoot, exeSuffix, guard, guardSpawn, headOf, hookBinary, inlineDataDirs, isAllowed, note, probe, read, registeredKeys, say, sepOf } from './bridge'
import type { Run } from './bridge'
import { WIRE_NAMES, wire, wireLine } from './fixtures/kit'

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

const BIN = '/data/bin/hook'
const PAYLOAD = { session_id: 's', cwd: '/w', hook_event_name: 'TurnStep', turn_id: 't', index: 1, model: 'm', ending: false }

describe('T-B1-01 · ask', () => {
  test('exit 0 with a JSON object is the answer: every answer on the wire, as the binary prints it', async () => {
    for (const name of WIRE_NAMES) {
      const { run } = fake(() => exited(0, wireLine(name)))
      expect(await ask(run, BIN, 'TurnStep', PAYLOAD, 250), name).toEqual({ ok: true, answer: wire(name) })
    }
  })

  test('exit 0 with empty stdout is nothing to do', async () => {
    const { run } = fake(() => exited(0, '  \n'))
    expect(await ask(run, BIN, 'TurnStep', PAYLOAD, 250)).toEqual({ ok: true, answer: undefined })
  })

  test('a non-zero exit is a failure, whatever it printed', async () => {
    const { run } = fake(() => exited(1, wireLine('predict')))
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

describe('T-X-04, T-B1-05 · guard: the allow-list and the environment', () => {
  const CLI = '/data/bin/adage'
  test('every verb in its one exact shape (T-X-04); the person-only verbs, install and config never', () => {
    expect(ALLOWED_VERBS).toEqual(['hook', 'version', 'watch', 'graph', 'doctor', 'status', 'ledger', 'off', 'map', 'condition', 'delegate'])
    for (const argv of [
      [BIN, 'version'],
      [BIN, 'hook', 'claude', 'TurnStep'],
      [CLI, 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '/w', '--session', 's1'],
      [CLI, 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '/w', '--session', 's1', '--since', '1~0~0~'],
      [CLI, 'graph', '--json'],
      [CLI, 'doctor', '--json'],
      [CLI, 'status', '--json'],
      [CLI, '--json', '--for-session'],
      [CLI, 'ledger', 'note'],
      [CLI, 'off'],
      [CLI, 'map', '--port', '0'],
      [CLI, 'condition'],
      [CLI, 'delegate', 'status', '--json'],
    ]) {
      expect(isAllowed(argv), argv.join(' ')).toBe(true)
    }
    for (const argv of [
      [BIN],
      [BIN, 'version', 'extra'],
      [BIN, 'hook', 'claude'],
      [BIN, 'hook', 'opencode', 'Stop'],
      [BIN, 'hook', 'claude', 'Turn Step'],
      [BIN, 'hook', 'claude', 'TurnStep', 'more'],
      [CLI, 'watch'],
      [CLI, 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '/w', '--session', '--since'],
      [CLI, 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '/w', '--session', 's1', '--since'],
      [CLI, 'watch', '--follow', '--json', '--stdin', 'ignore', '--repo', '', '--session', 's1'],
      // The follower reads no stdin: a child the engine started with a closed one keeps following.
      [CLI, 'watch', '--follow', '--json', '--repo', '/w', '--session', 's1'],
      [CLI, 'watch', '--follow', '--json', '--stdin', 'eof', '--repo', '/w', '--session', 's1'],
      [CLI, 'graph'],
      [CLI, 'doctor', '--json', '--fix'],
      [CLI, '--json'],
      [CLI, 'ledger', 'show'],
      [CLI, 'off', '--now'],
      [CLI, 'map'],
      [CLI, 'map', '--port', '8080'],
      [CLI, 'condition', '--auto'],
      [CLI, 'delegate', 'on'],
      [CLI, 'delegate', 'policy', '--max-level', 'e2'],
      [CLI, 'delegate', 'predict', 'on'],
      [CLI, 'review', 'approve', 'acme/learned', 'r'],
      [CLI, 'install'],
      [CLI, 'install', 'claude-code', '--write'],
      [CLI, 'config', 'set', 'learn.auto', 'true'],
      [CLI, 'on'],
      [CLI, 'add', 'pack', '--trust'],
      [BIN, 'run', 'make', 'gen'],
    ]) {
      expect(isAllowed(argv), argv.join(' ')).toBe(false)
    }
  })

  test('the heads T-X-04 counts: the status card reads as status, the ledger as ledger note', () => {
    expect(headOf([CLI, '--json', '--for-session'])).toBe('status')
    expect(headOf([CLI, 'ledger', 'note'])).toBe('ledger note')
    expect(headOf([CLI, 'watch', '--follow'])).toBe('watch')
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

  test('the spawner holds the same list and sets CLAUDECODE=1', () => {
    const seen: ProcessSpawnRequest[] = []
    const spawn = guardSpawn(request => {
      seen.push(request)
      return {} as never
    })
    expect(() => spawn({ argv: [CLI, 'review', 'approve'] })).toThrow('not allowed')
    spawn({ argv: [CLI, 'map', '--port', '0'], env: { CLAUDECODE: '' } })
    expect(seen).toEqual([{ argv: [CLI, 'map', '--port', '0'], env: { CLAUDECODE: '1' } }])
  })
})

describe('T-B1-01, T-X-02 · read and say: the --json reads and the lines a verb prints', () => {
  const CLI = '/data/bin/adage'
  test('one JSON object is the read; anything else keeps what was drawn', async () => {
    expect(await read(fake(() => exited(0, '{"checks":[]}')).run, CLI, ['doctor', '--json'], '/w')).toEqual({ ok: true, json: { checks: [] } })
    for (const r of [exited(1, '{}'), exited(0, ''), exited(0, '[1]'), exited(0, 'nope'), exited(0, '{}', { isStdoutTruncated: true })]) {
      expect(await read(fake(() => r).run, CLI, ['doctor', '--json'], '/w')).toEqual({ ok: false })
    }
    expect(await read(fake(() => new Error('x')).run, CLI, ['doctor', '--json'], '/w')).toEqual({ ok: false })
    expect(await read(fake(() => exited(0, '{}')).run, undefined, ['doctor', '--json'], '/w')).toEqual({ ok: false })
  })

  test('a read runs in the session directory with a bound', async () => {
    const { run, seen } = fake(() => exited(0, '{}'))
    await read(run, CLI, ['graph', '--json'], '/w')
    expect(seen).toEqual([{ argv: [CLI, 'graph', '--json'], init: { timeoutMs: 10_000, cwd: '/w', env: { CLAUDECODE: '1' } } }])
  })

  test('say: the line on exit 0, nothing otherwise', async () => {
    expect(await say(fake(() => exited(0, 'adage is off\n')).run, CLI, ['off'], '/w')).toBe('adage is off')
    expect(await say(fake(() => exited(2, 'x')).run, CLI, ['off'], '/w')).toBeUndefined()
    expect(await say(fake(() => exited(0, 'x')).run, CLI, ['on'], '/w')).toBeUndefined()
  })
})

describe('T-B1-02 · probe', () => {
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

describe('T-B1-03 · where the binary is', () => {
  test('`<data>/bin/hook` in the data directory; none without one, no PATH fallback', () => {
    expect(hookBinary('/d/')).toBe('/d/bin/hook')
    expect(hookBinary(undefined)).toBeUndefined()
    expect(hookBinary('')).toBeUndefined()
  })

  test('derived from an installed plugin root as Claude Code lays out plugins/data, either separator', () => {
    expect(dataDirFromRoot('/home/me/.claude/plugins/cache/adage/adage/0.1.0')).toBe('/home/me/.claude/plugins/data/adage-adage')
    expect(dataDirFromRoot('/c/plugins/cache/my.market/adage-exec/1.2.3/')).toBe('/c/plugins/data/adage-exec-my-market')
    expect(dataDirFromRoot('C:\\Users\\me\\.claude\\plugins\\cache\\adage\\adage\\0.1.0')).toBe('C:\\Users\\me\\.claude\\plugins\\data\\adage-adage')
    expect(dataDirFromRoot('C:/Users/me/.claude/plugins/cache/adage/adage/0.1.0')).toBe('C:/Users/me/.claude/plugins/data/adage-adage')
    expect(dataDirFromRoot('/home/me/src/adage/plugin/claude-code')).toBeUndefined()
    expect(hookBinary(dataDirFromRoot('/c/plugins/cache/adage/adage/0.1.0'))).toBe('/c/plugins/data/adage-adage/bin/hook')
  })

  test('a --plugin-dir plugin: `<config>/plugins/data/<name>-inline`, CLAUDE_CONFIG_DIR alone when set, else each home\'s .claude', () => {
    expect(inlineDataDirs('adage', '/cfg/', ['/home/me'])).toEqual(['/cfg/plugins/data/adage-inline'])
    expect(inlineDataDirs('adage', undefined, ['/home/me', undefined])).toEqual(['/home/me/.claude/plugins/data/adage-inline'])
    expect(inlineDataDirs('adage', undefined, [undefined, 'C:\\Users\\me'])).toEqual(['C:\\Users\\me\\.claude\\plugins\\data\\adage-inline'])
    expect(inlineDataDirs('adage', undefined, ['/c/Users/me', 'C:\\Users\\me'])).toEqual([
      '/c/Users/me/.claude/plugins/data/adage-inline',
      'C:\\Users\\me\\.claude\\plugins\\data\\adage-inline',
    ])
    expect(inlineDataDirs('adage', undefined, ['/home/me', '/home/me'])).toEqual(['/home/me/.claude/plugins/data/adage-inline'])
    expect(inlineDataDirs('my.mod', '/cfg', [])).toEqual(['/cfg/plugins/data/my-mod-inline'])
    expect(inlineDataDirs('adage', undefined, [undefined, ''])).toEqual([])
    expect(configDirs('/cfg/', ['/home/me'])).toEqual(['/cfg'])
  })

  // What `adage init` leaves (read live from Claude Code 2.1.292): a directory marketplace,
  // `adage-local` at $ADAGE_HOME/plugin, whose plugin the engine reads in place.
  const KNOWN = JSON.stringify({
    'adage-local': { source: { source: 'directory', path: '/ah/plugin' }, installLocation: '/ah/plugin', lastUpdated: '2026-10-07T00:55:18.766Z' },
    other: { source: { source: 'github', repo: 'x/y' }, installLocation: '/cfg/plugins/marketplaces/other' },
  })
  const INSTALLED = JSON.stringify({
    version: 2,
    plugins: {
      'adage@adage-local': [{ scope: 'user', installPath: '/cfg/plugins/cache/adage-local/adage/0.1.0', version: '0.1.0' }],
      'adage@adage': [{ scope: 'user', installPath: '/home/me/src/adage/plugin/claude-code' }],
      'adagex@adage': [{ scope: 'user', installPath: '/ah/plugin/adage' }],
    },
  })

  test('the key the engine registered the plugin\'s folder under: a marketplace that holds it, or an install at it', () => {
    expect(registeredKeys('/ah/plugin/adage', 'adage', KNOWN, INSTALLED)).toEqual(['adage@adage-local'])
    expect(registeredKeys('/ah/plugin/adage/', 'adage', JSON.parse(KNOWN), undefined)).toEqual(['adage@adage-local'])
    expect(registeredKeys('/home/me/src/adage/plugin/claude-code', 'adage', KNOWN, INSTALLED)).toEqual(['adage@adage'])
    expect(registeredKeys('/elsewhere/adage', 'adage', KNOWN, INSTALLED)).toEqual([])
    // Windows: either separator, a drive in any case.
    const winKnown = { 'adage-local': { installLocation: 'C:\\Users\\me\\AppData\\Roaming\\Adage\\plugin' } }
    expect(registeredKeys('c:/users/me/appdata/roaming/adage/plugin/adage', 'adage', winKnown, undefined)).toEqual(['adage@adage-local'])
    // Unreadable or malformed files: none.
    for (const bad of [undefined, '', 'not json', '[]', 'null', { 'adage-local': 3 }]) expect(registeredKeys('/ah/plugin/adage', 'adage', bad, bad)).toEqual([])
  })

  test('the candidates: an installed cache root alone; else each config\'s registered keys, then <name>-inline', () => {
    expect(dataDirCandidates('/home/me/.claude/plugins/cache/adage/adage/0.1.0', 'adage', [{ config: '/cfg', known: KNOWN, installed: INSTALLED }])).toEqual(['/home/me/.claude/plugins/data/adage-adage'])
    expect(dataDirCandidates('/home/me/src/adage/plugin/claude-code', 'adage', [{ config: '/home/me/.claude' }])).toEqual(['/home/me/.claude/plugins/data/adage-inline'])
    expect(dataDirCandidates('/ah/plugin/adage', 'adage', [{ config: '/cfg', known: KNOWN, installed: INSTALLED }])).toEqual([
      '/cfg/plugins/data/adage-adage-local',
      '/cfg/plugins/data/adage-inline',
    ])
    expect(dataDirCandidates('C:\\ah\\plugin\\adage', 'adage', [{ config: 'C:\\Users\\me\\.claude', known: { 'adage-local': { installLocation: 'C:\\ah\\plugin' } } }])).toEqual([
      'C:\\Users\\me\\.claude\\plugins\\data\\adage-adage-local',
      'C:\\Users\\me\\.claude\\plugins\\data\\adage-inline',
    ])
  })

  test('the CLI verbs come from `adage` beside the hook binary, never from PATH', () => {
    expect(cliBinary('/d')).toBe('/d/bin/adage')
    expect(cliBinary(undefined)).toBeUndefined()
  })

  test('on Windows the entries the shim copies are hook.exe and adage.exe, under the data directory\'s own separator', () => {
    expect(exeSuffix('Windows_NT')).toBe('.exe')
    expect(exeSuffix(undefined)).toBe('')
    expect(exeSuffix('Linux')).toBe('')
    expect(hookBinary('C:/Users/me/.claude/plugins/data/adage-adage', exeSuffix('Windows_NT'))).toBe('C:/Users/me/.claude/plugins/data/adage-adage/bin/hook.exe')
    expect(cliBinary('C:/Users/me/.claude/plugins/data/adage-adage', exeSuffix('Windows_NT'))).toBe('C:/Users/me/.claude/plugins/data/adage-adage/bin/adage.exe')
    expect(hookBinary('C:\\Users\\me\\.claude\\plugins\\data\\adage-inline\\', '.exe')).toBe('C:\\Users\\me\\.claude\\plugins\\data\\adage-inline\\bin\\hook.exe')
    expect(hookBinary('/d', exeSuffix(undefined))).toBe('/d/bin/hook')
    expect([sepOf('C:\\x'), sepOf('\\\\server\\share'), sepOf('C:/x'), sepOf('/home/me')]).toEqual(['\\', '\\', '/', '/'])
  })
})

describe('T-A4-02 · note (`adage ledger note`)', () => {
  const CLI_BIN = '/data/bin/adage'
  test('the note on stdin, in the session directory, CLAUDECODE=1; exit 0 is recorded', async () => {
    const { run, seen } = fake(() => exited(0, ''))
    expect(await note(run, CLI_BIN, { session_id: 's', steps: [] }, '/w')).toBe(true)
    expect(seen).toEqual([{ argv: [CLI_BIN, 'ledger', 'note'], init: { stdin: '{"session_id":"s","steps":[]}', timeoutMs: NOTE_TIMEOUT_MS, cwd: '/w', env: { CLAUDECODE: '1' } } }])
  })

  test('a refusal (exit 2), a run that cannot start and no binary are not recorded, and never throw', async () => {
    expect(await note(fake(() => exited(2, '')).run, CLI_BIN, {}, '/w')).toBe(false)
    expect(await note(fake(() => new Error('ENOENT')).run, CLI_BIN, {}, '/w')).toBe(false)
    expect(await note(fake(() => exited(0, '')).run, undefined, {}, '/w')).toBe(false)
  })
})
