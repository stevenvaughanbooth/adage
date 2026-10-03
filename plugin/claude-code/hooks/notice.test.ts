// What Adage noticed (notice.ts; plan D5; ux-design §4.7, §5; T-D5-01, T-D5-02): the noticed
// words, the count and the candidates' words kept current over the fake, and the noticed line and
// the learner's toasts through the engine on both surfaces.
import { describe, expect, test } from 'claude-code/testing'
import type { Engine } from 'claude-code/testing'

import { BANNED, CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE, CARD, GROUP, LINE, SESSION, SURFACES, T0, bandText, exited, fake, rowsOf as rows, start, texts, world } from './fixtures/kit'
import type { Surface } from './fixtures/kit'
import { noticedIds, noticedLine, noticedLines, recountNoticed, wordCandidates } from './notice'
import { candidatesOf } from './reads'

const CANDIDATES = candidatesOf({ candidates: [CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE] })
const NOTICED_CARD = { ...CARD, ui: { noticed: true } }
const FEED = [LINE.learnerIdle, LINE.healthy, LINE.noticed(9, 'run-a54d7c', ['t1', 't2']), LINE.noticed(10, 'run-a54d7c', ['t3']), LINE.noticed(11, 'lookup-then-read', ['t4'], { review: true, support: 4, s_min: 5 }), LINE.noticed(12, 'run-single', ['t5'])]

describe('T-D5-01 · noticed, as words', () => {
  test('noticed.forming and noticed.review: plain counts, never a forecast', () => {
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 3, s_min: 5, review: false }, 'make test after editing acme/**', false)).toBe('○ adage noticed · make test after editing acme/** · seen in 3 earlier sessions')
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 1, s_min: 5, review: false }, 'x', false)).toBe('○ adage noticed · x · seen in 1 earlier session')
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 4, s_min: 5, review: true }, 'scripts/wi-lookup then reading the files it names', false)).toBe('○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review')
    expect(noticedLine({ act: 'noticed', candidate: 'x', support: 3, review: false }, 'x', true)).toBe('o adage noticed - x - seen in 3 earlier sessions')
  })

  test('T-D5-01 (pure): under the first batch only, by the calls the row names, else by when its hooks ran; a one-event record draws nothing', () => {
    const r = rows(...FEED.slice(2))
    expect(noticedLines(r, ['t1', 't2'], [], CANDIDATES, false)).toEqual(['○ adage noticed · after make test, run make lint · seen in 2 earlier sessions'])
    expect(noticedLines(r, ['t3'], [], CANDIDATES, false)).toEqual([])
    expect(noticedLines(r, ['t4'], [], CANDIDATES, false)).toEqual(['○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review'])
    expect(noticedLines(r, ['t5'], [], CANDIDATES, false)).toEqual([])
    expect(noticedLines(r, ['t1'], [], [], false)).toEqual([])
    // A row that names no call: the batch whose hooks ran when it was written.
    const unnamed = rows(JSON.stringify({ kind: 'decision', cursor: 'c', ts: T0 + 500, session: SESSION, harness: 'claude', event: 'tools', acts: [{ act: 'noticed', candidate: 'run-a54d7c', support: 2, s_min: 3, review: false }], texts: [] }))
    expect(noticedLines(unnamed, ['t9'], [{ ids: ['t8', 't9'], from: T0, to: T0 + 400 }], CANDIDATES, false)).toHaveLength(1)
    expect(noticedLines(unnamed, ['t9'], [{ ids: ['t8', 't9'], from: T0 + 5000, to: T0 + 6000 }], CANDIDATES, false)).toEqual([])
    expect(noticedIds(r)).toEqual(['run-a54d7c', 'lookup-then-read', 'run-single'])
  })

  test('the band\'s count is kept current: a change redraws, the same count does not', async () => {
    const f = fake({ state: { rows: rows(...FEED.slice(2)) } })
    await recountNoticed(f.m)
    expect(f.m.rt.noticed).toBe(3)
    expect(f.invalidated).toBe(1)
    await recountNoticed(f.m)
    expect(f.invalidated).toBe(1)
  })

  test('the candidates\' words are read once per refresh window, only for a candidate not known yet', async () => {
    const f = fake({ run: () => exited(0, JSON.stringify({ candidates: [CANDIDATE, CANDIDATE_REVIEW] })) })
    f.m.rt.cli = '/data/bin/adage'
    const r = rows(...FEED.slice(2))
    await wordCandidates(f.m, r)
    expect(f.ran.map(x => x.argv)).toEqual([['/data/bin/adage', 'status', '--json']])
    expect(f.state.candidates.map(c => c.id)).toEqual(['run-a54d7c', 'lookup-then-read'])
    await wordCandidates(f.m, r)
    expect(f.ran).toHaveLength(1)
    for (const l of noticedLines(r, ['t1', 't2'], [], f.state.candidates, false)) expect(l).not.toMatch(BANNED)
  })
})

