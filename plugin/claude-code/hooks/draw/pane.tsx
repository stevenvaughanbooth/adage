// The pane (ux-design §6): three tabs as plain Buttons with lowercase hotkeys (`l` Live, `r`
// Reflexes, `t` Health; `h` is `adage watch`'s help key), every tab ending with `Explore in
// browser`. Strength and lifecycle words are allowed here and nowhere else (§1.3). Nothing in the
// pane runs a person-only verb: a review verb is a command to copy until the press (plan B4), and
// the one action it runs is `adage condition`, which the binary marks as allowed from a session.
import type { Health, MapState, Reflex, Row, Tick, View, Check, Lapse } from '../plugin'
import { ROLES } from '../tokens'
import type { Waiting } from '../view'
import {
  VERB_WORDS,
  checkAction,
  checkLine,
  cut,
  g,
  lapseText,
  lightText,
  reflexLine,
  reflexesEmpty,
  reviewCommand,
  rowLines,
  tickText,
  visibleReflexes,
  visibleRows,
} from '../view'
import type { Els, Tree } from './els'

/** What a press, a pick or a keystroke in the pane does (register.ts binds each to `$`). */
export type PaneActions = {
  tab: (tab: View['tab']) => void
  /** The Live filter: `submit` false while typing, true on enter (an empty submit clears it). */
  filter: (value: string, submit: boolean) => void
  inspect: (seq: number) => void
  pause: () => void
  quiet: () => void
  newest: () => void
  rfilter: (value: string) => void
  pick: (which: 'strength' | 'lifecycle', value: string) => void
  expand: (key: string) => void
  /** Copies a command on the surface the press came from. */
  copy: (command: string, surface: string) => void
  condition: () => void
  explore: () => void
}

export type PaneData = {
  view: View
  rows: readonly Row[]
  ticks: readonly Tick[]
  health: Health | null
  feed: 'off' | 'following' | 'reconnecting' | 'stopped' | 'paused'
  reflexes: readonly Reflex[]
  waiting: readonly Waiting[]
  checks: readonly Check[]
  lapsed: readonly Lapse[]
  map: MapState
  /** The shim's reason when no binary could serve (ux-design §9.1). */
  nobinary: string
  /** No bridge on this surface (a Desktop without `$.process`, ux-design §4.12). */
  noBridge: boolean
  columns: number
  ascii: boolean
}

/** The most Live rows drawn at once; the pane keeps 500 and the filter reaches all of them. */
const DRAWN = 200

const rule = (els: Els, title: string, columns: number) => {
  const { Text } = els
  const head = `─ ${title} `
  return <Text dimColor>{head + '─'.repeat(Math.max(0, Math.min(columns, 120) - head.length))}</Text>
}

function tabs(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box, Button, Text } = els
  const tab = (key: View['tab'], hotkey: string, label: string) => (
    <Button key={`tab-${key}`} plain hotkey={hotkey} label={label} dimColor={d.view.tab !== key} onPress={() => a.tab(key)} />
  )
  return (
    <Box flexDirection="row" justifyContent="space-between">
      <Box flexDirection="row" gap={3}>
        {tab('live', 'l', 'Live')}
        {tab('reflexes', 'r', 'Reflexes')}
        {tab('health', 't', 'Health')}
      </Box>
      <Text dimColor>esc: close</Text>
    </Box>
  )
}

function explore(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Button, Link } = els
  if (d.map.url !== '') return <Link key="explore-link" href={d.map.url} label="Explore in browser" />
  return <Button key="explore" plain label={d.map.starting ? 'Explore in browser (starting…)' : 'Explore in browser'} onPress={() => a.explore()} />
}

