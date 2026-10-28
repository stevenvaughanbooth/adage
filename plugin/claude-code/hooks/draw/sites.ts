// Every site Adage draws in (docs/claude-mod/ux-design.md §4–§7), one function per `ui.render`
// registration. Each tree is a function of `$.state` (read through the `Mod`): a site draws only
// for a person at the prompt, on the terminal or the desktop (§4.10), and returns next(e) whenever
// it has nothing to add. The words come from view/*, the trees from the other draw/* files.
import { paneActions, copy } from '../actions'
import { allowActions } from '../allow'
import { verbOf } from '../commands'
import { pressHandlers } from '../dialog'
import type { AnswerAt, EventAt, Mod, NextAt } from '../mod'
import { quietly } from '../mod'
import type { Mark } from '../plugin'
import { noticedLines, wordCandidates } from '../notice'
import { isRecord } from '../shape'
import { invokedMark, verifiedMark } from '../skill'
import { band, firedThisSession } from '../view/band'
import { firedLines, markText, openNotice, receiptBeside, receiptCard, spinnerSuffix, turnSummary } from '../view/lines'
import type { RowMark } from '../view/lines'
import { inspectText, waiting, worthAsking } from '../view/pane'
import { allowTree } from './allow'
import { bandTree } from './band'
import { cardTree, whyTree } from './card'
import { inspectTree, paneTree } from './pane'
import { beside, receipt, skillRow, under } from './rows'

/** Whether a site may draw: a person at the prompt, on a surface the trees fit (ux-design §4.10). */
function drawable(m: Mod, e: { surface: string }): boolean {
  return m.rt.interactive && (e.surface === 'terminal' || e.surface === 'desktop')
}

/** A pane draws on the terminal and the desktop, whoever opened it. */
const paneSurface = (e: { surface: string }): boolean => e.surface === 'terminal' || e.surface === 'desktop'

/** The opening line (`open.notice`): the binary's own notice, with the glyph and no prefix. */
export async function onInfoNotice(m: Mod, e: EventAt<'ui.render', { component: 'InfoNotice' }>, next: NextAt<'ui.render', { component: 'InfoNotice' }>): AnswerAt<'ui.render', { component: 'InfoNotice' }> {
  if (!drawable(m, e) || m.rt.bin === undefined) return next(e)
  const text = openNotice(e.props.text, (await m.state.mode.read()).ascii)
  return text === undefined ? next(e) : next({ ...e, props: { ...e.props, text } })
}

/**
 * `prompt.fired`: one line per reflex fired at the prompt, under the engine's row. The row is
 * matched to the hook's answer by the prompt's text; the payload carries no message id, and a
 * prompt typed twice draws the later one's lines.
 */
export async function onUserMessage(m: Mod, e: EventAt<'ui.render', { component: 'UserMessage' }>, next: NextAt<'ui.render', { component: 'UserMessage' }>): AnswerAt<'ui.render', { component: 'UserMessage' }> {
  if (!drawable(m, e) || m.rt.bin === undefined || e.props.isExpanded) return next(e)
  const fired = (await m.state.fired.read()).filter(f => f.event === 'UserPromptSubmit' && f.promptText === e.props.text)
  const n = Math.max(0, ...fired.map(f => f.prompt ?? 0))
  const these = fired.filter(f => f.prompt === n)
  if (these.length === 0) return next(e)
  const ascii = (await m.state.mode.read()).ascii
  const lines = firedLines(these, await m.state.reflexes.read(), await m.state.rows.read(), e.viewport?.columns ?? 80, ascii)
  return lines.length === 0 ? next(e) : under(m.ui.resolve(e), await next(e), lines)
}

/**
 * `tool.fired`: the lines for the reflexes the command hooks delivered after a tool batch, drawn
 * once, under the row of the batch's last call (one of `ids`), whether that row is a call's own or
 * its group's one line.
 */
async function toolFired(m: Mod, ids: readonly string[], columns: number, ascii: boolean): Promise<string[]> {
  const these = (await m.state.fired.read()).filter(f => f.ids !== undefined && f.ids.length > 0 && ids.includes(f.ids[f.ids.length - 1] ?? ''))
  if (these.length === 0) return []
  return firedLines(these, await m.state.reflexes.read(), await m.state.rows.read(), columns, ascii)
}

