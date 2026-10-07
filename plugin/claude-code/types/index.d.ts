// The adage mod's $.state contract (docs/claude-mod/architecture.md §2 rule 3): only what a
// drawing reads this session. Adage's own state stays in $ADAGE_HOME; every value here is a copy
// of what the binary said (a hook's answer, a feed line, a `--json` read) or of what the person did
// in the pane, so a reload loses nothing the binary cannot say again.

/** One `<adage-reflex …>` block a command hook delivered, as its opening tag names it. */
export type AdageFired = {
  /** The classic event whose command hook answered with the block (`UserPromptSubmit`, …). */
  event: string
  route: string
  track: string
  kind: string
  strength: string
  /** For `UserPromptSubmit`: the prompt's place among this session's prompts (1 = the first). */
  prompt?: number
  /** For `UserPromptSubmit`: the prompt as typed, so a line is never drawn under another prompt. */
  promptText?: string
}

/** One step Adage predicted in place of a model request (the `adage_` tool_use ids). */
export type AdagePredicted = {
  turnId: string
  index: number
  route: string
  kind: string
  ids: string[]
  /** True when the model's own end_turn stop was replaced (the finish line). */
  isEnding: boolean
}

/** A marker under one tool row, keyed by the call's tool_use_id. */
export type AdageMark =
  | { id: string; kind: 'rewrite'; interpreter: string; what: 'test' | 'python' }
  | { id: string; kind: 'substituted'; command: string }
  | { id: string; kind: 'avoided'; route: string }
  /** A receipt the hook delivered beside a run that did happen (the run was not answered by it). */
  | { id: string; kind: 'receipt'; receipt: string }
  /**
   * A receipt that was the call's result (plan C3 writes it): the run did not happen, and the
   * row shows the receipt in place of the tool's output.
   */
  | { id: string; kind: 'answered'; receipt: string; summary: string; tests: string[]; ago: string; command: string }

/** One tool batch the command hooks saw: its calls, and when the hooks beneath ran. */
export type AdageBatch = { ids: string[]; from: number; to: number }

/** The follower child (`adage watch --follow --json`). */
export type AdageFeed = {
  status: 'off' | 'following' | 'reconnecting' | 'stopped' | 'paused'
  /** The last cursor a line carried: a restart resumes from it. */
  cursor: string
  /** Consecutive children that ended early. */
  failures: number
}

/** One act of a feed row (A3's `acts[]`), as the mod keeps it. */
export type AdageAct = {
  act: string
  route?: string
  strength?: string
  detail?: string
  what?: string
  interpreter?: string
  rewrite?: boolean
  /** For `noticed` (plan D2): the sequence noticed, as the binary words it. */
  candidate?: string
  /** For `noticed`: the earlier sessions that showed the sequence, and how many make a reflex. */
  support?: number
  s_min?: number
  /** For `noticed`: the candidate waits for the person's review before it can become a reflex. */
  review?: boolean
}

/** One decision row of the feed. */
export type AdageRow = {
  /** Arrival order, for keys and selection. */
  seq: number
  ts: number
  /** The spool stem: the session id as the binary files it. */
  session: string
  harness: string
  event: string
  /** Set only on another session's row in the control arm (never on this session's). */
  control: boolean
  acts: AdageAct[]
  observed: number
  /** The directives as the agent read them. */
  texts: { route: string; text: string }[]
  /** This session's own row. */
  mine: boolean
  /** The calls the record named (`tool_use_ids`), when it did: a batch's calls in order. */
  ids?: string[]
}

/** One line of the conditioning ticker (`learn` feed lines). */
export type AdageTick = { ts: number; text: string; tone: string; strength: string | null }

export type AdageLight = { label: string; lamp: 'ok' | 'warn' | 'fail' | 'off'; hint: string | null }

/** The feed's `health` line: the lights, the worst check, and whether Adage is off here. */
export type AdageHealth = {
  lights: AdageLight[]
  word: 'healthy' | 'check' | 'problem'
  lamp: 'fire' | 'prime' | 'inhibit'
  check: string | null
  off: boolean
}

/** The feed's `learner` line, and when the mod saw it. */
export type AdageLearner = {
  state: 'waiting' | 'running' | 'idle'
  nextAt: number | null
  /** When a run was last seen ending (running → idle): `learned` shows for 10 s. */
  ranAt: number | null
}

/** One reflex from `adage graph --json`, as the pane and the lines read it (no arm-naming field). */
export type AdageReflex = {
  id: string
  track: string
  reference: string
  summary: string
  gist: string
  strength: string
  state: string
  instinct: boolean
  imported: boolean
  /** Who signed an imported reflex's pack, when the origin names one. */
  signer: string
  trigger: string
  step: string
  support: number
  opportunities: number
  lowerBound: number | null
  describe: string
  uplift: string
  directive: string
  verbs: string[]
  reasons: string[]
  offers: { kind: string; command: string }[]
}

/** One `adage doctor --json` check. */
export type AdageCheck = {
  label: string
  status: 'pass' | 'warn' | 'fail'
  detail: string
  fix: string
  action: { kind: 'run'; command: string } | { kind: 'manual' } | null
}

/** A grant that no longer matches (`delegate status --json`'s `lapsed`). */
export type AdageLapse = { route: string; changed: string[]; reapprove: string }

/** A team pack committed in the repository and not yet trusted (`status --json`). */
export type AdagePack = { name: string; command: string }

