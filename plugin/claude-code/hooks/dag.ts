// This session's graph (the pane's Graph tab, docs/claude-mod/ux-design.md §6.0): what the command
// hooks saw, in the order they saw it (each prompt the person submitted, each tool batch on the main
// loop, each call that failed, each reflex Adage delivered, each stop), kept in `$.state.dag` and
// capped so a long session keeps its latest. The observers (observe.ts) append; view/graph.ts lays
// it out. Nothing is decided here: every node is a copy of what a hook saw or the binary answered.
import type { Mod } from './mod'
import type { GraphNode } from './plugin'
import { isRecord } from './shape'

/** The most nodes kept; the oldest go first (a turn that loses its prompt draws without its box). */
export const MAX_NODES = 300

/** How long the newest node glows in the Graph tab before it settles (no glow under reduced motion). */
export const PULSE_MS = 3000

/** The most characters a lone call's word keeps (the tab cuts it to the pane). */
const DETAIL = 160

/**
 * Appends nodes to the graph, for a person at the prompt only (nothing is drawn headless), and,
 * unless motion is reduced, redraws once the newest has settled so its glow goes.
 */
export async function keepNodes(m: Mod, nodes: readonly GraphNode[]): Promise<void> {
  if (nodes.length === 0 || !m.rt.interactive) return
  await m.state.dag.update(cur => [...cur, ...nodes].slice(-MAX_NODES))
  if ((await m.state.mode.read()).reducedMotion) return
  m.rt.pulse?.cancel()
  m.rt.pulse = m.clock.after(PULSE_MS + 100, () => {
    m.rt.pulse = undefined
    m.ui.invalidate()
  })
}

/** A path's last two parts (`schema/invoice.json`), the rest of it known from the repository. */
export function shortPath(path: string): string {
  const parts = path.split(/[\\/]/).filter(p => p !== '')
  return parts.slice(-2).join('/')
}

/** The one word that tells a lone call apart: a Bash call's command, a file's path, a pattern, a skill. */
export function detailOf(tool: string, input: unknown): string {
  const s = (k: string): string => {
    const v = isRecord(input) ? input[k] : undefined
    return typeof v === 'string' ? v.trim() : ''
  }
  const said = (() => {
    switch (tool) {
      case 'Bash':
        return s('command').split('\n')[0] ?? ''
      case 'Read':
      case 'Edit':
      case 'Write':
      case 'MultiEdit':
        return shortPath(s('file_path'))
      case 'NotebookEdit':
        return shortPath(s('notebook_path'))
      case 'Grep':
      case 'Glob':
        return s('pattern')
      case 'Skill':
        return s('skill')
      case 'Agent':
      case 'Task':
        return s('description') || s('subagent_type')
      case 'WebFetch':
        return s('url').replace(/^https?:\/\//, '').split('/')[0] ?? ''
      case 'WebSearch':
        return s('query')
      default:
        return ''
    }
  })()
  return said.replace(/\s+/g, ' ').slice(0, DETAIL)
}

/** A call of a batch, as `PostToolBatch` hands it over (anything malformed reads as nothing). */
type Call = { tool_name?: unknown; tool_input?: unknown; tool_use_id?: unknown }

/**
 * A tool batch's node: one name per call, the calls' ids, the word on a lone call, and when the hooks
 * beneath began and ended. Undefined for a batch with no call that names an id.
 */
export function toolsNode(calls: readonly Call[], from: number, at: number): GraphNode | undefined {
  const named = calls.filter((c): c is Call & { tool_use_id: string } => typeof c.tool_use_id === 'string' && c.tool_use_id !== '')
  if (named.length === 0) return undefined
  const tools = named.map(c => (typeof c.tool_name === 'string' && c.tool_name !== '' ? c.tool_name : 'tool'))
  const [only] = named
  const detail = named.length === 1 && only !== undefined ? detailOf(tools[0] ?? '', only.tool_input) : ''
  return { kind: 'tools', at, from, tools, ids: named.map(c => c.tool_use_id), detail }
}
