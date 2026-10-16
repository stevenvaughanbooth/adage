// What outlives the session in `$.store` (ux-design §4.17, §9.12): the asks (one per reflex per day,
// so an ask does not return across sessions) and the answers kept for the next session start in
// the same repository (no press was held when they were given). `$.store` is a file the agent's
// tools can write: a kept answer is never pressed as it stands (dialog.ts asks again).
import { askedOf, keptOf, record } from './ask'
import type { Mod } from './mod'
import type { Kept } from './plugin'

/** The most answers kept across sessions. */
const MAX_KEPT = 50

/**
 * The repository the session works in (its root; the working directory outside one): a kept
 * answer is offered only where it was given, since `adage on` and `add <dir> --trust` act on the
 * working directory's repository.
 */
export async function repoRoot(m: Mod): Promise<string> {
  const repo = await m.session.repo().catch(() => null)
  return repo?.root ?? (await m.session.cwd())
}

/** The kept answers as the store holds them now (none when it holds anything else). */
export async function keptNow(m: Mod): Promise<Kept[]> {
  return keptOf(await m.store.kept.get().catch(() => undefined))
}

/** What `$.store` keeps across sessions, read at a start: the asks, and the count of answers kept here. */
export async function loadStore(m: Mod): Promise<void> {
  const asked = askedOf(await m.store.asked.get().catch(() => undefined))
  if (asked.length > 0) await m.state.asked.update(cur => (cur.length === 0 ? asked : cur))
  const root = await repoRoot(m)
  m.rt.kept = (await keptNow(m)).filter(k => k.root === root).length
}

/** Keeps an answer for the next session start in this repository (ux-design §9.12); the band says so. */
export async function keep(m: Mod, k: Kept): Promise<void> {
  const kept = (await keptNow(m)).filter(x => !(x.command === k.command && x.root === k.root))
  await m.store.kept.set([...kept, k].slice(-MAX_KEPT)).catch(() => undefined)
  m.rt.kept = kept.filter(x => x.root === k.root).length + 1
}

/** Records a decision on a waiting item, so the ask does not return (`$.state`, and the store across sessions). */
export async function noteAnswer(m: Mod, key: string, answer: string | undefined): Promise<void> {
  const now = await m.clock.now()
  const asked = await m.state.asked.update(cur => record(cur, key, answer, now))
  await m.store.asked.set(asked).catch(() => undefined)
}
