// The adage mod's $.state contract (docs/claude-mod/architecture.md §2 rule 3): only what a
// drawing reads this session. Adage's own state stays in $ADAGE_HOME.

/** One `<adage-reflex …>` block a command hook delivered, as its opening tag names it. */
export type AdageFired = {
  /** The classic event whose command hook answered with the block (`UserPromptSubmit`, …). */
  event: string
  route: string
  track: string
  kind: string
  strength: string
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

declare module 'claude-code' {
  interface PluginState {
    'adage-exec': {
      fired: AdageFired[]
      predicted: AdagePredicted[]
    }
  }
}
