// /how: the step-through walk from agent traces to reflexes. The chapters, the 3D stage and the
// sample data are the map's own (web/map/src/how, through the @how alias), so the site and
// `adage map` tell the same story. This file is the page around them: the chapter list, the
// caption with its figure, the transport, the URL and the keys. Without WebGL every step still reads.

import "@fontsource-variable/bricolage-grotesque/wght.css";
import "@fontsource-variable/hanken-grotesk/wght.css";
import "@fontsource-variable/red-hat-mono/wght.css";
import "./styles.css";
import { CHAPTERS } from "@how/chapters";
import { Stage, readPalette, webglAvailable } from "@how/engine";
import type { Built } from "@how/types";
import { initPage } from "./page";
import { initTheme } from "./theme";
import { initWilson } from "./wilson";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

let stage: Stage | null = null;
let built: Built | null = null;
let ch = 0, step = 0, playing = false, timer = 0;
// The tour plays by default while the walk is on screen, until the visitor takes the controls.
let userStopped = reduced, inView = false;

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
  const s = Object.assign(document.createElement("span"), { className: "chaps" + (c.kind === "flag" ? " flagtxt" : ""), textContent: c.kind === "flag" ? "engine flag" : c.status });
  b.append(n, t, s);
  b.addEventListener("click", () => { stop(); go(i, 0); });
  li.append(b); list.append(li);

  sel.append(new Option(`${pad(i)} ${c.title}`, c.id));

  const m = document.createElement("li");
  const a = document.createElement("a");
  a.href = `?ch=${c.id}#walk`;
  a.innerHTML = `<span class="mn"></span><b></b><span class="ms"></span>`;
  a.querySelector(".mn")!.textContent = pad(i);
  a.querySelector("b")!.textContent = c.title;
  a.querySelector(".ms")!.textContent = c.summary.split(". ")[0]!.replace(/\.$/, "") + ".";
  a.addEventListener("click", (e) => { e.preventDefault(); stop(); go(i, 0); $("walk").scrollIntoView({ behavior: reduced ? "auto" : "smooth" }); });
  m.append(a); map.append(m);
});
sel.addEventListener("change", () => { stop(); go(CHAPTERS.findIndex((c) => c.id === sel.value), 0); });

/* ------------------------------------------------------------ building and stepping */
function build() {
  const c = CHAPTERS[ch]!;
  const target = stage ?? textStage;
  target.clear();
  built = c.build(target, { graph: null, repo: false });
}

function go(i: number, s: number, fromUrl = false) {
  const nextCh = (i + CHAPTERS.length) % CHAPTERS.length;
  if (nextCh !== ch || !built) { ch = nextCh; build(); }
  const n = built!.steps.length;
  step = s >= 999 ? n - 1 : Math.min(Math.max(0, s), Math.max(0, n - 1));
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
  if (step < built!.steps.length - 1) go(ch, step + 1);
  else if (ch < CHAPTERS.length - 1) go(ch + 1, 0);
  else stop();
};
const prev = () => { if (step > 0) go(ch, step - 1); else if (ch > 0) go(ch - 1, 999); };

/** Long enough to read the caption: about 230 words a minute, at least 6 seconds. */
function dwell(): number {
  const s = built?.steps[step];
  const words = `${s?.title ?? ""} ${s?.body ?? ""}`.split(/\s+/).length;
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

  const meta = $("capmeta");
  meta.replaceChildren(
    Object.assign(document.createElement("span"), { textContent: `${pad(ch)} ${c.title}` }),
    Object.assign(document.createElement("span"), { textContent: `· step ${step + 1} of ${built!.steps.length}` }),
    Object.assign(document.createElement("span"), { textContent: `· ${built!.source}`, className: c.kind === "flag" ? "flagtxt" : "" }),
  );
  $("captitle").textContent = s?.title ?? "";
  $("capbody").textContent = s?.body ?? "";
  $("capchips").replaceChildren(...(s?.chips ?? []).map((chip) => {
    const el = document.createElement("span");
    if (chip.ok) el.append(Object.assign(document.createElement("b"), { textContent: "✓ " }));
    el.append(chip.text);
    return el;
  }));
  drawFigure();

  const ticks = $("ticks");
  ticks.replaceChildren(...built!.steps.map((st, i) => {
    const li = document.createElement("li");
    const b = document.createElement("button");
    b.type = "button"; b.title = `${i + 1} · ${st.title}`;
    b.setAttribute("aria-label", `Step ${i + 1}: ${st.title}`);
    if (i === step) b.setAttribute("aria-current", "step");
    b.className = i < step ? "done" : "";
    b.append(document.createElement("i"));
    b.addEventListener("click", () => { stop(); go(ch, i); });
    li.append(b);
    return li;
  }));
  $("count").textContent = `${step + 1} / ${built!.steps.length}`;
  $("stage").setAttribute("aria-label", `${c.title}: step ${step + 1} of ${built!.steps.length}`);

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
  if (!s?.figure) return;
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
  if (e.key === " " && t.closest("#walk") && !t.closest("button")) { e.preventDefault(); userStopped = playing; setPlaying(!playing); }
});
// Play while the walk is on screen (unless the visitor stopped it); pause when it scrolls away.
new IntersectionObserver((es) => {
  inView = !!es[0]?.isIntersecting;
  if (!inView && playing) setPlaying(false);
  else if (inView && !userStopped && !playing) setPlaying(true);
}, { threshold: 0.35 }).observe($("walk"));
document.addEventListener("visibilitychange", () => {
  if (document.hidden) { if (playing) setPlaying(false); }
  else if (inView && !userStopped) setPlaying(true);
});

initTheme();
startStage();
const params = new URLSearchParams(location.search);
const start = Math.max(0, CHAPTERS.findIndex((c) => c.id === params.get("ch")));
built = null;
go(start, Number(params.get("step") ?? 0) || 0, true);
initWilson();
initPage();
