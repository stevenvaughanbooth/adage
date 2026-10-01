// The page as a stack of scenes, the way Apple's product pages move. Nothing takes the scroll over:
// the browser scrolls, so it is always smooth. Each section holds the screen while the next one
// slides up over it, and the one underneath recedes. A section taller than the screen scrolls
// until its end is in view, then holds.
//
// The holding is CSS (position: sticky), and so is the receding: the browser runs it from the
// page's scroll position. This file measures, on load and resize, where each section holds (--pin:
// where its end meets the bottom of the screen) and the stretch of scroll over which the next one
// covers it (--r0 to --r1). As the page scrolls it only tells a section when it is fully covered
// ("scenecover"), so its animations can rest. It also keeps links and keyboard focus right, since a
// held section is not where the page would scroll to reach it. Phones keep plain scrolling.

const wide = matchMedia("(min-width: 861px)");
const still = matchMedia("(prefers-reduced-motion: reduce)");
// Where the browser cannot run the receding from the scroll (styles.css), this file sets --cover
// as the page scrolls instead.
const cssDriven = CSS.supports("animation-timeline: scroll()");
let scenes: HTMLElement[] = [];
let on = false;

/** Where the page scrolls to put `el` at the top of the screen, as if no section held. */
export function flowTop(el: Element): number {
  const scene = scenes.find((s) => s.contains(el));
  if (!on || !scene) return el.getBoundingClientRect().top + scrollY;
  // A held section's box is where it holds, not where it sits: count from the top of the stack.
  let top = scene.parentElement!.getBoundingClientRect().top + scrollY;
  for (const s of scenes) {
    if (s === scene) break;
    top += s.offsetHeight;
  }
  // Inside the section, layout offsets (neither holding nor receding moves them).
  for (let n = el as HTMLElement | null; n && n !== scene; n = n.offsetParent as HTMLElement | null) top += n.offsetTop;
  return top;
}

/** Scrolls so `el` (a section, or something in one) starts at the top of the screen. */
export function scrollToScene(el: Element, smooth = true) {
  scrollTo({ top: Math.max(0, flowTop(el)), behavior: smooth && !still.matches ? "smooth" : "instant" });
}

export function initScenes() {
  scenes = [...document.querySelectorAll<HTMLElement>("main > section")];
  if (scenes.length < 2) return;
  const root = document.documentElement;
  const cover = new Map<HTMLElement, number>();
  const covered = new Map<HTMLElement, boolean>();
  let raf = 0;

  // Where each section starts in the page, as if nothing held. Each holds where its end meets the
  // bottom of the screen (at the top, for one that fits), and the next covers it over the screen's
  // height of scroll before the next one starts.
  let r0: number[] = [], r1: number[] = [];
  const pin = () => {
    const vh = innerHeight;
    let top = scenes[0]!.parentElement!.getBoundingClientRect().top + scrollY;
    const tops = scenes.map((s) => { const t = top; top += s.offsetHeight; return t; });
    r1 = tops.slice(1);
    r0 = r1.map((t) => t - vh);
    scenes.forEach((s, i) => {
      s.style.setProperty("--pin", `${Math.min(0, vh - s.offsetHeight)}px`);
      if (i < r1.length) {
        s.style.setProperty("--r0", `${Math.round(r0[i]!)}px`);
        s.style.setProperty("--r1", `${Math.round(r1[i]!)}px`);
      }
    });
  };

  function update() {
    raf = 0;
    const y = scrollY;
    scenes.forEach((s, i) => {
      const c = i < r1.length ? Math.round(Math.min(1, Math.max(0, (y - r0[i]!) / (r1[i]! - r0[i]!))) * 1000) / 1000 : 0;
      if (!cssDriven && cover.get(s) !== c) {
        cover.set(s, c);
        s.style.setProperty("--cover", String(c));
        s.toggleAttribute("data-recede", c > 0);
      }
      const full = c >= 1;
      if (covered.get(s) !== full) {
        covered.set(s, full);
        s.toggleAttribute("data-covered", full);
        s.dispatchEvent(new CustomEvent("scenecover", { detail: full }));
      }
    });
    // Two deep under, a section cannot show at all: it stops being drawn.
    scenes.forEach((s, i) => s.toggleAttribute("data-gone", !!covered.get(s) && !!covered.get(scenes[i + 1]!)));
  }
  const frame = () => { if (on && !raf) raf = requestAnimationFrame(update); };

  function sync() {
    on = wide.matches;
    root.classList.toggle("scenes", on);
    if (on) {
      pin();
      update();
      return;
    }
    // Plain scrolling: nothing holds, nothing recedes, nothing is covered.
    cancelAnimationFrame(raf);
    raf = 0;
    for (const s of scenes) {
      for (const v of ["--pin", "--r0", "--r1", "--cover"]) s.style.removeProperty(v);
      for (const a of ["data-recede", "data-covered", "data-gone"]) s.removeAttribute(a);
      if (covered.get(s)) s.dispatchEvent(new CustomEvent("scenecover", { detail: false }));
    }
    cover.clear();
    covered.clear();
  }

  addEventListener("scroll", frame, { passive: true });
  addEventListener("resize", () => { if (on) { pin(); frame(); } });
  const ro = new ResizeObserver(() => { if (on) { pin(); frame(); } });
  for (const s of scenes) ro.observe(s);
  wide.addEventListener("change", sync);
  sync();

  // A link to somewhere on this page scrolls to where that place sits in the stack.
  document.addEventListener("click", (e) => {
    if (!on || e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target instanceof Element ? e.target.closest<HTMLAnchorElement>("a[href*='#']") : null;
    if (!a) return;
    const url = new URL(a.href, location.href);
    if (url.origin !== location.origin || url.pathname !== location.pathname || url.search !== location.search || url.hash.length < 2) return;
    const target = document.getElementById(decodeURIComponent(url.hash.slice(1)));
    if (!target || !scenes.some((s) => s.contains(target))) return;
    e.preventDefault();
    if (location.hash !== url.hash) history.pushState(null, "", url.hash);
    scrollToScene(target);
  });

  // Keyboard focus moving into a covered section brings that place back on screen.
  document.addEventListener("focusin", (e) => {
    if (!on || !(e.target instanceof Element)) return;
    const s = scenes.find((x) => x.contains(e.target as Element));
    if (s && covered.get(s)) scrollTo({ top: Math.max(0, flowTop(e.target) - innerHeight / 3), behavior: "instant" });
  });
}
