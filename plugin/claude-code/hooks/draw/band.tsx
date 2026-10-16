// The band above the prompt (ux-design §5): one line, a second only while something needs
// attention. No Button (a digit typed alone into an empty prompt would press it); numbers plain;
// the lamp the only coloured glyph, its word always beside it.
import { ROLES } from '../tokens'
import type { Band } from '../view/band'
import type { Els, Tree } from './els'

export function bandTree(els: Els, band: Band, sep: string): Tree {
  const { Box, Text } = els
  return (
    <Box flexDirection="column">
      <Box flexDirection="row">
        {band.cells.map((c, k) => (
          <Box key={`cell-${k}`} flexDirection="row">
            {k === 0 ? null : <Text dimColor>{k === 1 ? '  ' : ` ${sep} `}</Text>}
            {c.glyph === undefined ? null : <Text color={ROLES[c.lamp ?? 'ink']}>{`${c.glyph} `}</Text>}
            <Text color={k === 0 ? ROLES.accent : ROLES.ink} wrap="truncate-end">
              {c.text}
            </Text>
          </Box>
        ))}
      </Box>
      {band.second === '' ? null : (
        <Text color={ROLES.ink2} wrap="truncate-end">
          {band.second}
        </Text>
      )}
    </Box>
  )
}
