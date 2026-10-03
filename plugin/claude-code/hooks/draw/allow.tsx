// The grant pane (ux-design §4.17 `ask.allow.pane`, plan B4): what `see what it would do` opens,
// focused. Three plain lines, the staged diff as `Code`, and the second act as a Button: `a allow`
// confirms against the digest the stage produced, `n not now` closes it, `d details` shows what
// the terminal shows a person today, `c copy` the terminal command. Pure: allow.ts makes the
// actions; grant.ts words the lines.
import { allowDetails, allowLines, diffText } from '../grant'
import type { Allow } from '../plugin'
import { ROLES } from '../tokens'
import type { Els, Tree } from './els'
import { rule } from './rows'

export type AllowActions = {
  /** Confirms the grant against the stage's digest (the press). */
  allow: () => void
  /** Closes the pane; nothing runs. */
  notNow: () => void
  details: () => void
  copy: (surface: string) => void
}

export function allowTree(els: Els, a: Allow, columns: number, ascii: boolean, act: AllowActions): Tree {
  const { Box, Text, Button, Code } = els
  const [head = '', ...lines] = allowLines(a, ascii)
  const diff = diffText(a)
  const ready = a.stage !== null && a.stage.ok && !a.busy
  return (
    <Box flexDirection="column">
      <Text bold wrap="wrap">
        {head}
      </Text>
      {lines.map((l, k) => (
        <Text key={`allow-line-${k}`} wrap="wrap">
          {l}
        </Text>
      ))}
      {a.note === '' ? null : (
        <Text color={ROLES.prime} wrap="wrap">
          {a.note}
        </Text>
      )}
      {diff === '' ? null : rule(els, 'what it would do now', columns)}
      {diff === '' ? null : <Code key="diff" source={diff} format="diff" wrap="truncate-end" />}
      {!a.details ? null : rule(els, 'details', columns)}
      {!a.details
        ? null
        : allowDetails(a, ascii).map((l, k) => (
            <Text key={`allow-detail-${k}`} dimColor wrap="truncate-end">
              {l}
            </Text>
          ))}
      {!a.details ? null : (
        <Box flexDirection="row" gap={1}>
          <Text dimColor wrap="truncate-end">{a.command}</Text>
          <Button key="copy" plain dimColor hotkey="c" label="Copy" onPress={e => act.copy(e.surface)} />
        </Box>
      )}
      <Box flexDirection="row" justifyContent="space-between">
        <Box flexDirection="row" gap={2}>
          {ready ? <Button key="allow" plain hotkey="a" label="allow" onPress={() => act.allow()} /> : null}
          <Button key="not-now" plain hotkey="n" label="not now" onPress={() => act.notNow()} />
          {a.stage === null ? null : <Button key="details" plain hotkey="d" label="details" onPress={() => act.details()} />}
        </Box>
        <Text dimColor>esc: later</Text>
      </Box>
    </Box>
  )
}
