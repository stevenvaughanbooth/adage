// The pane's Graph tab (docs/claude-mod/ux-design.md §6.0): this session's graph, drawn from
// view/graph.ts's lines, each piece a Text in its role. No Button sits inside a line (a native one
// would break a box's borders on the desktop), so the lines are the same tree on both surfaces; the
// one press is the footer's, which shows the whole session or follows the newest again.
import { ROLES } from '../tokens'
import type { Seg } from '../view/pane'
import { g } from '../view/text'
import type { GraphDrawn } from '../view/graph'
import { GRAPH_EMPTY } from '../view/graph'
import type { Els, Tree } from './els'

/** One drawn line: each piece a Text in its role (none: the terminal's own ink), dim or bold. */
function segRow(els: Els, line: readonly Seg[], key: string): Tree {
  const { Box, Text } = els
  if (line.length === 0) return <Text key={key}> </Text>
  return (
    <Box key={key} flexDirection="row">
      {line.filter(s => s.text !== '').map((s, k) => (
        <Text key={`${key}-${k}`} {...(s.role === undefined ? {} : { color: ROLES[s.role] })} {...(s.dim ? { dimColor: true } : {})} {...(s.bold ? { bold: true } : {})}>
          {s.text}
        </Text>
      ))}
    </Box>
  )
}

/** What the Graph tab shows besides the lines: why it cannot grow here, and the footer's state. */
export type GraphTab = {
  drawn: GraphDrawn
  /** The shim's reason when no binary could serve (ux-design §9.1): nothing is observed, so nothing grows. */
  nobinary: string
  /** No bridge on this surface (a Desktop without `$.process`, ux-design §4.12). */
  noBridge: boolean
  /** The whole session (the person scrolls), or the newest followed. */
  history: boolean
  /** `a`: the whole session, or follow the newest again. */
  onHistory: () => void
  ascii: boolean
}

export function graphTree(els: Els, t: GraphTab): Tree {
  const { Box, Text, Button } = els
  const empty = t.drawn.lines.length === 0
  return (
    <Box flexDirection="column">
      {t.nobinary === '' ? null : <Text color={ROLES.prime} wrap="wrap">{t.nobinary}</Text>}
      {t.noBridge ? <Text dimColor>{`the graph needs the terminal ${g('sep', t.ascii)} adage watch`}</Text> : null}
      {segRow(els, t.drawn.head, 'graph-head')}
      {empty ? (
        <Text key="graph-empty" dimColor wrap="wrap">
          {GRAPH_EMPTY}
        </Text>
      ) : null}
      {t.drawn.lines.map((l, k) => segRow(els, l, `graph-${k}`))}
      {empty ? null : (
        <Box flexDirection="row" gap={2}>
          <Button key="history" plain hotkey="a" label={t.history ? 'follow the newest' : 'whole session'} onPress={() => t.onHistory()} />
        </Box>
      )}
    </Box>
  )
}
