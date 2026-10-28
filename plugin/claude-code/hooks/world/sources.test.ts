// The connector sources resolved against the session's MCP servers (world/sources.ts; T-W3-04):
// the descriptors as the binary prints them, a server's name as a tool name spells it, globs over
// it, and the first provider served winning.
import { describe, expect, test } from 'claude-code/testing'

import { DESCRIPTORS } from '../fixtures/kit'
import { descriptorsOf, matchServer, resolveSources, secs, serverKey, toolParts } from './sources'

describe('T-W3-04 · descriptors and servers', () => {
  test('T-W3-04: the descriptors read with their seconds; one without a name, a kind or a known need is left out, as is a provider without a server or a tool', () => {
    const d = descriptorsOf(DESCRIPTORS)
    expect(d.map(x => [x.name, x.kind, x.needs, x.every, x.lookback, x.lookahead, x.providers.length])).toEqual([
      ['calendar', 'calendar_event', 'connector', 300, 7200, 28_800, 1],
      ['github', 'pr', 'connector', 600, 7 * 86_400, 0, 1],
      ['meetings', 'transcript', 'local', 30, 0, 0, 0],
    ])
    expect(descriptorsOf({ descriptors: [{ source: { name: 'x', kind: 'pr' } }, { source: { name: '', kind: 'pr', needs: 'connector' } }, { source: { name: 'y', kind: 'pr', needs: 'connector' }, provider: [{ server: 'a' }, { tool: 'b' }, { server: 'a', tool: 'b' }] }] }).map(x => [x.name, x.providers.length])).toEqual([['y', 1]])
    expect(descriptorsOf('nope')).toEqual([])
    expect([secs('300s', 1), secs('15m', 1), secs('2h', 1), secs('1d', 1), secs('x', 7), secs(undefined, 7)]).toEqual([300, 900, 7200, 86_400, 7, 7])
  })

  test('T-W3-04: a server name as a tool name spells it, globs over it, and a tool name taken apart', () => {
    expect(serverKey('claude.ai Google Calendar')).toBe('claude_ai_google_calendar')
    expect(serverKey('GitHub')).toBe('github')
    expect(matchServer('claude.ai Google Calendar', 'claude_ai_google_calendar')).toBe(true)
    expect(matchServer('github*', 'github_enterprise')).toBe(true)
    expect(matchServer('github*', 'my_github')).toBe(false)
    expect(matchServer('*linear*', 'acme_linear_cloud')).toBe(true)
    expect(toolParts('mcp__github__search_pull_requests')).toEqual({ server: 'github', tool: 'search_pull_requests' })
    expect(toolParts('Bash')).toBeUndefined()
  })

  test('T-W3-04: a connector descriptor resolves to the first provider a connected server serves; a local one never; none connected, no sources', () => {
    const d = descriptorsOf(DESCRIPTORS)
    const tools = [{ name: 'Bash' }, { name: 'mcp__github__search_pull_requests', mcp: true }, { name: 'mcp__github__list_issues', mcp: true }, { name: 'mcp__claude_ai_google_calendar__list_events', mcp: true }]
    expect(resolveSources(d, tools).map(r => [r.descriptor.name, r.server, r.tool])).toEqual([
      ['calendar', 'claude_ai_google_calendar', 'list_events'],
      ['github', 'github', 'search_pull_requests'],
    ])
    expect(resolveSources(d, [{ name: 'mcp__github__list_issues' }])).toEqual([])
    expect(resolveSources(d, [])).toEqual([])
  })
})
