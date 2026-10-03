// The active binary, asked (architecture §2 rule 1; ux-design §9.2): every hook event the mod
// sends goes through `askBinary`, which counts the binary's failed answers in a row and, at three,
// warns once that hooks are not answering. bridge.ts makes the run itself.
import type { Asked } from './bridge'
import { ask } from './bridge'
import type { Mod } from './mod'
import { warnOnce } from './notify'

/** The bridge's failures in a row before the status line says so (ux-design §9.2). */
export const UNANSWERING = 3

/** One question to the active binary, tagged; three failures in a row warn once. */
export async function askBinary(m: Mod, event: string, payload: { cwd?: string }, timeoutMs: number): Promise<Asked> {
  const asked = await ask(m.run, m.rt.bin, event, payload, timeoutMs)
  if (m.rt.bin === undefined) return asked
  m.rt.failures = asked.ok ? 0 : m.rt.failures + 1
  if (m.rt.failures >= UNANSWERING) await warnOnce(m, 'unanswering', 'hooks are not answering; run adage doctor').catch(() => undefined)
  return asked
}
