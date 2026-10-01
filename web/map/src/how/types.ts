// What a chapter of "How it works" provides. A chapter builds its scene once on the stage, then
// describes each step: the words (title, body, chips), the scene state, and optionally a 2D figure
// drawn under the caption. Steps must be readable without the 3D view (it may be unavailable).

import type { Graph } from "../api/types";
import type { Palette, SceneState, Stage } from "./engine";

export type Status = "built" | "flag";

export interface Chip {
  text: string;
  /** A gate that passed: drawn with a check. */
  ok?: boolean;
}

/** The blocks of "The math" on the site's /how page, in walkthrough order. */
export type MathId = "read" | "graph" | "evidence" | "uplift" | "intent" | "cards" | "train" | "seal" | "hook" | "measure";

export interface Step {
  title: string;
  /** The step for someone new to it: one or two sentences in everyday words. */
  plain: string;
  /** The precise account, shown under "More detail". */
  body: string;
  /** The block of "The math" this step relies on, if any. */
  math?: MathId;
  /** A stop on the quick tour. */
  key?: boolean;
  chips: Chip[];
  state: SceneState;
  /** A figure under the caption (a chart or a table), drawn on a 2D canvas. */
  figure?: (g: CanvasRenderingContext2D, w: number, h: number, pal: Palette) => void;
}

export interface LegendItem {
  label: string;
  role: Parameters<Palette["css"]>[0] | "hue";
  hue?: number;
}

export interface Built {
  steps: Step[];
  legend?: LegendItem[];
  /** What the chapter's data is, shown by the stage ("12 sample traces", "your 14 reflexes"). */
  source: string;
}

export interface Context {
  graph: Graph | null;
  /** Use the repository's own data where the chapter can. */
  repo: boolean;
}

export interface Chapter {
  id: string;
  /** What the chapter does, in everyday words. */
  title: string;
  /** The chapter's technical name. */
  technical: string;
  /** One line for the chapter list. */
  status: string;
  kind: Status;
  /** Two sentences: what this part of Adage does, in everyday words. */
  summary: string;
  /** Where it lives in the code. */
  source: string;
  /** Whether "This repository" can drive the chapter (from the map's data). */
  usesRepo: boolean;
  build(stage: Stage, ctx: Context): Built;
}
