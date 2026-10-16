// The active binary, found and asked (binary.ts; ux-design §9.2; T-B1-03, T-X-02): the plugin's
// data directory, from the engine's process or derived where the shim linked the binary, and every
// hook event through `askBinary`, which counts the binary's failed answers in a row and warns once
// at three.
import { describe, expect, test } from 'claude-code/testing'

import { UNANSWERING, askBinary, dataDir } from './binary'
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

describe('T-B1-03 · dataDir: where the shim linked the binary', () => {
  const CHECKOUT = '/home/me/src/adage/plugin/claude-code'
  const INLINE = '/cfg/plugins/data/adage-inline'

  test('CLAUDE_PLUGIN_DATA when the engine\'s process carries it, as given (the probe asks the binary)', async () => {
    expect(await dataDir(fake({ env: { pluginData: '/data/', configDir: '/cfg' } }).m)).toBe('/data')
  })

  test('a --plugin-dir plugin: `<config>/plugins/data/adage-inline`, taken once the shim linked bin/hook there', async () => {
    expect(await dataDir(fake({ root: CHECKOUT, env: { configDir: '/cfg', home: '/home/me' }, present: [`${INLINE}/bin/hook`] }).m)).toBe(INLINE)
    // Before the session-start shim ran (the mod's first look at a first session): none, inert.
    expect(await dataDir(fake({ root: CHECKOUT, env: { configDir: '/cfg' } }).m)).toBeUndefined()
    // CLAUDE_CONFIG_DIR set: only its data directory, never another config's link under HOME.
    const homeLink = '/home/me/.claude/plugins/data/adage-inline/bin/hook'
    expect(await dataDir(fake({ root: CHECKOUT, env: { configDir: '/cfg', home: '/home/me' }, present: [homeLink] }).m)).toBeUndefined()
    // Unset: `~/.claude`, by HOME.
    expect(await dataDir(fake({ root: CHECKOUT, env: { home: '/home/me' }, present: [homeLink] }).m)).toBe('/home/me/.claude/plugins/data/adage-inline')
    // Nothing to derive from: none.
    expect(await dataDir(fake({ root: CHECKOUT }).m)).toBeUndefined()
  })

  test('on Windows: USERPROFILE where there is no HOME, backslashes kept, the shim\'s hook.exe', async () => {
    const win = 'C:\\Users\\me\\.claude\\plugins\\data\\adage-inline'
    const f = fake({ root: 'C:\\src\\adage\\plugin\\claude-code', env: { userProfile: 'C:\\Users\\me', os: 'Windows_NT' }, present: [`${win}\\bin\\hook.exe`] })
    expect(await dataDir(f.m)).toBe(win)
    // Git Bash's HOME first, then USERPROFILE: the one the shim linked in.
    const g = fake({ root: 'C:\\src\\adage', env: { home: '/c/Users/me', userProfile: 'C:\\Users\\me', os: 'Windows_NT' }, present: [`${win}\\bin\\hook.exe`] })
    expect(await dataDir(g.m)).toBe(win)
  })

  test('`adage init`\'s local marketplace: the folder read in place, keyed `adage@adage-local`, data `adage-adage-local`', async () => {
    const known = JSON.stringify({ 'adage-local': { source: { source: 'directory', path: '/ah/plugin' }, installLocation: '/ah/plugin' } })
    const installed = JSON.stringify({ version: 2, plugins: { 'adage@adage-local': [{ scope: 'user', installPath: '/cfg/plugins/cache/adage-local/adage/0.1.0' }] } })
    const files = { '/cfg/plugins/known_marketplaces.json': known, '/cfg/plugins/installed_plugins.json': installed }
    const local = '/cfg/plugins/data/adage-adage-local'
    expect(await dataDir(fake({ root: '/ah/plugin/adage', env: { configDir: '/cfg' }, files, present: [`${local}/bin/hook`] }).m)).toBe(local)
    // Its link before the inline one, were both there.
    const both = [`${local}/bin/hook`, `${INLINE}/bin/hook`]
    expect(await dataDir(fake({ root: '/ah/plugin/adage', env: { configDir: '/cfg' }, files, present: both }).m)).toBe(local)
    // No link yet (the shim has not run): none.
    expect(await dataDir(fake({ root: '/ah/plugin/adage', env: { configDir: '/cfg' }, files }).m)).toBeUndefined()
    // A --plugin-dir checkout is not that marketplace's: the other install's link is never taken.
    expect(await dataDir(fake({ root: CHECKOUT, env: { configDir: '/cfg' }, files, present: [`${local}/bin/hook`] }).m)).toBeUndefined()
  })

  test('the published marketplace read from a directory (`adage@adage`): `adage-adage`', async () => {
    const known = JSON.stringify({ adage: { source: { source: 'directory', path: '/src/adage' }, installLocation: '/src/adage' } })
    const data = '/home/me/.claude/plugins/data/adage-adage'
    const f = fake({ root: '/src/adage/plugin/claude-code', env: { home: '/home/me' }, files: { '/home/me/.claude/plugins/known_marketplaces.json': known }, present: [`${data}/bin/hook`] })
    expect(await dataDir(f.m)).toBe(data)
  })

  test('an installed plugin: the data directory its root says, never the --plugin-dir one', async () => {
    const root = 'C:\\Users\\me\\.claude\\plugins\\cache\\adage\\adage\\0.1.0'
    const installed = 'C:\\Users\\me\\.claude\\plugins\\data\\adage-adage'
    const inline = 'C:\\Users\\me\\.claude\\plugins\\data\\adage-inline'
    const both = [`${installed}\\bin\\hook.exe`, `${inline}\\bin\\hook.exe`]
    expect(await dataDir(fake({ root, env: { userProfile: 'C:\\Users\\me', os: 'Windows_NT' }, present: both }).m)).toBe(installed)
    expect(await dataDir(fake({ root, env: { userProfile: 'C:\\Users\\me', os: 'Windows_NT' }, present: [both[1] ?? ''] }).m)).toBeUndefined()
  })
})
