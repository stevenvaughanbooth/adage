// The lines Adage adds to the transcript's rows (ux-design §3, §4.2–§4.4, §10): one dim line under
// the engine's own drawing of a prompt, a tool row or its result, or the engine's drawing with one
// prop changed. Pure: register.ts reads the state, `view.ts` words the lines, these lay them out.
import { ROLES } from '../tokens'
import type { Els, Tree } from './els'

/** The engine's row, then one dim line per entry (`Box` column; ux-design §4, "How"). */
export function under(els: Els, engine: Tree, lines: readonly string[]): Tree {
  const { Box, Text } = els
  return (
    <Box flexDirection="column">
      {engine}
      {lines.map((line, k) => (
        <Text key={`adage-line-${k}`} dimColor wrap="truncate-end">
          {line}
        </Text>
      ))}
    </Box>
  )
}

/** The receipt card in place of a test run's output: `✓` in the fire role, then the second line dim. */
export function receipt(els: Els, lines: readonly [string, string]): Tree {
  const { Box, Text } = els
  return (
    <Box flexDirection="column">
      <Text color={ROLES.ink} wrap="wrap">
        {lines[0]}
      </Text>
      <Text dimColor wrap="wrap">
        {lines[1]}
      </Text>
    </Box>
  )
}

/** The engine's turn line with Adage's counts beside it (`TurnDuration`, `ui.turn_line`). */
export function beside(els: Els, engine: Tree, text: string): Tree {
  const { Box, Text } = els
  return (
    <Box flexDirection="row">
      {engine}
      <Text dimColor wrap="truncate-end">
        {text}
      </Text>
    </Box>
  )
}
