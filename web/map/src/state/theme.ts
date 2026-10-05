// The theme: system (follows the OS), night or day. Saved per browser; public/theme.js applies it
// before first paint.

import { useSyncExternalStore } from "react";

export type ThemeMode = "system" | "night" | "day";
export const THEMES: ThemeMode[] = ["system", "night", "day"];
const KEY = "adage-map-theme";

function read(): ThemeMode {
  try {
    const t = localStorage.getItem(KEY);
    return t === "night" || t === "day" ? t : "system";
  } catch {
    return "system";
  }
}

let mode: ThemeMode = typeof window === "undefined" ? "system" : read();
const listeners = new Set<() => void>();

export function applyTheme(m: ThemeMode) {
  const root = document.documentElement;
  if (m === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", m);
}

export function setTheme(m: ThemeMode) {
  mode = m;
  try {
    if (m === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, m);
  } catch {
    /* private window: this page only */
  }
  applyTheme(m);
  for (const l of listeners) l();
}

export function cycleTheme() {
  setTheme(THEMES[(THEMES.indexOf(mode) + 1) % THEMES.length] ?? "system");
}

export function useTheme(): ThemeMode {
  return useSyncExternalStore(
    (f) => {
      listeners.add(f);
      return () => listeners.delete(f);
    },
    () => mode,
    () => mode,
  );
}

/** Whether the page is showing day colours now (for the few places that must know). */
export function isDay(m: ThemeMode): boolean {
  if (m === "day") return true;
  if (m === "night") return false;
  return typeof matchMedia !== "undefined" && matchMedia("(prefers-color-scheme: light)").matches;
}

export function reducedMotion(): boolean {
  return (
    (typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches) ||
    document.documentElement.getAttribute("data-motion") === "off"
  );
}
