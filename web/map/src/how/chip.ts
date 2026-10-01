// How a caption chip reads. A gate that passed carries a check; one that failed reads as failing,
// "support 2 < 5", rather than as the bar it missed, "support 2 ≥ 5".

import type { Chip } from "./types";

export function chipText(c: Chip): string {
  return c.ok === false ? c.text.replace(" ≥ ", " < ") : c.text;
}

/** The mark before a gate's chip: a check when it passed, a cross when it failed, none otherwise. */
export function chipMark(c: Chip): "✓" | "✗" | null {
  return c.ok === true ? "✓" : c.ok === false ? "✗" : null;
}