/** Where the mod stands in this session. */
export type AdageMode = {
  active: boolean
  interactive: boolean
  surface: string | null
  /** The shim's reason when no binary could serve (`$DATA/status`); empty otherwise. */
  nobinary: string
  ascii: boolean
  reducedMotion: boolean
  /** The opt-in lines (`ui.spinner`, `ui.turn_line`, `ui.noticed`), as the binary reports them. */
  spinner: boolean
  turnLine: boolean
  noticed: boolean
  /** `» Claude Code … is newer …`, until the first prompt; empty otherwise. */
  versionLine: string
  /** A newer Adage, as the status card says it (`update`); null when it says none. */
  update: AdageUpdate | null
  /** Status warnings and once-per-session toasts already shown, by id. */
  warned: string[]
}

/** A newer Adage (the status card's `update`, drawn as the binary words it; onboarding plan O9). */
export type AdageUpdate = {
  /** This binary's version (the card's `current`). */
  have: string
  latest: string
  /** The command that brings it, as the card words it for a session (`/adage update`). */
  next: string
  /** The release's first lines, when the release said them. */
  notes: string[]
  /** Whether the band's update line shows this session, until the first prompt (once per release per machine). */
  band: boolean
}

/** What the person did in the pane. */
export type AdageView = {
  tab: 'live' | 'reflexes' | 'health'
  filter: string
  /** The Live row inspected (its `seq`), kept selected after the inspect pane closes. */
  selected: number | null
  hideQuiet: boolean
  paused: boolean
  rfilter: string
  strength: string
  lifecycle: string
  /** The Reflexes row expanded (`reference/id`). */
  expanded: string
}

/**
 * The current turn: how many prompts this session submitted, and where the turn's fired reflexes,
 * predicted steps and marks begin in their lists (the spinner and the turn line count from there).
 */
export type AdageTurn = { submits: number; fired: number; predicted: number; marks: number }

/** The map (`Explore in browser`): its address once it serves. */
export type AdageMap = { url: string; starting: boolean }

// --------------------------------------------- B4 · D5: the press, the asks, noticed and skills

/**
 * The press (plan B4, architecture §3.12): the single-use token the binary issued at this
 * session's start, held in engine memory only, never on argv, never in a file of the mod's.
 */
export type AdagePress = {
  /** The session id it was issued for ('' before the first start). */
  session: string
  /** The token the next press spends (64 hex); '' while none is held. */
  token: string
  /**
   * `held`; `none` (the window was missed, the binary refused one, or the mod reloaded before a
   * start); `off` (an issue the mod did not ask for: presses stop for the session).
   */
  state: 'held' | 'none' | 'off'
  /** Tokens the binary issued to the mod for this session (`press_issued` records are held to it). */
  issued: number
  /** Presses spent this session. */
  chain: number
}

/** One thing the mod asked about at a turn's end, by its waiting key (ux-design §4.17). */
export type AdageAsked = {
  key: string
  /** When it was last asked. */
  at: number
  /** How often the dialog was dismissed (`later`): twice, and it is not asked again. */
  times: number
  /** Answered (any answer but `why` and `later`): never asked again. */
  done: boolean
}

/** What `review delegate … --stage --json` answered, as the grant pane draws it. */
export type AdageStage = {
  ok: boolean
  why: string
  digest: string
  /** The first action on this machine (the policy and the marker come with the grant). */
  first: boolean
  argv: string[]
  diff: { path: string; change: string; plus: number; minus: number; text: string }[]
  refused: { path: string; why: string }[]
  scripts: { path: string; sha256: string; text: string }[]
  sandbox: string
  profile: string
  hashes: string[]
  effect: string
  cache: string[]
  outputs: string[]
  /** What the step reads (the input contracts), for a step that changes nothing. */
  inputs: string[]
}

/** The grant pane (`see what it would do`): the offer, the stage, where the person is in it. */
export type AdageAllow = {
  key: string
  say: string
  kind: 'execute' | 'predict'
  track: string
  route: string
  /** The terminal command for persona (b), behind Copy. */
  command: string
  stage: AdageStage | null
  details: boolean
  /** '' or what the pane says above the facts: the repository moved, or the binary's refusal. */
  note: string
  busy: boolean
}

/** A skill expanded with the steps Adage verified here (`skill.prompt`, plan D4). */
export type AdageSkill = {
  skill: string
  at: number
  /** The date in the heading the binary wrote. */
  date: string
  steps: { command: string; result: 'works' | 'stale'; signature: string }[]
}

/** A candidate as `status --json` words it (plan D1): what it would tell the agent, never its id. */
export type AdageCandidate = { id: string; say: string; sequence: boolean }

declare module 'claude-code' {
  interface PluginState {
    adage: {
      fired: AdageFired[]
      predicted: AdagePredicted[]
      marks: AdageMark[]
      batches: AdageBatch[]
      feed: AdageFeed
      rows: AdageRow[]
      ticks: AdageTick[]
      health: AdageHealth | null
      learner: AdageLearner | null
      reflexes: AdageReflex[]
      checks: AdageCheck[]
      lapsed: AdageLapse[]
      packs: AdagePack[]
      mode: AdageMode
      view: AdageView
      map: AdageMap
      turn: AdageTurn
      press: AdagePress
      asked: AdageAsked[]
      allow: AdageAllow | null
      skills: AdageSkill[]
      candidates: AdageCandidate[]
    }
  }
}
