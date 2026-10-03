// The execution grant as the person sees it (plan B4; ux-design §4.17 `ask.allow.pane`,
// `ask.allow.first`, `ask.allow.details`; architecture §3.12 "Stage, then confirm"): what
// `review delegate … --level e2 --stage --json` answered, read into the three plain lines, the
// diff and the details, and what `--confirm` answered. Pure: nothing here runs or records.
import type { Allow, Stage } from './plugin'
import { isRecord, records, str, strings } from './shape'
import { g } from './view/text'

/** The added and removed lines of a unified diff (`+4 −0`). */
export function diffCounts(text: string): { plus: number; minus: number } {
  let plus = 0
  let minus = 0
  for (const line of text.split('\n')) {
    if (line.startsWith('+++') || line.startsWith('---')) continue
    if (line.startsWith('+')) plus++
    else if (line.startsWith('-')) minus++
  }
  return { plus, minus }
}

/**
 * The stage as the grant pane draws it, from the object `--stage --json` printed (or `--confirm`
 * printed again with `changed: "snapshot"`); undefined when it is not one.
 */
export function stageOf(v: unknown): Stage | undefined {
  if (!isRecord(v) || typeof v.digest !== 'string' || typeof v.ok !== 'boolean') return undefined
  const facts = isRecord(v.facts) ? v.facts : {}
  const terms = isRecord(v.terms) ? v.terms : {}
  const staged = isRecord(facts.staged) ? facts.staged : {}
  const action = isRecord(facts.action) ? facts.action : {}
  const effect = isRecord(facts.effect) ? facts.effect : {}
  const python = isRecord(facts.python) ? facts.python : undefined
  const scripts = records(facts.scripts).map(s => ({ path: str(s.path), sha256: str(s.sha256), text: str(s.text) }))
  const diff = records(staged.diff).map(d => {
    const text = str(d.diff)
    return { path: str(d.path), change: str(d.change), ...diffCounts(text), text }
  })
  const refused = records(staged.refused).map(r => ({ path: str(r.path), why: str(r.why) }))
  const inputs = records(facts.inputs).map(i => str(i.contract) || str(i.slot)).filter(s => s !== '')
  const hashes = [
    ...(str(facts.profile_id) === '' ? [] : [`profile ${str(facts.profile_id)}`]),
    ...(str(facts.route_exec_digest) === '' ? [] : [`route ${str(facts.route_exec_digest)}`]),
    ...(python === undefined ? [] : [`runtime ${str(python.runtime_sha256)}`, `environment ${str(python.environment_sha256)}`].filter(h => !h.endsWith(' '))),
    ...scripts.map(s => `${s.path} ${s.sha256}`),
  ]
  return {
    ok: v.ok,
    why: str(v.why),
    digest: v.digest,
    first: v.first === true,
    argv: strings(facts.argv),
    diff,
    refused,
    scripts,
    sandbox: str(facts.sandbox),
    profile: str(facts.profile_id),
    hashes,
    effect: str(effect.approved) || str(effect.classified),
    cache: strings(action.cache).length > 0 ? strings(action.cache) : strings(terms.cache),
    outputs: strings(action.outputs).length > 0 ? strings(action.outputs) : strings(terms.outputs),
    inputs,
  }
}

/** What `--confirm` came to: recorded, or the fresh stage to decide on again (the repository moved). */
export type Confirmed = { recorded: true; wrote: string[]; takesEffect: string } | { recorded: false; stage: Stage | undefined }

export function confirmOf(v: unknown): Confirmed | undefined {
  if (!isRecord(v)) return undefined
  if (v.recorded === true) return { recorded: true, wrote: strings(v.wrote), takesEffect: str(v.takes_effect) }
  return { recorded: false, stage: stageOf(v) }
}

/** The `--confirm` command of a stage's terminal command (the binary's own `confirm` beside `offers`). */
export function confirmCommand(command: string): string {
  return command.replace(/\s--stage\s+--json\s*$/, ' --confirm')
}

/** The track and route a `review delegate <track> <route> …` command names. */
export function routeOf(command: string): { track: string; route: string } | undefined {
  const m = /^adage review delegate (\S+) (\S+)/.exec(command.trim())
  return m?.[1] !== undefined && m[2] !== undefined ? { track: m[1], route: m[2] } : undefined
}

/** `ask.allow.first`, once per machine (the binary says when the grant is the first). */
export const FIRST_LINE = 'this is the first time Adage would run something on this machine; it only ever runs steps you allow one by one'

/** The sentence a team-pack `why` ends with (ux-design §4.17). */
export const TRUST_LINE = 'this is the same trust you give CLAUDE.md'

/**
 * The three plain lines of the grant pane (`ask.allow.pane`): what it runs, what it changes (or
 * reads), what it never touches; the diff follows as `Code`. Before the stage lands, the first line
 * alone.
 */
export function allowLines(a: Allow, ascii: boolean): string[] {
  const s = a.stage
  if (s === null) return [a.say, a.busy ? (ascii ? 'staging it...' : 'staging it…') : '']
  const sep = g('sep', ascii)
  const lines = [a.say, `it runs    ${s.argv.join(' ')}`]
  if (s.diff.length > 0) {
    const plus = s.diff.reduce((n, d) => n + d.plus, 0)
    const minus = s.diff.reduce((n, d) => n + d.minus, 0)
    lines.push(`it changes ${s.diff.map(d => d.path).join(', ')}  (+${plus} ${ascii ? '-' : '−'}${minus}, shown below)`)
  } else if (s.outputs.length > 0) {
    lines.push(`it changes ${s.outputs.join(', ')}  (nothing differs now)`)
  } else if (s.inputs.length > 0) {
    lines.push(`it reads   ${s.inputs.join(', ')}`)
  }
  lines.push('it never   touches anything else, or the network')
  if (s.first) lines.push(FIRST_LINE)
  if (s.refused.length > 0) lines.push(`it would also touch ${s.refused.map(r => r.path).join(', ')}, which the grant would not cover ${sep} not allowed as it is`)
  return lines.filter(l => l !== '')
}

/** The unified diff of every staged output, one after another, for one `Code` element. */
export function diffText(a: Allow): string {
  return (a.stage?.diff ?? []).map(d => d.text).filter(t => t.trim() !== '').join('\n').slice(0, 10_000)
}

/** `ask.allow.details`: what the terminal shows a person today; nothing here is needed to decide. */
export function allowDetails(a: Allow, ascii: boolean): string[] {
  const s = a.stage
  if (s === null) return []
  const sep = ` ${g('sep', ascii)} `
  const out = [`argv      ${s.argv.join(' ')}`]
  for (const sc of s.scripts) out.push(`script    ${sc.path}`, ...sc.text.split('\n').map(l => `          ${l}`))
  if (s.sandbox !== '') out.push(`sandbox   ${s.sandbox}`)
  if (s.effect !== '') out.push(`effect    ${s.effect}`)
  if (s.outputs.length > 0) out.push(`outputs   ${s.outputs.join(sep)}`)
  if (s.cache.length > 0) out.push(`caches    ${s.cache.join(sep)}`)
  for (const h of s.hashes) out.push(`hash      ${h}`)
  out.push('takes effect in your next session')
  return out
}
