// /evidence: the results at a glance, what we tested and how, the held-out result in both modes, the
// Claude Code mod, the limits. The
// numbers are in the page from the build (public/evidence/*.json, src/evidence/tokens.ts); this file
// adds the dots of the running example, the dotted words with their definitions (the shared
// glossary), and redraws the held-out result from its data file.

import "@fontsource-variable/bricolage-grotesque/wght.css";
import "@fontsource-variable/hanken-grotesk/wght.css";
import "@fontsource-variable/red-hat-mono/wght.css";
import "./styles.css";
import { loadHeldout } from "./evidence/load";
import { initPage } from "./page";
import { allTerms, initTerms, termifyTree } from "./terms";
import { initTheme } from "./theme";

/** One dot per model call, rounded to the nearest whole call. */
export function drawDots(root: ParentNode = document) {
  root.querySelectorAll<HTMLElement>("[data-dots]").forEach((el) => {
    const n = Math.round(Number(el.dataset.dots));
    if (!Number.isFinite(n) || n < 0 || n > 200) return;
    el.replaceChildren(...Array.from({ length: n }, (_, i) => {
      const d = document.createElement("i");
      d.style.setProperty("--i", String(i));
      return d;
    }));
  });
}

// Words a newcomer may not know, marked once per section; the held-out block is marked as it draws.
const TERMED = "main > section .lede, .ev-question, .ev-ex-lead, .ev-rules p, .ev-callout p, #tested .ev-more-body > p, .ev-list li, .ev-repro-grid p";
function terms(root: ParentNode = document) {
  const seen = new Map<Element, Set<string>>();
  root.querySelectorAll(TERMED).forEach((el) => {
    const section = el.closest("section") ?? document.body;
    if (!seen.has(section)) seen.set(section, new Set());
    termifyTree(el, seen.get(section), allTerms);
  });
}

initTheme();
initTerms();
drawDots();
terms();
initPage();
// The held-out result's own words get their definitions, in the built copy and when it redraws.
const markResult = (el: ParentNode) => {
  const seen = new Set<string>();
  el.querySelectorAll(".ev-ho-plain, .ev-exec-what, .ev-exec-line").forEach((p) => termifyTree(p, seen, allTerms));
};
document.querySelectorAll("[data-heldout='full']").forEach(markResult);
void loadHeldout((el) => {
  drawDots(el);
  markResult(el);
});
