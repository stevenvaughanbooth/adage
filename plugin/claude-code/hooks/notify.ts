// What the person is told once per session, by id (ux-design §9): a status warning (the engine
// prefixes `⚠ adage:`) or a toast. The ids already said are kept in `$.state` `mode.warned`, which a
// new session (a /clear) empties; nothing is said to a session no person is at.
import type { Mod } from './mod'

/** A status line (warnings only), once per session for its id. */
export async function warnOnce(m: Mod, id: string, text: string): Promise<void> {
  if (!(await firstTime(m, id))) return
  m.ui.status(text)
}

/** A toast, once per session for its id (a lapse, a failed learn, a tamper). */
export async function toastOnce(m: Mod, id: string, text: string): Promise<void> {
  if (!(await firstTime(m, id))) return
  m.ui.toast(text)
}

/** Whether `id` is said now: a person is at the prompt and it was not said this session (it is marked said). */
async function firstTime(m: Mod, id: string): Promise<boolean> {
  if (!m.rt.interactive || (await m.state.mode.read()).warned.includes(id)) return false
  await m.state.mode.update(x => ({ ...x, warned: [...x.warned, id] }))
  return true
}
