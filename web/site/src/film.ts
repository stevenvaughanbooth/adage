// The teaser film opens the page, then dissolves into the hero. It plays muted (browsers allow no
// sound without a gesture); the controls stay hidden until the pointer moves or a control is focused.
// The day or night cut follows the theme. If the film cannot load, the page simply starts at the hero.

const MEDIA = (import.meta.env.VITE_MEDIA_BASE as string | undefined) || "/media/";
const OFF = import.meta.env.VITE_FILM === "off";
// Each cut can also be named by its full URL (a Blob store keeps whatever name it was uploaded with).
const SRC = {
  dark: [(import.meta.env.VITE_FILM_DARK_URL as string | undefined) || MEDIA + "teaser-dark.mp4", "/media/poster-dark.jpg"],
  light: [(import.meta.env.VITE_FILM_DAY_URL as string | undefined) || MEDIA + "teaser-day.mp4", "/media/poster-day.jpg"],
} as const;

const SEEN = "adage.film.seen";

type Opts = { isLight(): boolean; reduced(): boolean; onOpen(): void; onClose(): void };

export function createFilm(o: Opts) {
  const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
  const el = $<HTMLDivElement>("film"), v = $<HTMLVideoElement>("film-video");
  const skip = $<HTMLButtonElement>("film-skip"), snd = $<HTMLButtonElement>("film-sound"), prog = $<HTMLElement>("film-prog"), play = $<HTMLButtonElement>("film-play");
  const root = document.documentElement;
  const outside = () => [...document.querySelectorAll<HTMLElement>("body > header, body > main, body > footer, .skiplink")];
  let open = false, back: Element | null = null, doneT = 0, idleT = 0;

  // The film downloads only while it is open: a theme switch later does not fetch the other cut.
  function source(keep: boolean) {
    if (!open) return;
    const [src, poster] = SRC[o.isLight() ? "light" : "dark"];
    if (v.dataset.src === src) return;
    const t = v.currentTime, playing = open && !v.paused;
    v.dataset.src = src; v.poster = poster; v.src = src;
    if (keep && t) v.addEventListener("loadedmetadata", () => { v.currentTime = t; }, { once: true });
    if (playing) run();
  }
  function syncSound() { snd.setAttribute("aria-pressed", String(!v.muted)); snd.textContent = v.muted ? "Sound on" : "Sound off"; }
  function run() {
    play.hidden = true;
    v.play()?.catch(() => { if (open) play.hidden = false; });
  }
  function show({ sound = false } = {}) {
    if (OFF) return;
    clearTimeout(doneT);
    back = document.activeElement;
    open = true; o.onOpen();
    // Motion-sensitive visitors and data savers choose to play it, so only its metadata loads first.
    const saveData = (navigator as Navigator & { connection?: { saveData?: boolean } }).connection?.saveData;
    const wait = (o.reduced() || saveData) && !sound;
    v.preload = wait ? "metadata" : "auto";
    source(false);
    el.hidden = false; el.inert = false; el.classList.add("live"); void el.offsetWidth; el.classList.remove("out", "awake");
    root.classList.add("film-open"); outside().forEach((e) => (e.inert = true));
    v.currentTime = 0; v.muted = !sound; syncSound(); prog.style.transform = "scaleX(0)";
    if (wait) play.hidden = false; else run();
    el.focus({ preventScroll: true });
  }
  function done(instant = false) {
    if (!open) return;
    open = false;
    el.inert = true; // out of the accessibility tree at once, not after the fade
    el.classList.add("out"); root.classList.remove("film-open"); outside().forEach((e) => (e.inert = false));
    o.onClose();
    // Land on the hero: focus goes back where it was, or to the headline.
    const hero = document.querySelector(".hero")!;
    const to = back instanceof HTMLElement && hero.contains(back) ? back : $<HTMLElement>("h");
    to.focus({ preventScroll: true });
    const hide = () => { v.pause(); el.hidden = true; };
    if (instant) hide(); else doneT = window.setTimeout(hide, 1100);
  }

  v.addEventListener("ended", () => done());
  // A film that cannot load closes at once; "Watch the teaser" tries again.
  v.addEventListener("error", () => { if (open) done(true); v.removeAttribute("src"); delete v.dataset.src; });
  v.addEventListener("timeupdate", () => { if (v.duration) prog.style.transform = `scaleX(${v.currentTime / v.duration})`; });
  skip.addEventListener("click", () => done());
  $("film-close").addEventListener("click", () => done());
  // The Play button hides itself: keep focus in the dialog so Escape still works.
  play.addEventListener("click", () => { v.muted = false; syncSound(); el.focus({ preventScroll: true }); run(); });
  snd.addEventListener("click", () => { v.muted = !v.muted; syncSound(); if (v.paused) run(); });
  v.addEventListener("click", () => { if (v.paused) run(); else v.pause(); });
  document.addEventListener("keydown", (e) => {
    if (!open) return;
    if (e.key === "Escape") { e.preventDefault(); done(); }
    if (e.key === " " && (e.target === v || e.target === el || e.target === document.body)) { e.preventDefault(); if (v.paused) run(); else v.pause(); }
  });
  // The controls: shown while the pointer moves, hidden again after a moment of stillness.
  const wake = () => {
    el.classList.add("awake"); clearTimeout(idleT);
    idleT = window.setTimeout(() => { if (!el.querySelector(".film-ui:hover")) el.classList.remove("awake"); }, 2200);
  };
  el.addEventListener("pointermove", wake);
  el.addEventListener("pointerdown", wake);
  el.addEventListener("pointerleave", () => { clearTimeout(idleT); el.classList.remove("awake"); });

  return {
    show,
    done,
    source,
    /** Open the page on the film, unless it is switched off for this build. */
    start() {
      if (OFF) { el.hidden = true; document.getElementById("teaser")?.remove(); return; }
      // The film opens a visit, once: not on a link to a section (/#evidence), not when coming from
      // another page of the site, and not again later in the same visit. "Watch the teaser" still plays it.
      let seen = false;
      try { seen = sessionStorage.getItem(SEEN) === "1"; sessionStorage.setItem(SEEN, "1"); } catch { /* no storage: play it */ }
      const fromSite = !!document.referrer && new URL(document.referrer).origin === location.origin;
      if (seen || fromSite || location.hash) { el.classList.add("live"); el.hidden = true; return; }
      show();
    },
  };
}
