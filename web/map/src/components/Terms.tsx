// Words with a definition in How it works: hover, focus or tap a dotted word to see what it means.
// The words and definitions are the shared glossary (../how/glossary.ts), which the site uses too.
// One tooltip serves the caption; it renders into the body, so the stage never clips it.

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Segment } from "../how/glossary";

type Tip = { def: string; el: HTMLElement } | null;
const TIP_ID = "how-termtip";

export function useTermTip() {
  const [tip, setTip] = useState<Tip>(null);
  const box = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const t = box.current;
    if (!tip || !t) return;
    const r = tip.el.getBoundingClientRect();
    const w = t.offsetWidth, h = t.offsetHeight, gap = 8, edge = 12;
    const x = Math.max(edge, Math.min(window.innerWidth - edge - w, r.left + r.width / 2 - w / 2));
    const y = r.top - h - gap >= edge ? r.top - h - gap : r.bottom + gap;
    t.style.transform = `translate(${Math.round(x)}px, ${Math.round(y)}px)`;
  }, [tip]);
  useEffect(() => {
    if (!tip) return;
    const hide = () => setTip(null);
    const key = (e: KeyboardEvent) => e.key === "Escape" && hide();
    window.addEventListener("scroll", hide, true);
    window.addEventListener("keydown", key);
    return () => (window.removeEventListener("scroll", hide, true), window.removeEventListener("keydown", key));
  }, [tip]);
  const node = createPortal(
    <div
      ref={box}
      id={TIP_ID}
      role="tooltip"
      hidden={!tip}
      className="pointer-events-none fixed top-0 left-0 z-50 max-w-[min(320px,calc(100vw-24px))] rounded-lg border border-line-strong bg-raised px-3 py-2.5 text-[13px] leading-snug text-ink-1 shadow-lg"
    >
      {tip?.def}
    </div>,
    document.body,
  );
  return { setTip, node };
}

/** Text with its terms marked; `onTip` shows or hides the definition. */
export function TermText({ parts, onTip }: { parts: Segment[]; onTip: (t: Tip) => void }) {
  return (
    <>
      {parts.map((p, i) =>
        typeof p === "string" ? (
          p
        ) : (
          <span
            key={i}
            tabIndex={0}
            aria-describedby={TIP_ID}
            className="cursor-help rounded-sm underline decoration-accent/70 decoration-dotted decoration-[1.5px] underline-offset-[3px] hover:text-ink-1 focus-visible:outline-2 focus-visible:outline-accent"
            onPointerEnter={(e) => e.pointerType !== "touch" && onTip({ def: p.term.def, el: e.currentTarget })}
            onPointerLeave={(e) => e.pointerType !== "touch" && document.activeElement !== e.currentTarget && onTip(null)}
            onFocus={(e) => onTip({ def: p.term.def, el: e.currentTarget })}
            onBlur={() => onTip(null)}
            onClick={(e) => onTip({ def: p.term.def, el: e.currentTarget })}
          >
            {p.text}
          </span>
        ),
      )}
    </>
  );
}
