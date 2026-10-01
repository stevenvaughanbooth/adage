// The math on /how is a carousel: one block at a time, side by side. The track scrolls on its own
// (swipe, trackpad, keys); this file keeps the pills, the counter and the arrows in step with it,
// sizes the track to the block in view, and opens the block a #m-… link names.
// Without scripts the track still scrolls and snaps.

import { scrollToScene } from "./scenes";

export function initMath() {
  const found = document.getElementById("mtrack");
  if (!found) return null;
  const track: HTMLElement = found;
  const section = document.getElementById("math")!;
  const cards = [...track.querySelectorAll<HTMLElement>(".mblock")];
  const pills = [...document.querySelectorAll<HTMLAnchorElement>(".mathtoc a")];
  const prev = document.getElementById("mprev") as HTMLButtonElement;
  const next = document.getElementById("mnext") as HTMLButtonElement;
  const count = document.getElementById("mcount")!;
  const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const pad = (n: number) => String(n).padStart(2, "0");
  let at = -1;

  cards.forEach((c, n) => c.setAttribute("aria-label", `${n + 1} of ${cards.length}`));

  /** The block in view: the one whose left edge is nearest the track's scroll position. */
  const nearest = () => {
    let best = 0;
    cards.forEach((c, n) => { if (Math.abs(c.offsetLeft - track.scrollLeft) < Math.abs(cards[best]!.offsetLeft - track.scrollLeft)) best = n; });
    return best;
  };
  // The track is as tall as the block in view, so a short block leaves no empty screen below it.
  // Only at rest: a layout change mid-scroll makes the browser re-snap and cut the scroll short.
  let moving = false;
  const fit = () => { if (!moving && at >= 0) track.style.height = `${cards[at]!.offsetHeight}px`; };

  /** On phones the pills are one sideways row: keep the current one in the middle of it. */
  function centre(smooth: boolean) {
    const pill = pills[at], row = pill?.parentElement;
    if (pill && row && row.scrollWidth > row.clientWidth) row.scrollTo({ left: pill.offsetLeft - (row.clientWidth - pill.offsetWidth) / 2, behavior: smooth ? "smooth" : "instant" });
  }

  function mark(n: number) {
    if (n === at) return;
    at = n;
    cards.forEach((c, i) => c.classList.toggle("on", i === n));
    pills.forEach((p, i) => (i === n ? p.setAttribute("aria-current", "true") : p.removeAttribute("aria-current")));
    centre(!reduced);
    count.textContent = `${pad(n + 1)} / ${pad(cards.length)}`;
    prev.disabled = n === 0;
    next.disabled = n === cards.length - 1;
    fit();
  }

  function show(n: number, smooth = !reduced) {
    n = Math.min(Math.max(0, n), cards.length - 1);
    if (smooth && Math.abs(track.scrollLeft - cards[n]!.offsetLeft) > 1) moving = true;
    track.scrollTo({ left: cards[n]!.offsetLeft, behavior: smooth ? "smooth" : "instant" });
    mark(n);
    history.replaceState(null, "", `${location.pathname}${location.search}#${cards[n]!.id}`);
  }

  let raf = 0, rest = 0;
  const settle = () => { clearTimeout(rest); moving = false; mark(nearest()); fit(); };
  track.addEventListener("scroll", () => {
    moving = true;
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => mark(nearest()));
    // scrollend where the browser has it; a quiet moment where it does not.
    clearTimeout(rest);
    rest = window.setTimeout(settle, 180);
  }, { passive: true });
  track.addEventListener("scrollend", settle);

  prev.addEventListener("click", () => show(at - 1));
  next.addEventListener("click", () => show(at + 1));
  track.addEventListener("keydown", (e) => {
    if (e.target !== track || e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.key === "ArrowRight") { e.preventDefault(); show(at + 1); }
    if (e.key === "ArrowLeft") { e.preventDefault(); show(at - 1); }
    if (e.key === "Home") { e.preventDefault(); show(0); }
    if (e.key === "End") { e.preventDefault(); show(cards.length - 1); }
  });
  pills.forEach((p, n) => p.addEventListener("click", (e) => { e.preventDefault(); show(n); }));

  // A new width moves every block: keep the one in view in place. Heights follow the block's own size.
  let width = track.clientWidth;
  new ResizeObserver(() => {
    if (at < 0 || track.clientWidth === width) return;
    width = track.clientWidth;
    track.scrollLeft = cards[at]!.offsetLeft;
  }).observe(track);
  const sizes = new ResizeObserver(fit);
  cards.forEach((c) => sizes.observe(c));

  // The browser's own jump to a #m-… fragment lands on the math section's top, not the block's:
  // each block's scroll margin is the distance from the section's top to the track.
  const margin = () => track.style.setProperty("--mtop", `${track.getBoundingClientRect().top - section.getBoundingClientRect().top}px`);
  const reflow = new ResizeObserver(margin);
  reflow.observe(section);
  for (const el of section.querySelectorAll(".wrap > :not(.mtrack)")) reflow.observe(el);
  void document.fonts?.ready.then(margin);
  margin();

  /** A #m-… link opens its block and brings the math on screen. */
  function fromHash(smooth: boolean) {
    const n = cards.findIndex((c) => `#${c.id}` === location.hash);
    if (n < 0) return false;
    track.scrollLeft = cards[n]!.offsetLeft;
    mark(n);
    centre(false);
    scrollToScene(section, smooth && !reduced);
    return true;
  }
  addEventListener("hashchange", () => fromHash(true));
  if (fromHash(false)) {
    // Fonts arriving reflow the page above the math after the jump: once they and the page have
    // loaded, a visitor still at the math's top is put exactly on it.
    const align = () => requestAnimationFrame(() => {
      if (Math.abs(section.getBoundingClientRect().top) < 80) scrollToScene(section, false);
    });
    void document.fonts?.ready.then(align);
    addEventListener("load", align, { once: true });
  } else mark(nearest());

  return {
    /** Opens a block by id ("evidence" for #m-evidence) and brings the math on screen. */
    open(id: string) {
      history.replaceState(null, "", `${location.pathname}${location.search}#m-${id}`);
      fromHash(true);
    },
    label: (id: string) => pills[cards.findIndex((c) => c.id === `m-${id}`)]?.textContent ?? id,
  };
}
