// /how: the step-through walk from agent traces to reflexes. The chapters, the 3D stage and the
// sample data are the map's own (web/map/src/how, through the @how alias), so the site and
// `adage map` tell the same story. This file is the page around them: the chapter list, the quick
// and full tours, the caption (the plain words first, the precise account under "More detail", a
// link into the math), the transport, the URL and the keys. Without WebGL every step still reads.

import "@fontsource-variable/bricolage-grotesque/wght.css";
import "@fontsource-variable/hanken-grotesk/wght.css";
import "@fontsource-variable/red-hat-mono/wght.css";
import "./styles.css";
import { CHAPTERS, chapterIndex } from "@how/chapters";
import { chipMark, chipText } from "@how/chip";
import { Stage, readPalette, webglAvailable } from "@how/engine";
import type { Built } from "@how/types";
import { initMath } from "./math";
import { initPage } from "./page";
import { scrollToScene } from "./scenes";
import { initTerms, termify, termifyTree } from "./terms";
import { initTheme } from "./theme";
import { initUplift } from "./uplift";
import { initWilson } from "./wilson";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

let stage: Stage | null = null;
let built: Built | null = null;
let ch = 0, step = 0, playing = false, timer = 0;
// The tour plays by default while the walk is on screen, until the visitor takes the controls.
let userStopped = reduced, inView = false;

/** A visitor's choices, remembered on this device when storage allows. */
const pref = {
  get: (k: string) => { try { return localStorage.getItem(`adage.how.${k}`); } catch { return null; } },
  set: (k: string, v: string) => { try { localStorage.setItem(`adage.how.${k}`, v); } catch { /* not remembered */ } },
};
// The quick tour stops at each chapter's key steps; the full tour at every step.
let tour: "quick" | "full" = pref.get("tour") === "full" ? "full" : "quick";
// "More detail" stays open from step to step once a visitor opens it.
let detail = pref.get("detail") === "open";
const FIRST = -1, LAST = 999;

/** Chapters build on this when there is no WebGL: they get their words, and nothing draws. */
const textStage = {
  dot() {}, link() {}, label: () => document.createElement("div"), clear() {}, to() {}, palette: () => readPalette(),
} as unknown as Stage;

/* ------------------------------------------------------------ the chapter lists */
const pad = (i: number) => String(i + 1).padStart(2, "0");
const list = $("chaplist"), sel = $<HTMLSelectElement>("chapsel"), map = $("stagemap");
CHAPTERS.forEach((c, i) => {
  const li = document.createElement("li");
  const b = document.createElement("button");
  b.type = "button"; b.className = "chap";
  const n = Object.assign(document.createElement("span"), { className: "chapn", textContent: pad(i) });
  const t = Object.assign(document.createElement("span"), { className: "chapt", textContent: c.title });
  const s = Object.assign(document.createElement("span"), { className: "chaps" + (c.kind === "flag" ? " flagtxt" : ""), textContent: c.kind === "flag" ? "engine flag" : c.technical });
  b.append(n, t, s);
  b.addEventListener("click", () => { stop(); go(i, FIRST); });
  li.append(b); list.append(li);

  sel.append(new Option(`${pad(i)} ${c.title}`, c.id));

  const m = document.createElement("li");
  const a = document.createElement("a");
  a.href = `?ch=${c.id}#walk`;
  a.innerHTML = `<span class="mn"></span><b></b><span class="ms"></span>`;
  a.querySelector(".mn")!.textContent = pad(i);
  a.querySelector("b")!.textContent = c.title;
  a.querySelector(".ms")!.textContent = c.summary.split(". ")[0]!.replace(/\.$/, "") + ".";
  a.addEventListener("click", (e) => { e.preventDefault(); openChapter(i); });
  m.append(a); map.append(m);
});
sel.addEventListener("change", () => { stop(); go(CHAPTERS.findIndex((c) => c.id === sel.value), FIRST); });

/** Opens a chapter at its first stop and brings the walk on screen. */
function openChapter(i: number) {
  stop();
  go(i, FIRST);
  scrollToScene($("walk"));
}

/* ------------------------------------------------------------ building and stepping */
function build() {
  const c = CHAPTERS[ch]!;
  const target = stage ?? textStage;
  target.clear();
  built = c.build(target, { graph: null, repo: false });
}

/** The steps the tour stops at in this chapter: the key steps on the quick tour (and the step on
 *  screen, so a link to any step still lands), every step on the full tour. */
function stops(at = step): number[] {
  const all = built!.steps.map((_, i) => i);
  if (tour === "full") return all;
  const keys = all.filter((i) => built!.steps[i]!.key || i === at);
  return keys.length ? keys : all;
}

