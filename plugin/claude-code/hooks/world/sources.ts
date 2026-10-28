// The connector sources a session can run (the implementation plan's "Connectors"): each
// descriptor's providers, in order, against the MCP servers the session has connected, read off
// `$.tool.list()` (a server's tools are named `mcp__<server>__<tool>`). The first provider whose
// server is connected and whose tool it lists is the source's; a descriptor with none is not a
// source in this session. Pure: poll.ts reads the tools and runs the result.
import { isRecord, records, str } from '../shape'

export type Provider = {
  /** The server as `/mcp` lists it, or a glob over it (`github*`, `*linear*`). */
  server: string
  tool: string
  args: unknown
  /** JSON pointers by field, and the `*_when` lists. */
  map: Readonly<Record<string, unknown>>
}

export type Descriptor = {
  name: string
  kind: string
  needs: 'connector' | 'local'
  /** Seconds. */
  every: number
  lookback: number
  lookahead: number
  providers: Provider[]
}

/** A descriptor matched to a connected server's tool. */
export type Resolved = { descriptor: Descriptor; provider: Provider; server: string; tool: string }

/** `adage world descriptors --json` read; anything malformed is left out. */
export function descriptorsOf(v: unknown): Descriptor[] {
  if (!isRecord(v)) return []
  return records(v.descriptors)
    .map(d => {
      const s = isRecord(d.source) ? d.source : {}
      const name = str(s.name)
      const kind = str(s.kind)
      const needs = s.needs === 'local' ? 'local' : s.needs === 'connector' ? 'connector' : undefined
      if (name === '' || kind === '' || needs === undefined) return undefined
      const providers = records(d.provider)
        .map(p => ({ server: str(p.server), tool: str(p.tool), args: p.args ?? {}, map: isRecord(p.map) ? p.map : {} }))
        .filter(p => p.server !== '' && p.tool !== '')
      return { name, kind, needs, every: secs(s.every, 300), lookback: secs(s.lookback, 0), lookahead: secs(s.lookahead, 0), providers }
    })
    .filter((d): d is Descriptor => d !== undefined)
}

/** `300s`, `15m`, `2h`, `1d` as seconds, else `dflt`. */
export function secs(v: unknown, dflt: number): number {
  const m = /^(\d+)([smhd])$/.exec(str(v).trim())
  if (m === null) return dflt
  const n = Number(m[1])
  return m[2] === 'm' ? n * 60 : m[2] === 'h' ? n * 3600 : m[2] === 'd' ? n * 86_400 : n
}

/** A server's name as a tool name spells it: lowercase, every other character `_`. */
export const serverKey = (name: string): string => name.toLowerCase().replace(/[^a-z0-9*]+/g, '_')

/** Whether a provider's server (a name or a glob) names the server a tool name carries. */
export function matchServer(pattern: string, key: string): boolean {
  const re = new RegExp(`^${serverKey(pattern).split('*').map(s => s.replace(/[.+?^${}()|[\]\\]/g, '\\$&')).join('.*')}$`)
  return re.test(key)
}

/** The server and tool a tool name carries (`mcp__github__list_pull_requests`), or undefined. */
export function toolParts(name: string): { server: string; tool: string } | undefined {
  const m = /^mcp__(.+?)__(.+)$/.exec(name)
  return m === null ? undefined : { server: m[1] ?? '', tool: m[2] ?? '' }
}

/** The sources this session can run: each connector descriptor's first provider the tools list serves. */
export function resolveSources(descriptors: readonly Descriptor[], tools: readonly { name: string; mcp?: boolean }[]): Resolved[] {
  const served = tools.map(t => toolParts(t.name)).filter((p): p is { server: string; tool: string } => p !== undefined)
  const out: Resolved[] = []
  for (const d of descriptors) {
    if (d.needs !== 'connector') continue
    for (const p of d.providers) {
      const hit = served.find(s => s.tool === p.tool && matchServer(p.server, s.server))
      if (hit !== undefined) {
        out.push({ descriptor: d, provider: p, server: hit.server, tool: hit.tool })
        break
      }
    }
  }
  return out
}