function live(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box, Text, Button, Input } = els
  const rows = visibleRows(d.rows, d.view.filter, d.view.hideQuiet, d.ascii).slice(0, DRAWN)
  const sep = g('sep', d.ascii)
  const status =
    d.feed === 'reconnecting' ? `feed stopped ${sep} reconnecting${d.ascii ? '...' : '…'}`
    : d.feed === 'stopped' ? `feed stopped ${sep} run adage doctor`
    : ''
  return (
    <Box flexDirection="column">
      <Input key="filter" label="Filter:" value={d.view.filter} submitLabel="apply" onInput={v => a.filter(v, false)} onSubmit={v => a.filter(v, true)} />
      {status === '' ? null : <Text color={ROLES.prime}>{status}</Text>}
      {d.noBridge ? <Text dimColor>{`live rows need the terminal ${sep} adage watch`}</Text> : null}
      {!d.noBridge && d.rows.length === 0 ? <Text dimColor>Nothing yet in this repository. Reflexes appear here as Adage uses them.</Text> : null}
      {rows.map(r => {
        const [first = '', ...more] = rowLines(r, d.ascii)
        return (
          <Box key={`row-${r.seq}`} flexDirection="column">
            <Button key={`row:${r.seq}`} plain label={cut(first, d.columns, d.ascii)} dimColor={r.acts.length === 0} onPress={() => a.inspect(r.seq)} />
            {more.map((m, k) => (
              <Text key={`row-${r.seq}-${k}`} wrap="truncate-end">
                {m}
              </Text>
            ))}
          </Box>
        )
      })}
      {d.ticks.length === 0 ? null : rule(els, 'conditioning', d.columns)}
      {d.ticks.map((t, k) => (
        <Text key={`tick-${k}`} color={t.tone === 'error' ? ROLES.inhibit : ROLES.ink} wrap="truncate-end">
          {tickText(t, d.ascii)}
        </Text>
      ))}
      {d.health === null ? null : rule(els, 'health', d.columns)}
      {d.health === null ? null : (
        <Box flexDirection="row" flexWrap="wrap" columnGap={2}>
          {d.health.lights.map((l, k) => {
            const lt = lightText(l.lamp, l.label, d.ascii)
            return (
              <Box key={`light-${k}`} flexDirection="row">
                <Text color={ROLES[lt.role]}>{`${lt.glyph} `}</Text>
                <Text>{lt.text}</Text>
              </Box>
            )
          })}
        </Box>
      )}
      <Box flexDirection="row" gap={2}>
        <Button key="pause" plain hotkey="p" label={d.view.paused ? 'resume' : 'pause'} onPress={() => a.pause()} />
        <Button key="quiet" plain hotkey="v" label={d.view.hideQuiet ? 'all rows' : 'quiet rows'} onPress={() => a.quiet()} />
        <Button key="newest" plain hotkey="n" label="newest" onPress={() => a.newest()} />
      </Box>
    </Box>
  )
}

const STRENGTHS = ['all', 'fire', 'prime', 'inhibit'].map(v => ({ value: v }))
const LIFECYCLES = ['all', 'nascent', 'forming', 'firm', 'dormant', 'extinguished'].map(v => ({ value: v }))

function reflexRow(els: Els, d: PaneData, a: PaneActions, r: Reflex): Tree {
  const { Box, Text, Button } = els
  const key = `${r.reference}/${r.id}`
  const open = d.view.expanded === key
  const role = r.strength === 'fire' ? 'fire' : r.strength === 'inhibit' ? 'inhibit' : 'prime'
  return (
    <Box key={`reflex-${key}`} flexDirection="column">
      <Box flexDirection="row">
        <Text color={ROLES[role]}>{`${g('lamp', d.ascii)} `}</Text>
        <Button key={`reflex:${key}`} plain label={cut(reflexLine(r, d.ascii), Math.max(10, d.columns - 2), d.ascii)} onPress={() => a.expand(key)} />
      </Box>
      {!open ? null : (
        <Box flexDirection="column" paddingLeft={2}>
          {r.directive === '' ? null : <Text wrap="wrap">{r.directive}</Text>}
          {r.describe === '' ? null : <Text dimColor wrap="wrap">{r.describe}</Text>}
          {r.uplift === '' ? null : <Text dimColor wrap="wrap">{r.uplift}</Text>}
          {r.verbs.map(v => (
            <Box key={`verb-${key}-${v}`} flexDirection="row" gap={1}>
              <Text>{VERB_WORDS[v] ?? v}</Text>
              <Button key={`copy:${v}:${key}`} plain dimColor label={`Copy: ${reviewCommand(v, r)}`} onPress={e => a.copy(reviewCommand(v, r), e.surface)} />
            </Box>
          ))}
        </Box>
      )}
    </Box>
  )
}

