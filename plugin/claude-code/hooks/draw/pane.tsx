// The pane (ux-design §6): four tabs as plain Buttons with lowercase hotkeys (`g` Graph, the first
// and the default, `l` Live, `r` Reflexes, `t` Health; `h` is `adage watch`'s help key), every tab
// ending with `Explore in browser`. Strength and lifecycle words are allowed here and nowhere else (§1.3). A decision in
// the pane is a press (§4.17): a Button's onPress reaches `press()` with the session's token, and
// only through the handlers the actions carry (`press`, dialog.ts); until a person is at the prompt
// they are not bound and, as before a press existed, a review verb is a command to copy. The one
// action besides is `adage condition`, which the binary marks as allowed from a session.
import type { Check, Fired, GraphNode, Health, Lapse, MapState, Mark, Predicted, Reflex, Row, Tick, Update, View } from '../plugin'
import type { Offer } from '../reads'
import { TRUST_LINE } from '../grant'
import { ROLES } from '../tokens'
import type { Pulse, Seg, Waiting } from '../view/pane'
import {
  ROW_VERBS,
  TICKS_DRAWN,
  UPDATE_COMMAND,
  VERB_HOTKEYS,
  VERB_WORDS,
  checkAction,
  actFlows,
  checkLine,
  flowBoxes,
  flowLine,
  lapseText,
  lightText,
  pulseLine,
  pulses,
  GROUP_TITLES,
  GROUPS,
  WAITING_TITLE,
  reflexFacts,
  reflexesEmpty,
  reviewCommand,
  rowLines,
  tickText,
  trackTitle,
  updateRowText,
  visibleReflexes,
  visibleRows,
} from '../view/pane'
import { graphLines } from '../view/graph'
import { cut, g } from '../view/text'
import type { Els, Tree } from './els'
import { graphTree } from './graph'
import { rule } from './rows'

/** What a press in the pane or a command does (dialog.ts `pressHandlers`): each reaches the press. */
export type PressHandlers = {
  /** A review verb on a reflex row (`enable`, `not this one`, and the four more). */
  verb: (verb: string, reflex: Reflex) => void
  /** A waiting item's first answer in the pane (`enable`, `see what it would do`, `allow`, `use them`). */
  yes: (item: Waiting) => void
  /** A waiting item's `not this one` / `not now`. */
  no: (item: Waiting) => void
  /** `/adage on`: opens the ask (never presses itself). */
  askOn: () => void
  /** Health's `u: update` on the version row: `adage update` through the press (onboarding plan O9). */
  update: () => void
  /** `/adage update`: opens the ask (never presses itself). */
  askUpdate: (u: Offer) => void
}

/** What a press, a pick or a keystroke in the pane does (actions.ts, and the presses from dialog.ts). */
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
  /** The Graph tab's `a`: the whole session, or follow the newest again. */
  history: () => void
  /** The presses, once a person is at the prompt; undefined until then (a verb is a command to copy). */
  press: PressHandlers | undefined
}

export type PaneData = {
  view: View
  rows: readonly Row[]
  ticks: readonly Tick[]
  health: Health | null
  feed: 'off' | 'following' | 'reconnecting' | 'stopped' | 'paused'
  reflexes: readonly Reflex[]
  /** The reflexes the command hooks delivered in this session: with the rows, what the flow lights. */
  fired: readonly Fired[]
  waiting: readonly Waiting[]
  checks: readonly Check[]
  lapsed: readonly Lapse[]
  /** A newer Adage, as the status card says it: Health's version row. */
  update: Update | null
  map: MapState
  /** The shim's reason when no binary could serve (ux-design §9.1). */
  nobinary: string
  /** No bridge on this surface (a Desktop without `$.process`, ux-design §4.12). */
  noBridge: boolean
  columns: number
  ascii: boolean
  /**
   * The Reflexes flow as boxes when wide enough (the terminal); the one-line flow always (the
   * desktop, whose native Button inside a box line would break its borders).
   */
  boxes: boolean
  /** This session's graph (the Graph tab), and what the rest of the state says about its nodes. */
  dag: readonly GraphNode[]
  predicted: readonly Predicted[]
  marks: readonly Mark[]
  /** The clock now, and whether the newest node may glow (not under reduced motion). */
  now: number
  motion: boolean
  /** The rows the pane's body has room for (`scroll.bodyRows`): the Graph follows its newest within them. */
  bodyRows: number
}

