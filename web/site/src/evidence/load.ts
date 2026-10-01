// Redraws the held-out component from /evidence/heldout.json as the page opens. The build already
// put the same markup in the page; reading the file again keeps a page that was cached across an
// update honest, and lets the tests show every outcome from a fixture.

import { renderHeldout, renderHeldoutBrief, type Heldout } from "./heldout";

export async function loadHeldout(onDraw?: (root: HTMLElement) => void) {
  const full = document.querySelectorAll<HTMLElement>("[data-heldout='full']");
  const brief = document.querySelectorAll<HTMLElement>("[data-heldout='brief']");
  if (!full.length && !brief.length) return;
  let h: Heldout;
  try {
    const r = await fetch("/evidence/heldout.json", { cache: "no-cache" });
    if (!r.ok) return;
    h = (await r.json()) as Heldout;
    if (h?.schema !== "adage-heldout-evidence-1" || !h.study) return;
  } catch {
    return; // The built-in copy stays.
  }
  for (const el of full) {
    el.innerHTML = renderHeldout(h);
    el.dataset.loaded = h.status;
    onDraw?.(el);
  }
  for (const el of brief) {
    el.innerHTML = renderHeldoutBrief(h);
    el.dataset.loaded = h.status;
  }
}
