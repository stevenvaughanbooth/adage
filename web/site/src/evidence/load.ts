// Redraws the held-out components from /evidence/heldout.json as the page opens. The build already
// put the same markup in the page; reading the file again keeps a page that was cached across an
// update honest, and lets the tests show every outcome from a fixture.

import { HELDOUT_KINDS, SCHEMA, type Heldout } from "./heldout";

export async function loadHeldout(onDraw?: (root: HTMLElement) => void) {
  const els = [...document.querySelectorAll<HTMLElement>("[data-heldout]")];
  if (!els.length) return;
  let h: Heldout;
  try {
    const r = await fetch("/evidence/heldout.json", { cache: "no-cache" });
    if (!r.ok) return;
    h = (await r.json()) as Heldout;
    if (h?.schema !== SCHEMA || !h.study) return;
  } catch {
    return; // The built-in copy stays.
  }
  for (const el of els) {
    const draw = HELDOUT_KINDS[el.dataset.heldout ?? ""];
    if (!draw) continue;
    el.innerHTML = draw(h);
    el.dataset.loaded = h.status;
    onDraw?.(el);
  }
}