/**
 * The marker under a tool row: a rewrite, a predicted step, a substitution, a dead end headed off;
 * then what Adage told the agent after it (`tool.fired`).
 */
export async function onToolUse(m: Mod, e: EventAt<'ui.render', { component: 'ToolUse' }>, next: NextAt<'ui.render', { component: 'ToolUse' }>): AnswerAt<'ui.render', { component: 'ToolUse' }> {
  if (!drawable(m, e) || m.rt.bin === undefined) return next(e)
  const id = e.props.tool_use_id || e.requestId
  const marks: RowMark[] = (await m.state.marks.read()).filter(
    (x): x is Extract<Mark, { kind: 'rewrite' | 'substituted' | 'avoided' }> => x.id === id && (x.kind === 'rewrite' || x.kind === 'substituted' || x.kind === 'avoided'),
  )
  if ((await m.state.predicted.read()).some(p => p.ids.includes(id))) marks.unshift({ kind: 'predicted' })
  const ascii = (await m.state.mode.read()).ascii
  const lines = [...marks.map(x => markText(x, ascii)), ...(await toolFired(m, [id], e.viewport?.columns ?? 80, ascii))]
  if (lines.length === 0) return next(e)
  return under(m.ui.resolve(e), await next(e), lines)
}

/** The receipt card in place of a test run's output, or one line beside a run that happened. */
export async function onToolResult(m: Mod, e: EventAt<'ui.render', { component: 'ToolResult' }>, next: NextAt<'ui.render', { component: 'ToolResult' }>): AnswerAt<'ui.render', { component: 'ToolResult' }> {
  if (!drawable(m, e) || m.rt.bin === undefined) return next(e)
  const id = e.props.tool_use_id || e.requestId
  const marks = (await m.state.marks.read()).filter(x => x.id === id)
  const ascii = (await m.state.mode.read()).ascii
  const answered = marks.find((x): x is Extract<Mark, { kind: 'answered' }> => x.kind === 'answered')
  if (answered !== undefined) return receipt(m.ui.resolve(e), receiptCard(answered, ascii))
  const besides = marks.find((x): x is Extract<Mark, { kind: 'receipt' }> => x.kind === 'receipt')
  if (besides === undefined) return next(e)
  return under(m.ui.resolve(e), await next(e), [receiptBeside(besides.receipt, ascii)])
}

/** `spinner.suffix` (`ui.spinner`, off by default): while a reflex was delivered this turn. */
export async function onSpinner(m: Mod, e: EventAt<'ui.render', { component: 'Spinner' }>, next: NextAt<'ui.render', { component: 'Spinner' }>): AnswerAt<'ui.render', { component: 'Spinner' }> {
  if (!drawable(m, e) || m.rt.bin === undefined) return next(e)
  const mode = await m.state.mode.read()
  if (!mode.spinner) return next(e)
  const turn = await m.state.turn.read()
  const n = new Set((await m.state.fired.read()).slice(turn.fired).map(f => f.route)).size
  const suffix = spinnerSuffix(n, mode.ascii)
  return suffix === '' ? next(e) : next({ ...e, props: { ...e.props, suffix: `${e.props.suffix}${suffix}` } })
}

/** `turn.summary` (`ui.turn_line`, off by default; the terminal only): the turn's counts. */
export async function onTurnDuration(m: Mod, e: EventAt<'ui.render', { component: 'TurnDuration' }>, next: NextAt<'ui.render', { component: 'TurnDuration' }>): AnswerAt<'ui.render', { component: 'TurnDuration' }> {
  if (!drawable(m, e) || m.rt.bin === undefined) return next(e)
  const mode = await m.state.mode.read()
  if (!mode.turnLine) return next(e)
  const turn = await m.state.turn.read()
  const marks = (await m.state.marks.read()).slice(turn.marks)
  const text = turnSummary(
    (await m.state.predicted.read()).length - turn.predicted,
    marks.filter(x => x.kind === 'answered').length,
    marks.filter(x => x.kind === 'avoided' || x.kind === 'substituted').length,
    mode.ascii,
  )
  return text === '' ? next(e) : beside(m.ui.resolve(e), await next(e), text)
}

