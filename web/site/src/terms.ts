// Words with a definition: hover, focus or tap a dotted word on /how to see what it means. The
// words and definitions are the shared glossary (web/map/src/how/glossary.ts); this file marks
// them in the page and shows one tooltip for whichever is active.

import { segments, type Segment } from "@how/glossary";

let tip: HTMLElement | null = null;
let current: HTMLElement | null = null;

// A term is an inline span, not a button, so it wraps like the words around it; it takes focus,
// and a screen reader reads the word and then its definition.
function node(s: Segment): Node {
  if (typeof s === "string") return document.createTextNode(s);
  const t = document.createElement("span");
  t.className = "gloss";
  t.tabIndex = 0;
  t.textContent = s.text;
  t.dataset.def = s.term.def;
  t.setAttribute("aria-describedby", "termtip");
  return t;
}

/** Sets `el` to `text`, marking the first mention of each term not already in `seen`. */
export function termify(el: HTMLElement, text: string, seen: Set<string>) {
  el.replaceChildren(...segments(text, seen).map(node));
}

/** Marks terms in text already on the page under `root`, leaving code, math, links, headings,
 *  decorations and live readouts (which their scripts rewrite) alone. */
export function termifyTree(root: Element, seen = new Set<string>()) {
  const skip = "code, math, a, button, h1, h2, h3, summary, label, output, .mnum, .gloss, [aria-hidden='true'], [aria-live]";
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => (n.parentElement?.closest(skip) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  const texts: Text[] = [];
  while (walker.nextNode()) texts.push(walker.currentNode as Text);
  for (const t of texts) {
    const parts = segments(t.data, seen);
    if (parts.length > 1 || typeof parts[0] !== "string") t.replaceWith(...parts.map(node));
  }
}

function place(b: HTMLElement) {
  const t = tip!;
  const r = b.getBoundingClientRect();
  const w = t.offsetWidth, h = t.offsetHeight, gap = 8, edge = 12;
  const x = Math.max(edge, Math.min(innerWidth - edge - w, r.left + r.width / 2 - w / 2));
  const y = r.top - h - gap >= edge ? r.top - h - gap : r.bottom + gap;
  t.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
}

function show(b: HTMLElement) {
  current = b;
  tip!.textContent = b.dataset.def ?? "";
  tip!.hidden = false;
  place(b);
}

function hide() {
  current = null;
  tip!.hidden = true;
}

export function initTerms() {
  tip = Object.assign(document.createElement("div"), { id: "termtip", className: "termtip", hidden: true });
  tip.setAttribute("role", "tooltip");
  document.body.append(tip);
  const term = (e: Event) => (e.target instanceof Element ? e.target.closest<HTMLElement>(".gloss") : null);
  // A mouse shows the definition on hover; a finger or a key shows it on tap or focus.
  document.addEventListener("pointerover", (e) => { const b = term(e); if (b && e.pointerType !== "touch") show(b); });
  document.addEventListener("pointerout", (e) => {
    const b = term(e);
    if (b && b === current && e.pointerType !== "touch" && document.activeElement !== b) hide();
  });
  document.addEventListener("focusin", (e) => { const b = term(e); if (b) show(b); });
  document.addEventListener("focusout", (e) => { if (term(e) && term(e) === current) hide(); });
  document.addEventListener("click", (e) => { const b = term(e); if (b) show(b); else if (current) hide(); });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && current) hide();
    const b = term(e);
    if (b && (e.key === "Enter" || e.key === " ")) { e.preventDefault(); show(b); }
  });
  addEventListener("scroll", () => current && place(current), { passive: true, capture: true });
  addEventListener("resize", () => current && place(current));
}
