// What every hook function is handed (docs/claude-mod/impl/mod-structure.md): the engine's
// capabilities as plain closures (`Caps`), the mod's runtime (what one load of the module keeps
// between hooks) and the plugin's options, together a `Mod`.
//
// `claude plugin validate` follows `$` only into functions declared in the file that registers the
// hooks, and refuses `$` (or any of its nouns) passed across an import. register.ts therefore
// builds the capabilities over its `$`, one closure per call the mod makes, and hands modules the
// `Mod`; no other file sees `$`. A module is a function of a `Mod`, which a test builds without the
// engine (fixtures/kit.ts `fake`).
import type { EngineInterface, HookFor, MatchedHook, McpToolResult, Pattern, Register, RenderSurface, Timer, ToolInfo } from 'claude-code'

import type { Run, Spawn } from './bridge'
import type { Els } from './draw/els'
import type { StepUsage, Noted } from './ledger'
import { NOTED0 } from './ledger'
import type { AdageState, Skill, StoreKey } from './plugin'
import type { Standing } from './attach'
import type { Withheld } from './step'

/** One `$.state` value: read (its initial while nothing was written) and updated (read, apply, write). */
export type Cell<T> = { read: () => Promise<T>; update: (change: (value: T) => T) => Promise<T> }

/** Every `$.state` key of plugin.ts's `STATE0`, one cell each (register.ts binds each to its atom). */
export type State = { readonly [K in keyof AdageState]: Cell<AdageState[K]> }

/** One `$.store` key (`adage.<key>`): what it holds, untyped, since anything can write the file. */
export type StoreCell = { get: () => Promise<unknown>; set: (value: unknown) => Promise<void> }

/** Every `$.store` key of plugin.ts's `STORE_KEYS`. */
export type Store = { readonly [K in StoreKey]: StoreCell }

/** One environment variable the mod reads (each a literal name in register.ts). */
type Env = () => Promise<string | undefined>

/**
 * The engine's capabilities the mod uses, each a closure register.ts builds over its `$`. The
 * three runners are the only way to a process, and each is already behind its allow-list: `run`
 * and `spawn` behind bridge.ts `guard` and `guardSpawn`, `pressRun` behind press.ts `guardPress`.
 */
export type Caps = {
  state: State
  store: Store
  run: Run
  spawn: Spawn
  pressRun: Run
  /** CLAUDE_PLUGIN_DATA; CLAUDE_CONFIG_DIR, HOME and USERPROFILE (a --plugin-dir plugin's data directory is derived from them, binary.ts `dataDir`); the mod's own switches; OS. */
  env: { pluginData: Env; configDir: Env; home: Env; userProfile: Env; observe: Env; ascii: Env; reducedMotion: Env; os: Env }
  /** The plugin's folder and its manifest name. */
  plugin: { root: () => string; name: () => string }
  fs: { read: (path: string) => Promise<unknown>; exists: (path: string) => Promise<boolean> }
  command: Pick<EngineInterface['command'], 'register' | 'list'>
  session: Pick<EngineInterface['session'], 'id' | 'cwd' | 'repo' | 'usage' | 'version'>
  clock: { now: () => Promise<number>; every: (ms: number, fn: () => void) => Timer; after: (ms: number, fn: () => void) => Timer }
  /** The person's rules on a Bash command now, as the engine decides them (`allow`, `ask`, `deny`; nothing runs). */
  checkBash: (command: string) => Promise<string>
  /**
   * The engine's verdict on any tool's call now (`$.tool.check`: `{ decision, reason?, rule? }`;
   * nothing runs, no dialog opens, auto mode's classifier is not asked).
   */
  checkTool: (tool: string, input: Record<string, unknown>) => Promise<unknown>
  /**
   * A connected MCP server's tool, called with the engine's own connection and credentials
   * (`$.mcp.call`; the world's connector sources): Adage holds no token and signs in nowhere.
   */
  mcp: { call: (server: string, tool: string, args: Record<string, unknown>) => Promise<McpToolResult> }
  /** The tools the model can call now, MCP tools included (`$.tool.list`): which servers are connected. */
  tool: { list: () => Promise<ToolInfo[]> }
  ui: {
    status: (text: string) => void
    toast: (text: string) => void
    notice: (toolUseId: string, text: string) => void
    open: EngineInterface['ui']['open']
    close: EngineInterface['ui']['close']
    ask: (question: string, answers: readonly string[]) => Promise<string>
    copy: EngineInterface['ui']['copy']
    scroll: EngineInterface['ui']['scroll']
    /** Asks for the drawings to be drawn again (`ui.render`). */
    invalidate: () => void
    /** The elements of the surface a drawing hook draws on (`$.ui.resolve(e)`). */
    resolve: (e: Parameters<EngineInterface['ui']['resolve']>[0]) => Els
  }
}

/** The plugin's options (`userConfig`): `band` and `pane`. */
export type Options = Parameters<Register>[1]

/** A child the mod can end (the follower, the map). */
type Stoppable = AsyncGenerator<unknown, unknown>

/**
 * What one load of the module keeps between hooks: none of it is drawn (what is drawn is in
 * `$.state`), and a reload starts it again, which loses nothing the binary cannot say again.
 */
