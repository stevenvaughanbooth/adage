// Keyboard-first: one global handler for shortcuts and `g` sequences.

import { useEffect, useRef } from "react";

export interface Shortcut {
  keys: string[];
  label: string;
  group: "Go to" | "Map" | "Everywhere";
}

export const SHORTCUTS: Shortcut[] = [
  { keys: ["⌘", "K"], label: "Search reflexes and commands", group: "Everywhere" },
  { keys: ["?"], label: "This sheet", group: "Everywhere" },
  { keys: ["t"], label: "Theme: system, night, day", group: "Everywhere" },
  { keys: ["esc"], label: "Close, or clear the selection", group: "Everywhere" },
  { keys: ["g", "m"], label: "Map", group: "Go to" },
  { keys: ["g", "t"], label: "Traces", group: "Go to" },
  { keys: ["g", "e"], label: "Evidence", group: "Go to" },
  { keys: ["g", "h"], label: "Health", group: "Go to" },
  { keys: ["/"], label: "Filter reflexes", group: "Map" },
  { keys: ["j"], label: "Next reflex", group: "Map" },
  { keys: ["k"], label: "Previous reflex", group: "Map" },
  { keys: ["a"], label: "Aggregated / expanded", group: "Map" },
  { keys: ["f"], label: "Focus: 1, 2, 3 hops, off", group: "Map" },
  { keys: ["0"], label: "Fit the map", group: "Map" },
  { keys: ["+", "−"], label: "Zoom in, out", group: "Map" },
  { keys: ["c"], label: "Copy the reflex's first command", group: "Map" },
  { keys: ["space"], label: "Replay how the map formed", group: "Map" },
];

export type KeyHandler = (key: string, e: KeyboardEvent) => boolean | void;

function typing(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  return t.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(t.tagName);
}

/**
 * Calls `handler` with a normalised key: "mod+k", "g m" for sequences, or the key itself. Keys
 * typed into fields are left alone, except mod+k and Escape.
 */
export function useKeys(handler: KeyHandler) {
  const h = useRef(handler);
  h.current = handler;
  useEffect(() => {
    let pendingG = 0;
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        if (h.current("mod+k", e) !== false) e.preventDefault();
        return;
      }
      if (e.key === "Escape") {
        h.current("escape", e);
        return;
      }
      if (typing(e) || e.metaKey || e.ctrlKey || e.altKey) return;
      if (document.querySelector("[aria-modal='true']")) return;
      const now = performance.now();
      if (pendingG && now - pendingG < 1200) {
        pendingG = 0;
        if (h.current(`g ${e.key.toLowerCase()}`, e) !== false) e.preventDefault();
        return;
      }
      if (e.key === "g") {
        pendingG = now;
        return;
      }
      const k = e.key === " " ? "space" : e.key;
      if (h.current(k, e) === true) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
