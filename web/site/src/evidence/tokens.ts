// Numbers in the pages come from the data files in public/evidence, never from the HTML. At build
// time (vite.config.ts) every `{{ev:<file>:<path>|<format>}}` in a page becomes the value at <path>
// in public/evidence/<file>.json, and `{{heldout:<kind>}}` becomes a held-out component
// (src/evidence/heldout.ts: full, brief, card, headline, example). A missing file, path, format or
// kind fails the build.

import { HELDOUT_KINDS, type Heldout } from "./heldout";

const FORMATS: Record<string, (v: number) => string> = {
  int: (v) => Math.round(v).toLocaleString("en-US"),
  fixed0: (v) => v.toFixed(0),
  fixed1: (v) => v.toFixed(1),
  fixed2: (v) => v.toFixed(2),
  abs1: (v) => Math.abs(v).toFixed(1),
  usd3: (v) => `$${v.toFixed(3)}`,
  usd4: (v) => `$${v.toFixed(4)}`,
  x2: (v) => `${v.toFixed(2)}×`,
  // A signed change with a real minus sign: −10.3, +2.0.
  signed1: (v) => `${v < 0 ? "−" : "+"}${Math.abs(v).toFixed(1)}`,
  kilo: (v) => `${Math.round(v / 1000)}k`,
  // A ratio as "N for every 100".
  per100: (v) => String(Math.round(v * 100)),
  // A share as a percentage below 100: 0.6215 → 38 (fewer).
  less100: (v) => String(Math.round((1 - v) * 100)),
};

function at(data: unknown, path: string): unknown {
  let v: unknown = data;
  for (const k of path.split(".")) {
    if (v === null || typeof v !== "object") return undefined;
    v = (v as Record<string, unknown>)[/^\d+$/.test(k) && Array.isArray(v) ? Number(k) : k];
  }
  return v;
}

/** Replaces every data token in `html`, reading files with `load(name)`. */
export function fillTokens(html: string, load: (name: string) => unknown, page = "page"): string {
  return html
    .replace(/\{\{ev:([\w-]+):([\w.-]+)(?:\|(\w+))?\}\}/g, (_, file: string, path: string, format?: string) => {
      const v = at(load(file), path);
      if (v === undefined || v === null) throw new Error(`${page}: no value at ${file}.json ${path}`);
      if (format === undefined) return String(v);
      const f = FORMATS[format];
      if (!f) throw new Error(`${page}: unknown format ${format}`);
      if (typeof v !== "number") throw new Error(`${page}: ${file}.json ${path} is not a number`);
      return f(v);
    })
    .replace(/\{\{heldout:(\w+)\}\}/g, (_, kind: string) => {
      const draw = HELDOUT_KINDS[kind];
      if (!draw) throw new Error(`${page}: unknown held-out component ${kind}`);
      return draw(load("heldout") as Heldout);
    });
}
