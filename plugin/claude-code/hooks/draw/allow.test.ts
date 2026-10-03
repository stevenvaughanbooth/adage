// The grant pane drawn (draw/allow.tsx and its site; ux-design §4.17 `ask.allow.pane`,
// `ask.allow.first`, `ask.allow.details`; T-B4-03): the three plain lines, the diff as Code, the
// details and the terminal command, the acts as Buttons.
import { describe, expect, test } from 'claude-code/testing'

import { BANNED, CLI, GRAPH_OFFER, LINE, NEXT_TOKEN, SESSION, STAGE, SURFACES, TOKEN, begin, mountPane, personOnly, stdin, turnEnd, world } from '../fixtures/kit'
import { FIRST_LINE } from '../grant'

const FEED = [LINE.learnerIdle, LINE.healthy, LINE.start, LINE.pressIssued]
const ALLOW_Q = 'Adage can run make gen after a schema edit, instead of asking the agent to. Allow it?'
const STAGE_ARGV = [CLI, 'review', 'delegate', 'acme/learned', 'codegen-after-schema', '--level', 'e2', '--stage', '--json']
const CONFIRM_ARGV = [CLI, 'review', 'delegate', 'acme/learned', 'codegen-after-schema', '--level', 'e2', '--confirm']

describe('T-B4-03 · the grant pane', () => {
  test('see what it would do stages, opens the focused grant pane with every fact; allow confirms against the digest', async ($, on) => {
    const w = world(on, { feed: FEED, graph: GRAPH_OFFER, ask: () => 'see what it would do' })
    await begin($, w)
    await turnEnd($, w)
    expect(w.asked).toEqual([{ question: ALLOW_Q, options: ['see what it would do', 'not now'] }])
    expect(personOnly(w).map(r => r.argv)).toEqual([STAGE_ARGV])
    expect(stdin(personOnly(w)[0])).toEqual({ press: TOKEN, session: SESSION })
    expect(w.opened.at(-1)).toEqual({ id: 'adage-allow', title: 'adage · allow', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 })
    for (const surface of SURFACES) {
      const ui = await mountPane($, surface, 120, 'adage-allow')
      const t = (await ui.findAll({ type: 'Text' })).map(x => x.text)
      expect(t.slice(0, 5)).toEqual(['run make gen after a schema edit', 'it runs    make gen', 'it changes acme/generated/models.py  (+1 −0, shown below)', 'it never   touches anything else, or the network', FIRST_LINE])
      const code = (await ui.findAll({ type: 'Code' }))[0]
      expect(code?.props.format).toBe('diff')
      expect(String(code?.props.source)).toContain('+    currency: str = "USD"')
      const buttons = (await ui.findAll({ type: 'Button' })).map(b => `${b.props.hotkey}:${b.text}`)
      expect(buttons).toEqual(['a:allow', 'n:not now', 'd:details'])
      for (const x of t) expect(x).not.toMatch(BANNED)
      await ui.press({ key: 'details' })
      const details = (await ui.findAll({ type: 'Text' })).map(x => x.text)
      expect(details).toContain('argv      make gen')
      expect(details).toContain('script    scripts/gen.sh')
      expect(details).toContain('sandbox   Linux, Landlock ABI 7 + seccomp; network off')
      expect(details).toContain('hash      profile p9f8e7d6')
      expect(details).toContain('takes effect in your next session')
      expect((await ui.findAll({ type: 'Button' })).map(b => `${b.props.hotkey}:${b.text}`)).toContain('c:Copy')
      await ui.press({ key: 'copy' })
      expect(w.copies.at(-1)).toBe('adage review delegate acme/learned codegen-after-schema --level e2 --stage --json')
      await ui.press({ key: 'details' })
      await ui.unmount()
    }
    const ui = await mountPane($, 'terminal', 120, 'adage-allow')
    await ui.press({ key: 'allow' })
    await w.clock.settle()
    expect(personOnly(w).map(r => r.argv)).toEqual([STAGE_ARGV, CONFIRM_ARGV])
    expect(stdin(personOnly(w)[1])).toEqual({ expect: STAGE.digest, press: NEXT_TOKEN, session: SESSION })
    expect(w.toasts).toContain('allowed: run make gen after a schema edit · from your next session')
    expect(w.closed).toEqual(['adage-allow'])
  })
})