/** The band (§5), registered unless the person turned it off (`band` in /config). */
export async function onAbovePrompt(m: Mod, e: EventAt<'ui.render', { component: 'AbovePrompt' }>, next: NextAt<'ui.render', { component: 'AbovePrompt' }>): AnswerAt<'ui.render', { component: 'AbovePrompt' }> {
  if (!drawable(m, e) || e.props.hasSurvey) return next(e)
  const mode = await m.state.mode.read()
  const view = await m.state.view.read()
  const rows = await m.state.rows.read()
  const turn = await m.state.turn.read()
  const marks = await m.state.marks.read()
  const learner = await m.state.learner.read()
  const b = band({
    active: mode.active,
    nobinary: mode.nobinary,
    paused: view.paused,
    health: await m.state.health.read(),
    inRepo: learner !== null,
    reflexes: firedThisSession(await m.state.fired.read(), rows),
    steps: (await m.state.predicted.read()).length,
    reused: marks.filter(x => x.kind === 'answered').length,
    learner,
    now: await m.clock.now(),
    lapsed: (await m.state.lapsed.read()).length,
    review: worthAsking(waiting(await m.state.reflexes.read(), await m.state.packs.read())).length,
    update: turn.submits === 0 && mode.update?.band === true ? { latest: mode.update.latest, next: mode.update.next } : null,
    versionLine: turn.submits === 0 ? mode.versionLine : '',
    columns: e.props.bodyColumns,
    ascii: mode.ascii,
    kept: m.rt.kept,
    noticed: m.rt.noticed,
    paneOpen: m.rt.paneOpen,
    withheld: m.rt.withheldSaid !== undefined && m.rt.withheldSaid.submits === turn.submits && m.rt.withheldSaid.session === (await m.session.id()),
  })
  return b === undefined ? next(e) : bandTree(m.ui.resolve(e), b, mode.ascii ? '-' : '·')
}

/** The pane (§6): Graph, Live, Reflexes, Health, World; its presses once a person is at the prompt. */
export async function onPane(m: Mod, e: EventAt<'ui.render', { component: 'Pane'; requestId: 'adage' }>, next: NextAt<'ui.render', { component: 'Pane'; requestId: 'adage' }>): AnswerAt<'ui.render', { component: 'Pane'; requestId: 'adage' }> {
  if (!paneSurface(e)) return next(e)
  const mode = await m.state.mode.read()
  const reflexes = await m.state.reflexes.read()
  return paneTree(
    m.ui.resolve(e),
    {
      view: await m.state.view.read(),
      rows: await m.state.rows.read(),
      ticks: await m.state.ticks.read(),
      health: await m.state.health.read(),
      feed: (await m.state.feed.read()).status,
      reflexes,
      fired: await m.state.fired.read(),
      waiting: waiting(reflexes, await m.state.packs.read()),
      checks: await m.state.checks.read(),
      lapsed: await m.state.lapsed.read(),
      update: mode.update,
      map: await m.state.map.read(),
      nobinary: mode.nobinary,
      noBridge: e.surface === 'desktop' && !mode.active,
      columns: e.props.bodyColumns,
      ascii: mode.ascii,
      boxes: e.surface === 'terminal',
      dag: await m.state.dag.read(),
      predicted: await m.state.predicted.read(),
      marks: await m.state.marks.read(),
      now: await m.clock.now(),
      motion: !mode.reducedMotion,
      bodyRows: e.props.scroll.bodyRows,
      world: await m.state.world.read(),
    },
    { ...paneActions(m), press: m.rt.pressBound ? pressHandlers(m) : undefined },
  )
}

/** The inspect pane over the Live tab: the selected row's directives and evidence. */
export async function onInspectPane(m: Mod, e: EventAt<'ui.render', { component: 'Pane'; requestId: 'adage-inspect' }>, next: NextAt<'ui.render', { component: 'Pane'; requestId: 'adage-inspect' }>): AnswerAt<'ui.render', { component: 'Pane'; requestId: 'adage-inspect' }> {
  if (!paneSurface(e)) return next(e)
  const view = await m.state.view.read()
  const row = (await m.state.rows.read()).find(r => r.seq === view.selected)
  if (row === undefined) return next(e)
  return inspectTree(m.ui.resolve(e), inspectText(row, await m.state.reflexes.read()))
}

