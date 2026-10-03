// What the person's presses in the pane do, and opening it (docs/claude-mod/ux-design.md §6): the
// tabs, the filters, the inspect pane, the pause, copying a command, the Health tab's one action
// (`adage condition`) and `Explore in browser` (`adage map`, started once). None of these is a
// decision: a decision is a press (dialog.ts, token.ts), which draw/sites.ts adds to these.
import { say } from './bridge'
import type { PaneActions } from './draw/pane'
import { togglePause } from './follower'
import type { AnswerAt, EventAt, Mod, NextAt } from './mod'
import { quietly } from './mod'
import type { View } from './plugin'
import { INSPECT, MAP0, PANE } from './plugin'
import { mapUrl } from './reads'
import { refresh } from './refresh'
import { visibleRows } from './view/pane'

/** Opens the pane on a tab, focused: the person asked (`/adage …`, ux-design §6). */
export async function openPane(m: Mod, tab: View['tab']): Promise<void> {
  await m.state.view.update(v => ({ ...v, tab }))
  await m.ui.open({ id: PANE, title: 'adage', focus: true, closeOnEscape: true, holdToasts: true, rows: 20 })
  if (tab !== 'live') quietly(refresh(m, true))
}

/** `why`: the pane, focused, on that reflex (or pack), with `e enable` and `n not this one`. */
export async function showReflex(m: Mod, key: string): Promise<void> {
  await m.state.view.update(v => ({ ...v, tab: 'reflexes' as const, expanded: key, rfilter: '', strength: 'all', lifecycle: 'all' }))
  await openPane(m, 'reflexes')
}

/** Copies a command for the person and says so (`toast.copied`, `toast.copy.unavailable`). */
export async function copy(m: Mod, command: string, on?: string): Promise<boolean> {
  const target = on === 'terminal' || on === 'desktop' || on === 'vscode' || on === 'mobile' ? on : undefined
  const done = await m.ui.copy(target === undefined ? { text: command } : { text: command, surface: target }).catch(() => ({ isCopied: false as const }))
  m.ui.toast(done.isCopied ? `copied: ${command}` : 'copy is not available here · the command is in /adage health')
  return done.isCopied
}

/** `Explore in browser` (ux-design §6): starts `adage map` once (it opens the browser) and keeps its address. */
export async function explore(m: Mod): Promise<string | undefined> {
  const now = await m.state.map.read()
  if (now.url !== '') return now.url
  if (m.rt.cli === undefined || now.starting) return undefined
  await m.state.map.update(x => ({ ...x, starting: true }))
  const stream = m.spawn({ argv: [m.rt.cli, 'map', '--port', '0'], cwd: await m.session.cwd() })
  return new Promise<string | undefined>(resolve => {
    let said = ''
    let found = false
    quietly((async () => {
      try {
        for await (const chunk of stream) {
          said = (said + chunk.text).slice(-4096)
          const url = found ? undefined : mapUrl(said)
          if (url !== undefined) {
            found = true
            await m.state.map.update(() => ({ url, starting: false }))
            resolve(url)
          }
        }
      } catch {
        // It could not start, or it ended: the link goes, and a press starts it again.
      }
      if (!found) resolve(undefined)
      await m.state.map.update(() => MAP0)
    })())
  })
}

/** The pane's handlers (draw/pane.tsx calls them from presses, picks and fields); the presses are added by draw/sites.ts. */
export function paneActions(m: Mod): Omit<PaneActions, 'press'> {
  return {
    tab: tab => {
      quietly(m.state.view.update(v => ({ ...v, tab })))
      quietly(m.store.tab.set(tab))
    },
    filter: (value, submit) => quietly(m.state.view.update(v => ({ ...v, filter: submit && value.trim() === '' ? '' : value }))),
    inspect: seq => {
      quietly((async () => {
        const view = await m.state.view.read()
        const rows = await m.state.rows.read()
        const ascii = (await m.state.mode.read()).ascii
        // A row the filter removed is never inspected (ux-design §6.1).
        if (!visibleRows(rows, view.filter, view.hideQuiet, ascii).some(r => r.seq === seq)) return
        await m.state.view.update(v => ({ ...v, selected: seq }))
        await m.ui.open({ id: INSPECT, title: 'adage · inspect', focus: true, closeOnEscape: true, rows: 12 })
      })())
    },
    pause: () => quietly(togglePause(m)),
    quiet: () => quietly(m.state.view.update(v => ({ ...v, hideQuiet: !v.hideQuiet }))),
    newest: () => quietly(m.ui.scroll({ in: PANE, to: 'start' })),
    rfilter: value => quietly(m.state.view.update(v => ({ ...v, rfilter: value }))),
    pick: (which, value) => quietly(m.state.view.update(v => (which === 'strength' ? { ...v, strength: value } : { ...v, lifecycle: value }))),
    expand: key => quietly(m.state.view.update(v => ({ ...v, expanded: v.expanded === key ? '' : key }))),
    copy: (command, on) => quietly(copy(m, command, on)),
    condition: () => {
      quietly((async () => {
        await say(m.run, m.rt.cli, ['condition'], await m.session.cwd())
        await refresh(m, true)
      })())
    },
    explore: () => quietly(explore(m)),
  }
}

/** Whether the pane is open: the band's `1 pattern noticed` cell shows only while it is closed. */
export async function onPaneOpen(m: Mod, e: EventAt<'ui.open', { id: 'adage' }>, next: NextAt<'ui.open', { id: 'adage' }>): AnswerAt<'ui.open', { id: 'adage' }> {
  m.rt.paneOpen = true
  return next(e)
}

export async function onPaneClose(m: Mod, e: EventAt<'ui.close', { id: 'adage' }>, next: NextAt<'ui.close', { id: 'adage' }>): AnswerAt<'ui.close', { id: 'adage' }> {
  m.rt.paneOpen = false
  return next(e)
}