/** The rows besides the graph's own lines: the tabs, its summary, its footer, `Explore in browser`. */
const GRAPH_CHROME = 4

function graph(els: Els, d: PaneData, a: PaneActions): Tree {
  const extra = (d.nobinary === '' ? 0 : 1) + (d.noBridge ? 1 : 0)
  const height = d.view.history ? 0 : Math.max(6, d.bodyRows - GRAPH_CHROME - extra)
  const drawn = graphLines(
    { nodes: d.dag, predicted: d.predicted, marks: d.marks, rows: d.rows, reflexes: d.reflexes, now: d.now, motion: d.motion },
    { columns: d.columns, ascii: d.ascii, boxes: d.boxes, height },
  )
  return graphTree(els, { drawn, nobinary: d.nobinary, noBridge: d.noBridge, history: d.view.history, onHistory: () => a.history(), ascii: d.ascii })
}

/**
 * `pane.live.quiet`: what the hidden quiet rows are, and how to show them. With none shown it says
 * so plainly, so an empty tab never reads as a broken one.
 */
function quietLine(all: readonly Row[], shown: number, ascii: boolean): string {
  const hidden = all.filter(r => r.acts.length === 0).length
  const sep = g('sep', ascii)
  if (hidden === 0) return ''
  const what = `${hidden} quiet ${hidden === 1 ? 'event' : 'events'} hidden ${sep} v: show`
  return shown === 0 ? `Adage has had nothing to say yet ${sep} ${what}` : what
}

/** The most Live rows drawn at once; the pane keeps 500 and the filter reaches all of them. */
const DRAWN = 200

/**
 * The expanded row's details (`d: details`) are kept in the one expanded key with this suffix, so
 * the view needs no second field: `key` is the row open, `key!` the row open with its details.
 */
const DETAILS = '!'