/** `/adage` and `/adage why` drawn in their output row (§7); every other verb's line as printed. */
export async function onCommandOutput(m: Mod, e: EventAt<'ui.render', { component: 'CommandOutput'; props: { command: 'adage' } }>, next: NextAt<'ui.render', { component: 'CommandOutput'; props: { command: 'adage' } }>): AnswerAt<'ui.render', { component: 'CommandOutput'; props: { command: 'adage' } }> {
  if (!drawable(m, e) || e.props.isErrored) return next(e)
  const verb = verbOf(e.props.args)
  if (verb === 'why') return whyTree(m.ui.resolve(e), e.props.text)
  if ((verb === 'card' || verb === 'status' || verb === 'savings') && e.props.text.includes('\n')) {
    return cardTree(m.ui.resolve(e), e.props.text, (command, at) => quietly(copy(m, command, at)))
  }
  return next(e)
}

/** `noticed.forming` / `noticed.review` under the tool batch that completed a candidate (§3.10). */
export async function onToolGroup(m: Mod, e: EventAt<'ui.render', { component: 'ToolGroup' }>, next: NextAt<'ui.render', { component: 'ToolGroup' }>): AnswerAt<'ui.render', { component: 'ToolGroup' }> {
  if (!drawable(m, e) || m.rt.bin === undefined) return next(e)
  const mode = await m.state.mode.read()
  const ids = e.props.calls.map(c => c.tool_use_id).filter((id): id is string => typeof id === 'string' && id !== '')
  if (ids.length === 0) return next(e)
  // Expanded, each call draws its own ToolUse row, which carries `tool.fired`; collapsed, the group's line does.
  const fired = e.props.isExpanded ? [] : await toolFired(m, ids, e.viewport?.columns ?? 80, mode.ascii)
  if (!mode.noticed) return fired.length === 0 ? next(e) : under(m.ui.resolve(e), await next(e), fired)
  const rows = await m.state.rows.read()
  const lines = [...noticedLines(rows, ids, await m.state.batches.read(), await m.state.candidates.read(), mode.ascii), ...fired]
  quietly(wordCandidates(m, rows))
  if (lines.length === 0) return next(e)
  return under(m.ui.resolve(e), await next(e), lines)
}

/** The skill markers (`tool.skill.invoked`, `tool.skill.verified`) under a Skill tool row (§4.8). */
export async function onSkillToolUse(m: Mod, e: EventAt<'ui.render', { component: 'ToolUse'; props: { tool: 'Skill' } }>, next: NextAt<'ui.render', { component: 'ToolUse'; props: { tool: 'Skill' } }>): AnswerAt<'ui.render', { component: 'ToolUse'; props: { tool: 'Skill' } }> {
  if (!drawable(m, e) || m.rt.bin === undefined) return next(e)
  const id = e.props.tool_use_id || e.requestId
  const name = isRecord(e.props.input) && typeof e.props.input.skill === 'string' ? e.props.input.skill : ''
  const ascii = (await m.state.mode.read()).ascii
  const lines: string[] = []
  if ((await m.state.predicted.read()).some(p => p.kind === 'skill' && p.ids.includes(id))) lines.push(invokedMark(name === '' ? 'this' : name, ascii))
  const verified = (await m.state.skills.read()).find(s => s.skill === name)
  if (verified !== undefined) lines.push(verifiedMark(verified, ascii))
  if (lines.length === 0) return next(e)
  return skillRow(m.ui.resolve(e), await next(e), lines)
}

/** The grant pane (`see what it would do`, §4.17): the facts between the two acts. */
export async function onAllowPane(m: Mod, e: EventAt<'ui.render', { component: 'Pane'; requestId: 'adage-allow' }>, next: NextAt<'ui.render', { component: 'Pane'; requestId: 'adage-allow' }>): AnswerAt<'ui.render', { component: 'Pane'; requestId: 'adage-allow' }> {
  if (!paneSurface(e)) return next(e)
  const allow = await m.state.allow.read()
  if (allow === null) return next(e)
  return allowTree(m.ui.resolve(e), allow, e.props.bodyColumns, (await m.state.mode.read()).ascii, allowActions(m))
}
