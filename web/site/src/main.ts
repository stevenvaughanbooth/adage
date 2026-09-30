import "@fontsource-variable/bricolage-grotesque/wght.css";
import "@fontsource-variable/hanken-grotesk/wght.css";
import "@fontsource-variable/red-hat-mono/wght.css";
import "./styles.css";
import { TABS, SCENE_OF } from "./tabs";
import { createFilm } from "./film";
import { initPage } from "./page";
import { initTheme, isLight } from "./theme";

type Scene = {
  go(id: string): void;
  intro(): void;
  theme(light: boolean): void;
  hold(held: boolean): void;
  motion(reduced: boolean): void;
};

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const mqReduce = matchMedia("(prefers-reduced-motion: reduce)");
let reduced = mqReduce.matches;
let scene: Scene | null = null;

/* ------------------------------------------------------------ the tabs and the tour */
const DWELL = 12000;
let tab = 0, auto = !reduced, timer = 0, heroOn = true, filmOn = false, hovering = false;
const tabsEl = $("tabs"), pauseBtn = $<HTMLButtonElement>("pause");
TABS.forEach((t, i) => {
  const b = document.createElement("button");
  b.className = "tab"; b.type = "button"; b.id = "tab-" + t.id;
  b.setAttribute("role", "tab"); b.setAttribute("aria-controls", "panel");
  const dot = document.createElement("i"); dot.style.background = t.dot;
  const bar = document.createElement("span"); bar.className = "bar";
  b.append(dot, t.name, bar);
  b.addEventListener("click", () => { stopAuto(); select(i); });
  b.addEventListener("keydown", (e) => {
    const n = ({ ArrowRight: (tab + 1) % TABS.length, ArrowLeft: (tab + TABS.length - 1) % TABS.length, Home: 0, End: TABS.length - 1 } as Record<string, number>)[e.key];
    if (n === undefined) return;
    e.preventDefault(); stopAuto(); select(n); (tabsEl.children[n] as HTMLElement).focus();
  });
  tabsEl.appendChild(b);
});
function schedule() {
  clearTimeout(timer);
  if (auto && heroOn && !filmOn && !hovering) timer = window.setTimeout(() => select((tab + 1) % TABS.length), DWELL);
}
function select(i: number) {
  tab = i;
  const t = TABS[i]!;
  [...tabsEl.children].forEach((b, k) => {
    b.setAttribute("aria-selected", String(k === i)); (b as HTMLElement).tabIndex = k === i ? 0 : -1;
    const bar = b.querySelector<HTMLElement>(".bar")!; bar.classList.remove("run"); void bar.offsetWidth;
    if (k === i && auto && !filmOn) { (b as HTMLElement).style.setProperty("--dwell", DWELL + "ms"); bar.classList.add("run"); }
  });
  $("panel").setAttribute("aria-labelledby", "tab-" + t.id);
  $("p-title").textContent = t.title; $("p-body").textContent = t.body;
  $("p-facts").replaceChildren(...t.facts.map((f) => Object.assign(document.createElement("span"), { textContent: f })));
  schedule();
  scene?.go(SCENE_OF[t.id] ?? "overview");
}
function syncPause() {
  pauseBtn.setAttribute("aria-pressed", String(!auto));
  pauseBtn.textContent = auto ? "Pause tour" : "Play tour";
}
function stopAuto() { if (!auto) return; auto = false; syncPause(); select(tab); }
pauseBtn.addEventListener("click", () => { auto = !auto; syncPause(); select(auto ? (tab + 1) % TABS.length : tab); });
// Using the tabs or the panel with the keyboard stops the tour; Play tour resumes it.
document.querySelector(".copy")!.addEventListener("focusin", (e) => { const t = e.target as HTMLElement; if (t !== pauseBtn && t.id !== "h" && t.closest(".tabrow, .panel")) stopAuto(); });
// Reading the panel holds the tour on this part; it moves on when the pointer leaves.
$("panel").addEventListener("pointerenter", () => { hovering = true; schedule(); });
$("panel").addEventListener("pointerleave", () => { hovering = false; if (auto) select(tab); });
mqReduce.addEventListener("change", () => { reduced = mqReduce.matches; if (reduced) stopAuto(); scene?.motion(reduced); });
new IntersectionObserver((es) => { heroOn = !!es[0]?.isIntersecting; schedule(); }).observe(document.querySelector(".hero")!);

/* ------------------------------------------------------------ the film: first, then the hero */
const film = createFilm({
  isLight,
  reduced: () => reduced,
  onOpen() { filmOn = true; clearTimeout(timer); scene?.hold(true); },
  onClose() { filmOn = false; scene?.hold(false); select(tab); },
});
$("teaser").addEventListener("click", (e) => { e.preventDefault(); select(0); film.show({ sound: true }); });

/* ------------------------------------------------------------ the scene, loaded after the words */
function fallback() {
  $("gl")?.remove();
  if (!document.querySelector(".nogl")) {
    const d = document.createElement("div"); d.className = "nogl";
    d.textContent = "The 3D view needs WebGL. Each part is described in the panel.";
    $("stagewrap").appendChild(d);
  }
}
// three.js needs WebGL 2. The probe's context is released at once.
function webgl() {
  try {
    const gl = document.createElement("canvas").getContext("webgl2");
    gl?.getExtension("WEBGL_lose_context")?.loseContext();
    return !!gl;
  } catch { return false; }
}

syncPause();
initTheme((light) => { scene?.theme(light); film.source(true); });
select(0);
film.start();
initPage();

if (webgl()) {
  import("./hero/scene.js")
    .then(({ build }) => {
      scene = build({ canvas: $<HTMLCanvasElement>("gl"), labels: $("labels"), onLost: fallback, light: isLight(), held: filmOn, reduced }) as Scene;
      select(tab);
    })
    .catch((e) => { console.error(e); fallback(); });
} else {
  fallback();
}