export type Runtime = {
  /** The hook binary that answered the probe; undefined while the mod is inert. */
  bin: string | undefined
  /** `adage` itself, beside it (`<data>/bin/adage`): the verbs the mod reads and draws from. */
  cli: string | undefined
  /** The suffix of the shim's entries: `.exe` on Windows, where it copies `hook.exe` and `adage.exe`. */
  exe: string
  /** Whether a person is at the prompt (`session.start`'s `isInteractive`): nothing draws headless. */
  interactive: boolean
  /** The surface the session drew on at start. */
  surface: RenderSurface | null
  /**
   * Observe mode (ADAGE_MOD_OBSERVE set and non-empty; the demo's `mod-observe` arm): the binary is
   * still asked at every step and Bash call, with `observe`, and records what it would have done as
   * the control arm does; the mod never yields a predicted call and never replaces a command.
   */
  observing: boolean
  /** The follower: the session it follows, its generation (a new one ends the old), its child. */
  feedSession: string
  feedGen: number
  paused: boolean
  child: Stoppable | undefined
  /** The redraw timers (the learner's countdown, `learned` fading, the Graph tab's newest node settling). */
  countdown: Timer | undefined
  fading: Timer | undefined
  pulse: Timer | undefined
  /** When the `--json` reads were last made, and `status --json` for the candidates' words. */
  lastRefresh: number
  lastCandidates: number
  /** The hook binary's failed answers in a row (three: a status warning, ux-design §9.2). */
  failures: number
  /** The person's routine model (`runtime.routine_model`, from the status card); '' routes nothing. */
  routineModel: string
  /** The repository the session works in, as the status card names it (`repo`); '' until it is read. */
  repo: string
  /** Whether a session that draws nothing has read the card for it (once, at its first step that could route). */
  settled: boolean
  /**
   * The session's `Standing` answer, asked once per session id (a /clear is a new one): the section,
   * the tool description and the compaction instruction read the same text all session, so the
   * prompt cache keeps it. A failed ask is the empty answer for the session.
   */
  standingFor: { session: string; answer: Promise<Standing> } | undefined
  /** The main-loop steps and tool calls of the turn under way, and the session's noted so far. */
  turnSteps: Omit<StepUsage, 'index'>[]
  turnCalls: number
  noted: Noted
  /** One ask at a time: a second turn end while the dialog waits asks nothing. */
  asking: boolean
  /** The session that last had a suggestion put to it at a turn's end (one per session; a /clear is a new one). */
  askedIn: string | undefined
  /** Whether presses are bound (a person is at the prompt): until then a review verb is a command to copy. */
  pressBound: boolean
  /** Answers kept for the next session start (no press was held): the band says so. */
  kept: number
  /** Distinct sequences noticed this session (the band's `1 pattern noticed`). */
  noticed: number
  /** Whether the pane is open: the noticed cell shows only while it is closed. */
  paneOpen: boolean
  /** The last skill expanded with verified steps (for `/adage why`). */
  skill: Skill | undefined
  /** The latest predicted steps the person's permissions held back (step.ts `keepWithheld`). */
  withheld: Withheld[]
  /** The session whose band said why a step was held back, and the prompt count it was raised at (`band.withheld`). */
  withheldSaid: { session: string; submits: number } | undefined
  /** The world's connector sources running this session (world/poll.ts), their generation, and its follow child's. */
  worldRuns: { resolved: unknown; failures: number; timer: { cancel: () => void } | undefined; polls: number }[]
  worldGen: number
  worldFeedGen: number
  worldChild: Stoppable | undefined
}

/** The runtime of a fresh load: inert, nothing followed, nothing asked. */
export function runtime(): Runtime {
  return {
    bin: undefined,
    cli: undefined,
    exe: '',
    interactive: false,
    surface: null,
    observing: false,
    feedSession: '',
    feedGen: 0,
    paused: false,
    child: undefined,
    countdown: undefined,
    fading: undefined,
    pulse: undefined,
    lastRefresh: -Infinity,
    lastCandidates: -Infinity,
    failures: 0,
    routineModel: '',
    repo: '',
    settled: false,
    standingFor: undefined,
    turnSteps: [],
    turnCalls: 0,
    noted: NOTED0,
    asking: false,
    askedIn: undefined,
    pressBound: false,
    kept: 0,
    noticed: 0,
    paneOpen: false,
    skill: undefined,
    withheld: [],
    withheldSaid: undefined,
    worldRuns: [],
    worldGen: 0,
    worldFeedGen: 0,
    worldChild: undefined,
  }
}

/** What every hook function takes first: the capabilities, the runtime, the options. */
export type Mod = Caps & { rt: Runtime; options: Options }

/**
 * Lets work run on without the hook waiting for it, failing quietly: a hook never fails because
 * a drawing's read did (fail open, architecture §6 invariant 2).
 */
export function quietly(work: Promise<unknown>): void {
  work.catch(() => undefined)
}

/** The hook `on(pattern)` or `on(pattern, matcher)` takes, as the engine types it. */
type HookAt<P extends Pattern, M> = [M] extends [never] ? HookFor<P> : MatchedHook<P, M>

/** A hook function's event and `next`, as the engine hands them to the hook registered for them. */
export type EventAt<P extends Pattern, M = never> = Parameters<HookAt<P, M>>[1]
export type NextAt<P extends Pattern, M = never> = Parameters<HookAt<P, M>>[2]

/** What a hook function resolves to: the hook's answer. */
export type AnswerAt<P extends Pattern, M = never> = Promise<Awaited<ReturnType<HookAt<P, M>>>>