function go(i: number, s: number, fromUrl = false) {
  const nextCh = (i + CHAPTERS.length) % CHAPTERS.length;
  if (nextCh !== ch || !built) { ch = nextCh; build(); }
  const n = built!.steps.length;
  const st = stops(-1);
  step = s === FIRST ? st[0]! : s >= LAST ? st[st.length - 1]! : Math.min(Math.max(0, s), Math.max(0, n - 1));
  render();
  if (!fromUrl) {
    const u = new URL(location.href);
    u.searchParams.set("ch", CHAPTERS[ch]!.id);
    u.searchParams.set("step", String(step));
    history.replaceState(null, "", u);
  }
  schedule();
}
const next = () => {
  const st = stops(), i = st.indexOf(step);
  if (i < st.length - 1) go(ch, st[i + 1]!);
  else if (ch < CHAPTERS.length - 1) go(ch + 1, FIRST);
  else stop();
};
const prev = () => {
  const st = stops(), i = st.indexOf(step);
  if (i > 0) go(ch, st[i - 1]!);
  else if (ch > 0) go(ch - 1, LAST);
};

/** Long enough to read the caption: about 230 words a minute, at least 6 seconds. */
function dwell(): number {
  const s = built?.steps[step];
  const words = `${s?.title ?? ""} ${s?.plain ?? ""} ${detail ? s?.body ?? "" : ""}`.split(/\s+/).length;
  return Math.max(6000, words * 260 + 1500);
}
function schedule() {
  clearTimeout(timer);
  if (playing) timer = window.setTimeout(next, dwell());
}
function setPlaying(p: boolean) {
  playing = p;
  $("play").setAttribute("aria-label", p ? "Pause" : "Play");
  $("playicon").setAttribute("d", p ? "M3 2h4v12H3zM9 2h4v12H9z" : "M4 2l10 6-10 6z");
  schedule();
}
/** The visitor took the controls: the tour stays stopped until they press play. */
const stop = () => { userStopped = true; setPlaying(false); };

/* ------------------------------------------------------------ drawing the page for a step */
function render() {
  const c = CHAPTERS[ch]!;
  const s = built!.steps[step];
  [...list.querySelectorAll(".chap")].forEach((b, i) => (i === ch ? b.setAttribute("aria-current", "step") : b.removeAttribute("aria-current")));
  sel.value = c.id;

  const st = stops(), at = st.indexOf(step);
  const meta = $("capmeta");
  meta.replaceChildren(
    Object.assign(document.createElement("span"), { textContent: `${pad(ch)} ${c.title}` }),
    Object.assign(document.createElement("span"), { textContent: `· step ${at + 1} of ${st.length}` }),
    Object.assign(document.createElement("span"), { textContent: `· ${built!.source}`, className: c.kind === "flag" ? "flagtxt" : "" }),
  );
  $("captitle").textContent = s?.title ?? "";
  // The first mention of each hard word gets its definition, in the plain words or the detail.
  const seen = new Set<string>();
  termify($("capplain"), s?.plain ?? "", seen);
  termify($("capbody"), s?.body ?? "", seen);
  const ml = $<HTMLAnchorElement>("capmath");
  ml.hidden = !s?.math || !math;
  if (s?.math && math) {
    ml.href = `#m-${s.math}`;
    ml.dataset.math = s.math;
    ml.textContent = `The math: ${math.label(s.math)} →`;
  }
  $("capchips").replaceChildren(...(s?.chips ?? []).map((chip) => {
    const el = document.createElement("span");
    const mark = chipMark(chip);
    if (mark) el.append(Object.assign(document.createElement("b"), { textContent: `${mark} `, className: chip.ok ? "" : "fail" }));
    el.append(chipText(chip));
    return el;
  }));
  drawFigure();

  const ticks = $("ticks");
  ticks.replaceChildren(...st.map((i, n) => {
    const li = document.createElement("li");
    const b = document.createElement("button");
    const t = built!.steps[i]!.title;
    b.type = "button"; b.title = `${n + 1} · ${t}`;
    b.setAttribute("aria-label", `Step ${n + 1}: ${t}`);
    if (i === step) b.setAttribute("aria-current", "step");
    b.className = i < step ? "done" : "";
    b.append(document.createElement("i"));
    b.addEventListener("click", () => { stop(); go(ch, i); });
    li.append(b);
    return li;
  }));
  $("count").textContent = `${at + 1} / ${st.length}`;
  $("stage").setAttribute("aria-label", `${c.title}: step ${at + 1} of ${st.length}`);

  const pal = stage?.palette() ?? readPalette();
  const legend = built!.legend ?? [];
  const lg = $("howlegend");
  lg.hidden = legend.length === 0;
  lg.replaceChildren(...legend.map((l) => {
    const li = document.createElement("li");
    const dot = document.createElement("i");
    dot.style.background = l.role === "hue" ? `#${pal.hue(l.hue ?? 0).getHexString()}` : pal.css(l.role);
    li.append(dot, l.label);
    return li;
  }));

  if (stage && s) stage.to(s.state);
}

function drawFigure() {
  const c = $<HTMLCanvasElement>("capfig");
  const s = built?.steps[step];
  c.hidden = !s?.figure;
  if (!s?.figure || !detail) return;
  c.setAttribute("aria-label", `Figure: ${s.title}`);
  const dpr = Math.min(2, window.devicePixelRatio || 1);
  const w = c.clientWidth, h = c.clientHeight;
  c.width = w * dpr; c.height = h * dpr;
  const g = c.getContext("2d");
  if (!g) return;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  s.figure(g, w, h, stage?.palette() ?? readPalette());
}

