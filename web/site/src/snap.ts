// The page moves a section at a time. CSS scroll snapping does this on phones (and without
// scripts), but on a desktop Chrome treats a short wheel turn as "not far enough" and pulls the
// page back to the section it left. So on wide screens the snapping is directional instead: when
// scrolling stops between two sections, the page finishes the move in the direction it was going.
// A section taller than the screen scrolls freely until its end, like any page. Only a scroll
// the visitor makes by hand (wheel, touch, keys, the scrollbar) snaps; a scroll a link, a script
// or the browser's find-in-page makes lands where it was sent.

const wide = matchMedia("(min-width: 861px)");
const reduced = matchMedia("(prefers-reduced-motion: reduce)");
const SCROLL_KEYS = new Set(["PageDown", "PageUp", " ", "ArrowDown", "ArrowUp", "Home", "End"]);
// How long after the visitor's last scrolling input a scroll still counts as theirs.
const BY_HAND_MS = 1200;

export function initSnap() {
  const parts = [...document.querySelectorAll<HTMLElement>("main > section, body > .foot")];
  if (parts.length < 2) return;
  const root = document.documentElement;
  const sync = () => root.classList.toggle("snapjs", wide.matches);
  wide.addEventListener("change", sync);
  sync();

  let rest = scrollY, timer = 0, gliding = false, byHand = -Infinity;

  /** Section tops on the page; the first is the page's own top, above any header. */
  const tops = () => parts.map((p, i) => (i === 0 ? 0 : Math.round(p.getBoundingClientRect().top + scrollY)));

  function settle() {
    clearTimeout(timer);
    const y = scrollY, vh = innerHeight, dir = Math.sign(y - rest);
    const max = root.scrollHeight - vh;
    rest = y;
    if (!wide.matches || gliding || !dir || performance.now() - byHand > BY_HAND_MS) { gliding = false; return; }
    const t = tops();
    // The section the top of the screen is in, and where the next one starts.
    let i = t.length - 1;
    while (i > 0 && t[i]! > y + 1) i--;
    const top = t[i]!, end = Math.min(t[i + 1] ?? max + vh, max + vh);
    if (Math.abs(y - top) <= 1 || y >= max - 1) return;
    if (y + vh <= end + 1) return; // inside a section taller than the screen: read freely
    // Between two sections: finish the move the way the visitor was going.
    const to = Math.round(dir > 0 ? Math.min(end, max) : end - top > vh ? end - vh : top);
    // Never move keyboard focus off the screen.
    const f = document.activeElement;
    if (f && f !== document.body && f.matches(":focus-visible")) {
      const r = f.getBoundingClientRect(), shift = to - y;
      if (r.top - shift < 0 || r.bottom - shift > vh) return;
    }
    if (Math.abs(to - y) <= 1) return;
    gliding = true;
    rest = to;
    scrollTo({ top: to, behavior: reduced.matches ? "instant" : "smooth" });
  }

  // scrollend marks the true end of a scroll, smooth ones included. Where there is none, a quiet
  // moment stands in for it; it must outlast a slow frame, or it would cut a smooth scroll short.
  if ("onscrollend" in window) addEventListener("scrollend", settle);
  else addEventListener("scroll", () => {
    clearTimeout(timer);
    timer = window.setTimeout(settle, 250);
  }, { passive: true });
  // The visitor's own scrolling: it is what snaps, and it takes the page back from a glide. Heard
  // while capturing, because the 3D stage keeps a plain wheel from bubbling (it scrolls the page).
  const hand = () => { byHand = performance.now(); gliding = false; };
  addEventListener("wheel", hand, { passive: true, capture: true });
  addEventListener("touchmove", hand, { passive: true, capture: true });
  addEventListener("keydown", (e) => { if (SCROLL_KEYS.has(e.key)) hand(); }, { capture: true });
  // A press on the page's own scrollbar lands on the root element.
  addEventListener("pointerdown", (e) => { if (e.target === root) hand(); }, { capture: true });
}
