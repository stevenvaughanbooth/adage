// The `--json` reads the pane and the band draw from (architecture §2; ux-design §5, §6): the
// reflexes (`graph --json`), the checks (`doctor --json`), the lapsed grants (`delegate status
// --json`), the packs awaiting trust (`status --json`) and the opt-in lines and routine model (the
// status card). A read that fails keeps what was drawn; reads.ts reads what each printed.
import { read } from './bridge'
import type { Mod } from './mod'
import { toastOnce } from './notify'
import { checksOf, lapsedOf, optInOf, packsOf, reflexesOf, repoOf, routineModelOf, updateOf } from './reads'
import type { Offer } from './reads'
import { lapseToast } from './view/lines'

/** At most one refresh of the `--json` reads in this long, however many learn lines arrive. */
export const REFRESH_MS = 5000

/**
 * Reads again what the pane and the band draw from the binary, at most once per `REFRESH_MS`
 * unless forced; a session no person is at reads nothing. A lapsed grant is toasted once.
 */
export async function refresh(m: Mod, force = false): Promise<void> {
  if (m.rt.cli === undefined || !m.rt.interactive) return
  const now = await m.clock.now()
  if (!force && now - m.rt.lastRefresh < REFRESH_MS) return
  m.rt.lastRefresh = now
  const cwd = await m.session.cwd()
  const [graph, doctor, deleg, status, card] = await Promise.all([
    read(m.run, m.rt.cli, ['graph', '--json'], cwd),
    read(m.run, m.rt.cli, ['doctor', '--json'], cwd),
    read(m.run, m.rt.cli, ['delegate', 'status', '--json'], cwd),
    read(m.run, m.rt.cli, ['status', '--json'], cwd),
    read(m.run, m.rt.cli, ['--json', '--for-session'], cwd),
  ])
  // The graph holds every repository's reflexes; the pane, the band and the asks are about this one.
  if (card.ok) m.rt.repo = repoOf(card.json)
  if (graph.ok) await m.state.reflexes.update(() => reflexesOf(graph.json, m.rt.repo))
  if (doctor.ok) await m.state.checks.update(() => checksOf(doctor.json))
  if (status.ok) await m.state.packs.update(() => packsOf(status.json))
  if (card.ok) {
    // The opt-in lines, when the binary reports them (`ui.spinner`, `ui.turn_line`, `ui.noticed`);
    // off otherwise.
    const optIn = optInOf(card.json)
    await m.state.mode.update(x => ({ ...x, ...optIn }))
    m.rt.routineModel = routineModelOf(card.json)
    await noteUpdate(m, updateOf(card.json))
  }
  if (deleg.ok) {
    const lapsed = lapsedOf(deleg.json)
    await m.state.lapsed.update(() => lapsed)
    const ascii = (await m.state.mode.read()).ascii
    for (const l of lapsed) await toastOnce(m, `lapsed:${l.route}`, lapseToast(l, ascii))
  }
}

/**
 * A newer Adage, as the card says it (onboarding plan O9), kept for the band, Health's version row
 * and `/adage update`. The band's line shows it once per release per machine (`$.store` `latest`,
 * written when a session first shows it) and only before the session's first prompt: a session
 * that showed it keeps it until then, however often the card is read again.
 */
async function noteUpdate(m: Mod, u: Offer | undefined): Promise<void> {
  const was = (await m.state.mode.read()).update
  if (u === undefined) {
    if (was !== null) await m.state.mode.update(x => ({ ...x, update: null }))
    return
  }
  const first = (await m.state.turn.read()).submits === 0
  let band = first && was?.latest === u.latest && was.band
  if (was?.latest !== u.latest && first) {
    const seen = await m.store.latest.get().catch(() => undefined)
    if (seen !== u.latest) {
      band = true
      await m.store.latest.set(u.latest).catch(() => undefined)
    }
  }
  if (was === null || was.latest !== u.latest || was.band !== band || was.next !== u.next || was.have !== u.have || was.notes.join('\n') !== u.notes.join('\n')) {
    await m.state.mode.update(x => ({ ...x, update: { ...u, band } }))
  }
}

/**
 * What a session that draws nothing still needs from the status card: the routine model. A session
 * that draws reads the card with the rest (`refresh`).
 */
export async function settings(m: Mod): Promise<void> {
  if (m.rt.cli === undefined) return
  const card = await read(m.run, m.rt.cli, ['--json', '--for-session'], await m.session.cwd())
  if (card.ok) m.rt.routineModel = routineModelOf(card.json)
}