function tabs(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box, Button, Text } = els
  const tab = (key: View['tab'], hotkey: string, label: string) => (
    <Button key={`tab-${key}`} plain hotkey={hotkey} label={label} dimColor={d.view.tab !== key} onPress={() => a.tab(key)} />
  )
  return (
    <Box flexDirection="row" justifyContent="space-between">
      <Box flexDirection="row" gap={3}>
        {tab('graph', 'g', 'Graph')}
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
      <Input key="filter" label="Filter" value={d.view.filter} submitLabel="apply" onInput={v => a.filter(v, false)} onSubmit={v => a.filter(v, true)} />
      {status === '' ? null : <Text color={ROLES.prime}>{status}</Text>}
      {d.noBridge ? <Text dimColor>{`live rows need the terminal ${sep} adage watch`}</Text> : null}
      {!d.noBridge && d.rows.length === 0 ? <Text dimColor>Nothing yet in this repository. Reflexes appear here as Adage uses them.</Text> : null}
      {!d.view.hideQuiet || quietLine(d.rows, rows.length, d.ascii) === '' ? null : <Text dimColor>{quietLine(d.rows, rows.length, d.ascii)}</Text>}
      {rows.map(r => {
        const [first = '', ...more] = rowLines(r, d.ascii)
        // Each directive's flow in small beneath its act line (ux-design §6.1).
        const flows = actFlows(r, d.reflexes, d.columns, d.ascii)
        const flow = (k: number) => {
          const f = flows[k]
          return f === undefined ? null : flowRow(els, f, `flow-row-${r.seq}-${k}`)
        }
        return (
          <Box key={`row-${r.seq}`} flexDirection="column">
            <Button key={`row:${r.seq}`} plain label={cut(first, d.columns, d.ascii)} dimColor={r.acts.length === 0} onPress={() => a.inspect(r.seq)} />
            {flow(0)}
            {more.flatMap((m, k) => [
              <Text key={`row-${r.seq}-${k}`} wrap="truncate-end">
                {m}
              </Text>,
              flow(k + 1),
            ])}
          </Box>
        )
      })}
      {d.ticks.length === 0 ? null : rule(els, 'conditioning', d.columns)}
      {d.ticks.slice(-TICKS_DRAWN).map((t, k) => (
        <Text key={`tick-${k}`} color={t.tone === 'error' ? ROLES.inhibit : ROLES.ink} wrap="truncate-end">
          {tickText(t, d.ascii, d.reflexes)}
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
        <Button key="quiet" plain hotkey="v" label={d.view.hideQuiet ? 'show quiet rows' : 'hide quiet rows'} onPress={() => a.quiet()} />
        <Button key="newest" plain hotkey="n" label="newest" onPress={() => a.newest()} />
      </Box>
    </Box>
  )
}

const STRENGTHS = ['all', 'fire', 'prime', 'inhibit'].map(v => ({ value: v }))
const LIFECYCLES = ['all', 'nascent', 'forming', 'firm', 'dormant', 'extinguished'].map(v => ({ value: v }))

/** A review verb's Button: a press when the handlers are bound, else the command to copy (B3). */
function verbButton(els: Els, a: PaneActions, r: Reflex, v: string, key: string, hotkey: string | undefined): Tree {
  const { Button } = els
  const press = a.press
  if (press === undefined) {
    return <Button key={`copy:${v}:${key}`} plain dimColor label={`Copy: ${reviewCommand(v, r)}`} onPress={e => a.copy(reviewCommand(v, r), e.surface)} />
  }
  return <Button key={`verb:${v}:${key}`} plain hotkey={hotkey} label={VERB_WORDS[v] ?? v} onPress={() => press.verb(v, r)} />
}

/**
 * One drawn line of a flow (view/pane.ts `flowBoxes`, `flowLine`): each piece a Text in its role,
 * and the one piece marked `press` the row's Button, which expands the row.
 */
function flowRow(els: Els, line: readonly Seg[], key: string, button?: { key: string; onPress: () => void }): Tree {
  const { Box, Text, Button } = els
  return (
    <Box key={key} flexDirection="row">
      {line.map((s, k) =>
        s.press && button !== undefined ? (
          <Button key={button.key} plain label={s.text} onPress={button.onPress} />
        ) : (
          <Text key={`${key}-${k}`} {...(s.role === undefined ? {} : { color: ROLES[s.role] })} {...(s.dim ? { dimColor: true } : {})} {...(s.bold ? { bold: true } : {})}>
            {s.text}
          </Text>
        ),
      )}
    </Box>
  )
}

function reflexRow(els: Els, d: PaneData, a: PaneActions, r: Reflex, pulse: Pulse | undefined): Tree {
  const { Box, Text, Button } = els
  const key = `${r.reference}/${r.id}`
  const open = d.view.expanded === key || d.view.expanded === `${key}${DETAILS}`
  const details = d.view.expanded === `${key}${DETAILS}`
  const lines = (d.boxes ? flowBoxes(r, pulse, d.columns, d.ascii, open) : undefined) ?? [flowLine(r, pulse, d.columns, d.ascii)]
  const button = { key: `reflex:${key}`, onPress: () => a.expand(details ? `${key}${DETAILS}` : key) }
  const onRow = r.verbs.filter(v => ROW_VERBS.includes(v))
  const more = r.verbs.filter(v => !ROW_VERBS.includes(v))
  const command = r.verbs[0] === undefined ? '' : reviewCommand(r.verbs[0], r)
  return (
    <Box key={`reflex-${key}`} flexDirection="column">
      {lines.map((l, k) => flowRow(els, l, `flow-${key}-${k}`, button))}
      {!open ? null : (
        <Box flexDirection="column" paddingLeft={2} marginBottom={1}>
          {r.directive === '' ? null : <Text wrap="wrap">{r.directive}</Text>}
          <Text dimColor wrap="wrap">{reflexFacts(r, d.ascii)}</Text>
          {r.describe === '' ? null : <Text dimColor wrap="wrap">{r.describe}</Text>}
          {r.uplift === '' ? null : <Text dimColor wrap="wrap">{r.uplift}</Text>}
          <Box flexDirection="row" gap={2}>
            {onRow.map(v => verbButton(els, a, r, v, key, VERB_HOTKEYS[v]))}
            {more.length === 0 && command === '' ? null : <Button key={`details:${key}`} plain hotkey="d" label="details" onPress={() => a.expand(details ? key : `${key}${DETAILS}`)} />}
          </Box>
          {!details ? null : (
            <Box flexDirection="column">
              {more.length === 0 ? null : (
                <Box flexDirection="row" gap={2}>
                  <Text dimColor>more:</Text>
                  {more.map(v => verbButton(els, a, r, v, key, VERB_HOTKEYS[v]))}
                </Box>
              )}
              {a.press === undefined || command === '' ? null : (
                <Box flexDirection="row" gap={1}>
                  <Text dimColor wrap="truncate-end">{command}</Text>
                  <Button key={`copy:${r.verbs[0]}:${key}`} plain dimColor hotkey="c" label="Copy" onPress={e => a.copy(command, e.surface)} />
                </Box>
              )}
            </Box>
          )}
        </Box>
      )}
    </Box>
  )
}

/** The first answer's words for a waiting item in the pane (the dialog's own, ux-design §4.17). */
function yesWord(w: Waiting): string {
  switch (w.kind) {
    case 'enable':
      return 'enable'
    case 'allow':
      return w.offer === 'predict' ? 'allow' : 'see what it would do'
    case 'trust':
      return 'use them'
  }
}

/**
 * One suggestion: the reflex's sentence, whole and wrapped (its group's heading says what the
 * answer changes), then its two answers beneath, so a narrow pane never cuts the sentence to fit
 * the buttons; `command` beside them shows the terminal command, dim with Copy.
 */
function waitingRow(els: Els, d: PaneData, a: PaneActions, w: Waiting): Tree {
  const { Box, Text, Button } = els
  const press = a.press
  // Expanded under its own key: an enable item's key is its reflex's, whose row expands apart.
  const open = d.view.expanded === `wait:${w.key}`
  return (
    <Box key={`wait-${w.key}`} flexDirection="column" paddingLeft={2}>
      <Text wrap="wrap">{w.text}</Text>
      <Box flexDirection="row" gap={3} paddingLeft={2}>
        {press === undefined ? (
          <Button key={`copy:wait:${w.key}`} plain dimColor label="Copy command" onPress={e => a.copy(w.command, e.surface)} />
        ) : (
          <Box flexDirection="row" gap={3}>
            <Button key={`yes:${w.key}`} plain label={yesWord(w)} onPress={() => press.yes(w)} />
            <Button key={`no:${w.key}`} plain dimColor label={w.kind === 'enable' ? 'not this one' : 'not now'} onPress={() => press.no(w)} />
          </Box>
        )}
        <Button key={`wait:${w.key}`} plain dimColor label={open ? 'hide command' : 'command'} onPress={() => a.expand(`wait:${w.key}`)} />
      </Box>
      {!open ? null : (
        <Box flexDirection="column" paddingLeft={2}>
          {w.kind === 'trust' ? <Text dimColor>{`packs ${w.say}`}</Text> : null}
          {w.kind === 'trust' ? <Text dimColor>{TRUST_LINE}</Text> : null}
          <Box flexDirection="row" gap={1}>
            <Text dimColor wrap="truncate-end">{w.command}</Text>
            <Button key={`copy:wait:${w.key}`} plain dimColor label="Copy" onPress={e => a.copy(w.command, e.surface)} />
          </Box>
        </Box>
      )}
    </Box>
  )
}

/** The suggestions (`pane.waiting`): the rule, then each group under its plain heading. */
function suggestions(els: Els, d: PaneData, a: PaneActions): Tree[] {
  const { Box, Text } = els
  if (d.waiting.length === 0) return []
  const out: Tree[] = [rule(els, WAITING_TITLE, d.columns)]
  for (const gr of GROUPS) {
    const items = d.waiting.filter(w => w.group === gr)
    if (items.length === 0) continue
    out.push(
      <Box key={`group-${gr}`} flexDirection="column">
        <Text dimColor wrap="wrap">{GROUP_TITLES[gr]}</Text>
        {items.map(w => waitingRow(els, d, a, w))}
      </Box>,
    )
  }
  return out
}

function reflexes(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box, Text, Input, Select } = els
  const shown = visibleReflexes(d.reflexes, d.view.rfilter, d.view.strength, d.view.lifecycle)
  // What this repository learned first, a team's next, Adage's built-in instincts last.
  const rank = (t: string) => (shown.find(r => r.reference === t)?.instinct ? 2 : t.endsWith('/learned') ? 0 : 1)
  const tracks = [...new Set(shown.map(r => r.reference))].sort((a, b) => rank(a) - rank(b))
  const lit = pulses(d.fired, d.rows)
  const live = pulseLine(lit, d.ascii)
  return (
    <Box flexDirection="column">
      <Box flexDirection="row" gap={2}>
        <Input key="rfilter" label="Filter" value={d.view.rfilter} submitLabel="apply" onInput={v => a.rfilter(v)} onSubmit={v => a.rfilter(v)} />
        <Select key="strength" label="strength" options={STRENGTHS} value={d.view.strength} onSelect={v => a.pick('strength', v)} />
        <Select key="lifecycle" label="lifecycle" options={LIFECYCLES} value={d.view.lifecycle} onSelect={v => a.pick('lifecycle', v)} />
      </Box>
      {suggestions(els, d, a)}
      {d.waiting.length === 0 ? null : rule(els, 'reflexes', d.columns)}
      {live === '' ? null : <Text color={ROLES.fire} wrap="truncate-end">{live}</Text>}
      {d.reflexes.every(r => r.instinct) ? <Text dimColor>{reflexesEmpty(d.reflexes.filter(r => r.instinct).length, d.ascii)}</Text> : null}
      {tracks.map(t => {
        const inTrack = shown.filter(r => r.reference === t)
        const first = inTrack[0]
        return (
          <Box key={`track-${t}`} flexDirection="column">
            <Text dimColor>{first === undefined ? t : trackTitle(first)}</Text>
            {inTrack.map(r => reflexRow(els, d, a, r, lit.get(r.id)))}
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
      {d.update === null ? null : versionRow(els, d.update, d.ascii, a)}
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

/**
 * Health's `version` row (onboarding plan O9): a newer Adage, in plain ink (information, never a
 * problem), the release's first lines, `u: update` (the press, once a person is at the prompt) and
 * the terminal command to copy.
 */
function versionRow(els: Els, u: Update, ascii: boolean, a: PaneActions): Tree {
  const { Box, Text, Button } = els
  return (
    <Box key="version" flexDirection="column">
      <Text wrap="truncate-end">{updateRowText(u, ascii)}</Text>
      {u.notes.map((n, k) => (
        <Text key={`note-${k}`} dimColor wrap="truncate-end">{`  ${n}`}</Text>
      ))}
      <Box flexDirection="row" gap={1}>
        {a.press === undefined ? null : <Button key="update" plain hotkey="u" label="update" onPress={() => a.press?.update()} />}
        <Button key="copy:update" plain dimColor label={`Copy: ${UPDATE_COMMAND}`} onPress={e => a.copy(UPDATE_COMMAND, e.surface)} />
      </Box>
    </Box>
  )
}

export function paneTree(els: Els, d: PaneData, a: PaneActions): Tree {
  const { Box } = els
  const body = d.view.tab === 'graph' ? graph(els, d, a) : d.view.tab === 'reflexes' ? reflexes(els, d, a) : d.view.tab === 'health' ? health(els, d, a) : live(els, d, a)
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
