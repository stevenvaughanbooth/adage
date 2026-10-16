// `/adage` and `/adage why` drawn in their output row (ux-design §7): the status card's header,
// its key column and `next`, the terminal-only commands behind `Copy`; or the last directive.
// The text the command answered is the source, so a headless session and the VS Code panel read
// the same words as plain text.
import { ROLES } from '../tokens'
import { parseCardText } from '../view/card'
import type { Els, Tree } from './els'

export function cardTree(els: Els, text: string, copy: (command: string, surface: string) => void): Tree {
  const { Box, Text, Button } = els
  const { header, rows } = parseCardText(text)
  return (
    <Box flexDirection="column">
      <Text bold wrap="truncate-end">
        {header}
      </Text>
      {rows.map((r, k) => {
        const command = r.key === 'next' || (r.key === '' && /^adage \S/.test(r.text)) ? (/^(adage [^·]+?)\s+·/.exec(r.text)?.[1] ?? '') : ''
        return (
          <Box key={`row-${k}`} flexDirection="row">
            <Text dimColor>{r.key.padEnd(12)}</Text>
            <Text color={command !== '' || r.text.startsWith('/adage') ? ROLES.command : ROLES.ink} wrap="truncate-end">
              {r.text}
            </Text>
            {command === '' ? null : <Button key={`copy-next-${k}`} label="Copy" plain dimColor onPress={e => copy(command, e.surface)} />}
          </Box>
        )
      })}
    </Box>
  )
}

export function whyTree(els: Els, text: string): Tree {
  const { Box, Text } = els
  const [first = '', ...rest] = text.split('\n')
  return (
    <Box flexDirection="column">
      <Text color={ROLES.accent} wrap="wrap">
        {first}
      </Text>
      {rest.map((line, k) => (
        <Text key={`why-${k}`} dimColor={k === rest.length - 1} wrap="wrap">
          {line === '' ? ' ' : line}
        </Text>
      ))}
    </Box>
  )
}