/* ------------------------------------------------------------ the stage */
function startStage() {
  if (!webglAvailable()) { $("nogl").hidden = false; $("howgl").remove(); return; }
  try {
    stage = new Stage($<HTMLCanvasElement>("howgl"), $("howlayer"));
  } catch {
    $("nogl").hidden = false;
    return;
  }
  stage.onTheme(() => { drawFigure(); render(); });
  // The caption covers the stage's left side on wide screens: the scene centres in the rest.
  const cap = $("caption");
  const inset = () => stage?.setInset(getComputedStyle(cap).position === "absolute" ? cap.offsetLeft + cap.offsetWidth : 0);
  new ResizeObserver(inset).observe(cap);
  inset();
}

// The page moves a section at a time: a plain wheel over the scene scrolls the page, and a pinch
// (which arrives as ctrl + wheel) still zooms it. The engine's own wheel handler never sees the rest.
$("stage").addEventListener("wheel", (e) => { if (!e.ctrlKey) e.stopPropagation(); }, { capture: true, passive: true });

/* ------------------------------------------------------------ detail, tours, links */
function setDetail(open: boolean, remember = true) {
  detail = open;
  if (remember) pref.set("detail", open ? "open" : "closed");
  $("capmore").hidden = !open;
  const b = $("capmorebtn");
  b.setAttribute("aria-expanded", String(open));
  b.querySelector("span")!.textContent = open ? "Less detail" : "More detail";
  drawFigure();
  schedule();
}
$("capmorebtn").addEventListener("click", () => setDetail(!detail));

function setTour(t: "quick" | "full") {
  tour = t;
  pref.set("tour", t);
  document.querySelectorAll<HTMLButtonElement>("[data-tour]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.tour === t)));
  if (built) render();
}
document.querySelectorAll<HTMLButtonElement>("[data-tour]").forEach((b) => b.addEventListener("click", () => setTour(b.dataset.tour === "full" ? "full" : "quick")));

// "The math →" opens its block in the carousel below; the math's "See it in the walkthrough" comes back.
$("capmath").addEventListener("click", (e) => {
  const id = (e.currentTarget as HTMLElement).dataset.math;
  if (!id || !math) return;
  e.preventDefault();
  stop();
  math.open(id);
});
document.querySelectorAll<HTMLAnchorElement>(".mback a[data-ch]").forEach((a) =>
  a.addEventListener("click", (e) => { e.preventDefault(); openChapter(chapterIndex(a.dataset.ch)); }),
);

/* ------------------------------------------------------------ controls */
$("prev").addEventListener("click", () => { stop(); prev(); });
$("next").addEventListener("click", () => { stop(); next(); });
$("play").addEventListener("click", () => { userStopped = playing; setPlaying(!playing); });
document.addEventListener("keydown", (e) => {
  const t = e.target as HTMLElement;
  if (e.altKey || e.ctrlKey || e.metaKey || t.closest("input, select, textarea")) return;
  const inWalk = !!t.closest("#walk") || t === document.body;
  if (e.key === "ArrowRight" && inWalk) { e.preventDefault(); stop(); next(); }
  if (e.key === "ArrowLeft" && inWalk) { e.preventDefault(); stop(); prev(); }
  if (e.key === " " && t.closest("#walk") && !t.closest("button, .gloss")) { e.preventDefault(); userStopped = playing; setPlaying(!playing); }
  if ((e.key === "d" || e.key === "D") && inWalk) { e.preventDefault(); setDetail(!detail); }
});
// Play while the walk is on screen (unless the visitor stopped it); pause when it scrolls away or
// the next scene has slid over it.
let seen = false, covered = false;
const watch = () => {
  inView = seen && !covered;
  if (!inView && playing) setPlaying(false);
  else if (inView && !userStopped && !playing) setPlaying(true);
};
new IntersectionObserver((es) => { seen = !!es[0]?.isIntersecting; watch(); }, { threshold: 0.35 }).observe($("walk"));
$("walk").addEventListener("scenecover", (e) => { covered = (e as CustomEvent<boolean>).detail; watch(); });
document.addEventListener("visibilitychange", () => {
  if (document.hidden) { if (playing) setPlaying(false); }
  else if (inView && !userStopped) setPlaying(true);
});

initTheme();
initTerms();
const math = initMath();
setTour(tour);
setDetail(detail, false);
startStage();
const params = new URLSearchParams(location.search);
const start = chapterIndex(params.get("ch"));
built = null;
go(start, params.has("step") ? Number(params.get("step")) || 0 : FIRST, true);
initWilson();
initUplift();
// Hard words in the intro and the math get their definitions too.
termifyTree(document.querySelector(".howintro .lede")!);
document.querySelectorAll(".mblock").forEach((b) => termifyTree(b));
initPage();