function reflexes(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box, Text, Button, Input, Select } = els
  const shown = visibleReflexes(d.reflexes, d.view.rfilter, d.view.strength, d.view.lifecycle)
  const tracks = [...new Set(shown.map(r => r.reference))]
  return (
    <Box flexDirection="column">
      <Box flexDirection="row" gap={2}>
        <Input key="rfilter" label="Filter:" value={d.view.rfilter} submitLabel="apply" onInput={v => a.rfilter(v)} onSubmit={v => a.rfilter(v)} />
        <Select key="strength" label="strength" options={STRENGTHS} value={d.view.strength} onSelect={v => a.pick('strength', v)} />
        <Select key="lifecycle" label="lifecycle" options={LIFECYCLES} value={d.view.lifecycle} onSelect={v => a.pick('lifecycle', v)} />
      </Box>
      {d.waiting.length === 0 ? null : rule(els, 'waiting for you', d.columns)}
      {d.waiting.map(w => (
        <Box key={`wait-${w.key}`} flexDirection="row" gap={1}>
          <Text wrap="truncate-end">{w.text}</Text>
          <Button key={`copy:wait:${w.key}`} plain dimColor label="Copy command" onPress={e => a.copy(w.command, e.surface)} />
        </Box>
      ))}
      {d.reflexes.every(r => r.instinct) ? <Text dimColor>{reflexesEmpty(d.reflexes.filter(r => r.instinct).length, d.ascii)}</Text> : null}
      {tracks.map(t => {
        const inTrack = shown.filter(r => r.reference === t)
        const first = inTrack[0]
        return (
          <Box key={`track-${t}`} flexDirection="column">
            <Text dimColor>{first?.imported && first.signer !== '' ? `${first.track}  ${first.signer}` : (first?.track ?? t)}</Text>
            {inTrack.map(r => reflexRow(els, d, a, r))}
          </Box>
        )
      })}
    </Box>
  )
}

function health(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box, Text, Button } = els
  return (
    <Box flexDirection="column">
      {d.nobinary === '' ? null : <Text color={ROLES.prime} wrap="wrap">{d.nobinary}</Text>}
      {d.checks.map((c, k) => {
        const l = checkLine(c, d.ascii)
        const act = checkAction(c)
        return (
          <Box key={`check-${k}`} flexDirection="column">
            <Box flexDirection="row">
              <Text color={ROLES[l.role]}>{`${l.glyph} `}</Text>
              <Text wrap="truncate-end">{l.text}</Text>
            </Box>
            {act === undefined || c.status === 'pass' ? null
              : act.kind === 'run' ? <Button key={`run:${k}`} plain label="enter: condition this repository now" onPress={() => a.condition()} />
              : <Button key={`copy:check:${k}`} plain dimColor label="Copy command" onPress={e => a.copy(act.command, e.surface)} />}
          </Box>
        )
      })}
      {d.lapsed.length === 0 ? null : rule(els, 'grants that lapsed', d.columns)}
      {d.lapsed.map(l => (
        <Box key={`lapse-${l.route}`} flexDirection="row" gap={1}>
          <Text wrap="truncate-end">{lapseText(l)}</Text>
          <Button key={`copy:lapse:${l.route}`} plain dimColor label="Copy command" onPress={e => a.copy(l.reapprove, e.surface)} />
        </Box>
      ))}
    </Box>
  )
}

export function paneTree(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box } = els
  const body = d.view.tab === 'reflexes' ? reflexes(els, d, a) : d.view.tab === 'health' ? health(els, d, a) : live(els, d, a)
  return (
    <Box flexDirection="column">
      {tabs(els, d, a)}
      {body}
      {explore(els, d, a)}
    </Box>
  )
}

/** The inspect pane: the row's directives as the agent read them, and their evidence, as Markdown. */
export function inspectTree(els: Els, text: string): Tree {
  const { Markdown } = els
  return <Markdown key="inspect" text={text} />
}