describe('T-D5-01 · the noticed line through the engine', () => {
  async function group($: Engine, surface: Surface, ids: string[]) {
    const ui = await $.ui.mount({ plugin: 'adage', surface, component: 'ToolGroup', requestId: ids[0], props: GROUP(ids) })
    const t = await texts(ui)
    await ui.unmount()
    return t
  }

  test('once under the first batch of a sequence, worded from status --json, in both arms; nothing with ui.noticed off', async ($, on) => {
    const w = world(on, { feed: FEED, card: NOTICED_CARD, graph: { routes: [] }, status_json: { tracks: [], candidates: [CANDIDATE, CANDIDATE_REVIEW, CANDIDATE_SINGLE] } })
    await start($, w)
    // The first drawing finds the candidate's words not read yet, reads them, and redraws.
    await group($, 'terminal', ['t1', 't2'])
    await w.clock.settle()
    for (const surface of SURFACES) {
      expect(await group($, surface, ['t1', 't2'])).toEqual(['ENGINE ToolGroup', '○ adage noticed · after make test, run make lint · seen in 2 earlier sessions'])
      expect(await group($, surface, ['t3'])).toEqual(['ENGINE ToolGroup'])
      expect(await group($, surface, ['t4'])).toEqual(['ENGINE ToolGroup', '○ adage noticed · scripts/wi-lookup then reading the files it names · waits for your review · /adage review'])
      expect(await group($, surface, ['t5'])).toEqual(['ENGINE ToolGroup'])
      expect(await group($, surface, ['unmatched'])).toEqual(['ENGINE ToolGroup'])
    }
    expect(w.ran.filter(r => r.argv[1] === 'status').length).toBeGreaterThanOrEqual(2)
    // The band counts what was noticed while the pane is closed.
    expect(await bandText($)).toBe('◠ adage  ● healthy · 3 patterns noticed')
    await $.command.run({ command: 'adage', args: 'watch' } as never)
    expect(await bandText($)).toBe('ENGINE AbovePrompt')
  })

  test('off by default: the setting the binary reports draws the line', async ($, on) => {
    const w = world(on, { feed: FEED, status_json: { tracks: [], candidates: [CANDIDATE] } })
    await start($, w)
    await group($, 'terminal', ['t1', 't2'])
    await w.clock.settle()
    expect(await group($, 'terminal', ['t1', 't2'])).toEqual(['ENGINE ToolGroup'])
    expect(w.ran.filter(r => r.argv[1] === 'status').length).toBe(1)
  })
})

describe('T-D5-02 · the learner and its toasts through the engine', () => {
  test('learning in 38 s, then learning…; a strength change from this session or recent sessions; a candidate that formed; no change, nothing; a failed learn once', async ($, on) => {
    const w = world(on, { feed: [LINE.learnerWaiting, LINE.healthy, LINE.firedGen], later: [LINE.learnStrength(T0 + 10_000, [SESSION]), LINE.learnStrength(T0 + 10_001), LINE.learnNew(T0 + 10_002, [SESSION]), LINE.learnEvidence(T0 + 10_003), LINE.learnFailed(T0 + 10_004), LINE.learnFailed(T0 + 10_005), LINE.learnerRunning] })
    await start($, w)
    expect(await bandText($)).toContain('learning in 38 s')
    await w.deliver()
    expect(await bandText($)).toContain('learning…')
    expect(w.toasts).toEqual([
      'adage learned from this session\n◠ run make gen after a schema edit · now a direct instruction',
      'adage learned from recent sessions\n◠ run make gen after a schema edit · now a direct instruction',
      'adage learned from this session\n◠ run make test after editing acme/** · new, from this session',
      'adage could not learn from this session (sandbox) · run adage doctor',
    ])
  })
})
