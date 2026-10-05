// How it works: a step-through 3D walk from agent traces to reflexes, one chapter per part of
// Adage. The 3D stage (three.js) lives in this lazily loaded chunk; without WebGL the chapters
// still read and step, as text.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { Graph } from "../api/types";
import { TermText, useTermTip } from "../components/Terms";
import { Segmented, cx } from "../components/ui";
import { CHAPTERS, chapterIndex } from "../how/chapters";
import { chipMark, chipText } from "../how/chip";
import { segments } from "../how/glossary";
import { Stage, readPalette, webglAvailable } from "../how/engine";
import type { Built } from "../how/types";
import { useUrl } from "../state/url";

const STEP_MS = 5200;
const MORE_KEY = "adage.how.detail";

export default function HowItWorks({ graph }: { graph: Graph | null }) {
  const [url, setUrl] = useUrl();
  const chIndex = chapterIndex(url.ch);
  const chapter = CHAPTERS[chIndex]!;
  const canvas = useRef<HTMLCanvasElement>(null);
  const layer = useRef<HTMLDivElement>(null);
  const fig = useRef<HTMLCanvasElement>(null);
  const captionBox = useRef<HTMLDivElement>(null);
  const [stage, setStage] = useState<Stage | null>(null);
  const [gl] = useState(() => webglAvailable());
  const [built, setBuilt] = useState<Built | null>(null);
  const [playing, setPlaying] = useState(false);
  const [themeTick, setThemeTick] = useState(0);
  const repoAvailable = chapter.usesRepo && Boolean(graph?.routes.length);
  const [useRepo, setUseRepo] = useState(true);
  const repo = repoAvailable && useRepo;
  // "More detail" stays open from step to step once it is opened.
  const [more, setMoreState] = useState(() => {
    try {
      return localStorage.getItem(MORE_KEY) === "open";
    } catch {
      return false;
    }
  });
  const setMore = useCallback((m: boolean) => {
    setMoreState(m);
    try {
      localStorage.setItem(MORE_KEY, m ? "open" : "closed");
    } catch {
      /* not remembered */
    }
  }, []);

  // The stage lives as long as the view.
  useEffect(() => {
    if (!gl || !canvas.current || !layer.current) return;
    let s: Stage;
    try {
      s = new Stage(canvas.current, layer.current);
    } catch {
      return;
    }
    s.onTheme(() => setThemeTick((t) => t + 1));
    setStage(s);
    return () => s.dispose();
  }, [gl]);

  // The caption covers the stage's left side on wide screens: the scene centres in the rest.
  useEffect(() => {
    const el = captionBox.current;
    if (!stage || !el) return;
    const measure = () => stage.setInset(getComputedStyle(el).position === "absolute" ? el.offsetLeft + el.offsetWidth : 0);
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  }, [stage]);

  // Build the chapter (on the stage, or text only). The graph changes as traces arrive; a chapter
  // that uses this repository's data rebuilds with it.
  const graphKey = repo ? graph?.generation : null;
  useEffect(() => {
    const target = stage ?? textStage;
    target.clear();
    setBuilt(chapter.build(target as Stage, { graph, repo }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stage, chapter, repo, graphKey]);

  const steps = built?.steps ?? [];
  const step = Math.min(Math.max(0, url.step), Math.max(0, steps.length - 1));
  const cur = steps[step];

  useEffect(() => {
    if (stage && cur) stage.to(cur.state);
  }, [stage, cur]);

  // The caption's figure.
  useEffect(() => {
    const c = fig.current;
    if (!c || !cur?.figure) return;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const w = c.clientWidth;
    const h = c.clientHeight;
    c.width = w * dpr;
    c.height = h * dpr;
    const g = c.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    cur.figure(g, w, h, stage?.palette() ?? readPalette());
  }, [cur, stage, themeTick, more]);

  const go = useCallback(
    (ch: number, s: number, push = false) => {
      const c = CHAPTERS[(ch + CHAPTERS.length) % CHAPTERS.length]!;
      setUrl({ ch: c.id, step: Math.max(0, s) }, push);
    },
    [setUrl],
  );
  const next = useCallback(() => {
    if (step < steps.length - 1) go(chIndex, step + 1);
    else if (chIndex < CHAPTERS.length - 1) go(chIndex + 1, 0);
    else setPlaying(false);
  }, [step, steps.length, chIndex, go]);
  const prev = useCallback(() => {
    if (step > 0) go(chIndex, step - 1);
    else if (chIndex > 0) go(chIndex - 1, 999);
  }, [step, chIndex, go]);

  // A chapter entered from the end (going back) starts on its last step.
  useEffect(() => {
    if (url.step >= 999 && steps.length) setUrl({ step: steps.length - 1 });
  }, [url.step, steps.length, setUrl]);

  useEffect(() => {
    if (!playing) return;
    const t = window.setTimeout(next, STEP_MS);
    return () => window.clearTimeout(t);
  }, [playing, next, step, chIndex]);

  // Keys come from the app's one handler (App.tsx): ← → step, space plays.
  useEffect(() => {
    const on = (e: Event) => {
      const d = (e as CustomEvent<string>).detail;
      if (d === "next") (setPlaying(false), next());
      if (d === "prev") (setPlaying(false), prev());
      if (d === "toggle") setPlaying((p) => !p);
    };
    window.addEventListener("adage-how", on);
    return () => window.removeEventListener("adage-how", on);
  }, [next, prev]);

  const legend = useMemo(() => {
    const pal = stage?.palette() ?? readPalette();
    return (built?.legend ?? []).map((l) => ({
      label: l.label,
      color: l.role === "hue" ? `#${pal.hue(l.hue ?? 0).getHexString()}` : pal.css(l.role),
    }));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [built, stage, themeTick]);

  return (
    <main className="grid min-h-0 grow grid-cols-1 overflow-y-auto lg:grid-cols-[260px_minmax(0,1fr)] lg:overflow-hidden" aria-labelledby="how-title">
      <nav aria-label="Chapters" className="border-b border-line p-4 lg:overflow-y-auto lg:border-r lg:border-b-0">
        <h1 id="how-title" className="m-0 mb-3 font-mono text-[11px] font-medium tracking-[0.14em] text-ink-3 uppercase">
          How it works · traces to reflexes
        </h1>
        <label className="flex items-center gap-2 lg:hidden">
          <span className="sr-only">Chapter</span>
          <select
            value={chapter.id}
            onChange={(e) => (
              setPlaying(false),
              go(
                CHAPTERS.findIndex((c) => c.id === e.target.value),
                0,
                true,
              )
            )}
            className="h-9 w-full rounded-lg border border-line-strong bg-raised px-2 text-sm text-ink-1"
          >
            {CHAPTERS.map((c, i) => (
              <option key={c.id} value={c.id}>
                {String(i + 1).padStart(2, "0")} {c.title}
              </option>
            ))}
          </select>
        </label>
        <ol className="m-0 hidden list-none gap-0.5 p-0 lg:grid">
          {CHAPTERS.map((c, i) => (
            <li key={c.id}>
              <button
                type="button"
                onClick={() => (setPlaying(false), go(i, 0, true))}
                aria-current={i === chIndex ? "step" : undefined}
                className={cx(
                  "grid w-full grid-cols-[22px_1fr] gap-x-2 rounded-lg px-2.5 py-2 text-left",
                  i === chIndex ? "bg-raised text-ink-1 shadow-[inset_0_0_0_1px_var(--color-line-strong)]" : "text-ink-2 hover:bg-raised hover:text-ink-1",
                )}
              >
                <span className="pt-0.5 font-mono text-xs text-ink-3 tabular-nums">{String(i + 1).padStart(2, "0")}</span>
                <span className="text-sm font-semibold">{c.title}</span>
                <span className={cx("col-start-2 font-mono text-[11px]", c.kind === "flag" ? "text-prime" : "text-ink-3")}>{c.kind === "flag" ? c.status : c.technical}</span>
              </button>
            </li>
          ))}
        </ol>
        <p className="mt-4 mb-0 hidden max-w-[32ch] text-[13px] text-ink-3 lg:block">
          Every chapter describes Adage as it ships: the graph engine is the only learner, and it is on by default.
        </p>
      </nav>

      <section aria-label={`${chapter.title}: step ${step + 1} of ${steps.length}`} className="relative flex min-h-[560px] flex-col lg:min-h-0">
        <div className="relative flex grow flex-col overflow-hidden bg-[radial-gradient(120%_90%_at_70%_20%,color-mix(in_oklab,var(--color-accent-base)_9%,transparent),transparent_60%)] sm:min-h-[380px]">
          <div className="relative h-[360px] flex-none sm:absolute sm:inset-0 sm:h-auto">
            {gl ? (
              <>
                <canvas ref={canvas} className="absolute inset-0 block h-full w-full cursor-grab touch-pan-y active:cursor-grabbing" aria-hidden="true" />
                <div ref={layer} className="pointer-events-none absolute inset-0 overflow-hidden" aria-hidden="true" />
              </>
            ) : (
              <p className="absolute inset-0 m-0 grid place-items-center p-8 text-center text-sm text-ink-3">
                This view draws in 3D with WebGL, which this browser has turned off. Every step still reads below.
              </p>
            )}

            <div className="absolute top-3 right-3 flex flex-col items-end gap-2 sm:top-4 sm:right-4">
              <Segmented
                label="Whose traces"
                size="sm"
                value={repo ? "repo" : "sample"}
                onChange={(v) => setUseRepo(v === "repo")}
                options={[
                  { value: "sample", label: "Sample traces" },
                  {
                    value: "repo",
                    label: "This repository",
                    title: repoAvailable
                      ? "Replay this chapter with this repository's reflexes"
                      : "This chapter shows sample traces; the map does not read raw traces",
                  },
                ]}
              />
              {!repoAvailable && (
                <span className="font-mono text-[11px] text-ink-3">{chapter.usesRepo ? "no reflexes here yet" : "sample traces in this chapter"}</span>
              )}
              {legend.length > 0 && (
                <ul
                  className="m-0 hidden list-none gap-1 rounded-lg border border-line bg-[color-mix(in_oklab,var(--color-bg-panel)_80%,transparent)] p-2.5 font-mono text-[11px] text-ink-2 sm:grid"
                  aria-label="Legend"
                >
                  {legend.map((l) => (
                    <li key={l.label}>
                      <span className="mr-1.5 inline-block size-2 rounded-full" style={{ background: l.color }} />
                      {l.label}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          </div>

          <div ref={captionBox} className="sm:absolute sm:top-4 sm:left-4 sm:w-[min(420px,calc(100%-32px))]">
            <Caption chapter={chIndex} step={step} total={steps.length} built={built} figRef={fig} more={more} setMore={setMore} />
          </div>
        </div>

        <Transport
          steps={steps.map((s) => s.title)}
          step={step}
          playing={playing}
          onStep={(i) => (setPlaying(false), go(chIndex, i))}
          onPrev={() => (setPlaying(false), prev())}
          onNext={() => (setPlaying(false), next())}
          onPlay={() => setPlaying((p) => !p)}
        />
      </section>
    </main>
  );
}

/** Chapters build on this when there is no WebGL: they get their words, and nothing draws. */
const textStage = {
  dot() {},
  link() {},
  label() {
    return document.createElement("div");
  },
  clear() {},
  to() {},
  palette: () => readPalette(),
} as unknown as Stage;

function Caption({
  chapter,
  step,
  total,
  built,
  figRef,
  more,
  setMore,
}: {
  chapter: number;
  step: number;
  total: number;
  built: Built | null;
  figRef: React.RefObject<HTMLCanvasElement | null>;
  more: boolean;
  setMore: (m: boolean) => void;
}) {
  const c = CHAPTERS[chapter]!;
  const s = built?.steps[step];
  const { setTip, node } = useTermTip();
  // The first mention of each hard word gets its definition, in the plain words or the detail.
  const [plain, body] = useMemo(() => {
    const seen = new Set<string>();
    return [segments(s?.plain ?? "", seen), segments(s?.body ?? "", seen)];
  }, [s]);
  return (
    <div
      aria-live="polite"
      className="m-3 grid gap-2 rounded-xl border border-line-strong bg-[color-mix(in_oklab,var(--color-bg-panel)_84%,transparent)] p-4 backdrop-blur-md sm:m-0"
    >
      <div className="flex flex-wrap gap-x-2.5 gap-y-1 font-mono text-[11px] tracking-[0.1em] text-ink-3 uppercase">
        <span>
          {String(chapter + 1).padStart(2, "0")} {c.title}
        </span>
        <span>
          · step {step + 1} of {total}
        </span>
        <span className={c.kind === "flag" ? "text-prime" : undefined}>· {built?.source ?? ""}</span>
      </div>
      <h2 className="display m-0 text-[22px] leading-[1.15] font-semibold text-balance">{s?.title}</h2>
      <p className="m-0 text-[15px] leading-normal text-ink-1">
        <TermText parts={plain} onTip={setTip} />
      </p>
      {more && (
        <div id="how-more" className="grid gap-2 border-t border-dashed border-line-strong pt-2">
          <p className="m-0 text-sm text-ink-2">
            <TermText parts={body} onTip={setTip} />
          </p>
          {s?.figure && <canvas ref={figRef} className="block h-24 w-full" role="img" aria-label={`Figure: ${s.title}`} />}
          <div className="flex flex-wrap gap-1.5">
            {s?.chips.map((chip) => (
              <span key={chip.text} className="rounded-md border border-line bg-raised px-1.5 py-0.5 font-mono text-[11.5px] text-ink-1 tabular-nums">
                {chipMark(chip) && <span className={chip.ok ? "text-fire" : "text-inhibit"}>{chipMark(chip)} </span>}
                {chipText(chip)}
              </span>
            ))}
          </div>
        </div>
      )}
      <div>
        <button
          type="button"
          aria-expanded={more}
          aria-controls="how-more"
          onClick={() => setMore(!more)}
          className="inline-flex items-center gap-1.5 rounded-full border border-line-strong bg-raised px-3 py-1 text-[13px] font-semibold text-ink-1 hover:border-accent"
        >
          {more ? "Less detail" : "More detail"}
          <svg viewBox="0 0 12 12" aria-hidden="true" className={cx("size-3 fill-none stroke-current stroke-[1.6]", more && "rotate-180")}>
            <path d="M2.5 4.5 6 8l3.5-3.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      </div>
      {node}
    </div>
  );
}

function Transport(p: {
  steps: string[];
  step: number;
  playing: boolean;
  onStep: (i: number) => void;
  onPrev: () => void;
  onNext: () => void;
  onPlay: () => void;
}) {
  const btn = "grid size-9 flex-none place-items-center rounded-lg border border-line-strong bg-raised hover:border-accent";
  return (
    <div className="flex items-center gap-2.5 border-t border-line bg-panel px-3 py-2">
      <button type="button" className={btn} onClick={p.onPrev} aria-label="Previous step">
        <svg viewBox="0 0 16 16" className="size-3.5 fill-ink-1" aria-hidden="true">
          <path d="M11 2 4 8l7 6z" />
        </svg>
      </button>
      <button type="button" className={btn} onClick={p.onPlay} aria-label={p.playing ? "Pause" : "Play"}>
        <svg viewBox="0 0 16 16" className="size-3.5 fill-ink-1" aria-hidden="true">
          {p.playing ? <path d="M3 2h4v12H3zM9 2h4v12H9z" /> : <path d="M4 2l10 6-10 6z" />}
        </svg>
      </button>
      <button type="button" className={btn} onClick={p.onNext} aria-label="Next step">
        <svg viewBox="0 0 16 16" className="size-3.5 fill-ink-1" aria-hidden="true">
          <path d="M5 2l7 6-7 6z" />
        </svg>
      </button>
      <ol className="m-0 flex min-w-0 grow list-none p-0" aria-label="Steps">
        {p.steps.map((t, i) => (
          <li key={t} className="min-w-0 flex-1">
            <button
              type="button"
              onClick={() => p.onStep(i)}
              aria-label={`Step ${i + 1}: ${t}`}
              aria-current={i === p.step ? "step" : undefined}
              title={`${i + 1} · ${t}`}
              className="group relative h-9 w-full"
            >
              <span
                className={cx(
                  "absolute inset-x-0.5 top-1/2 -translate-y-1/2 rounded-sm transition-colors",
                  i === p.step
                    ? "h-[5px] bg-accent"
                    : i < p.step
                      ? "h-[3px] bg-[color-mix(in_oklab,var(--color-accent-base)_55%,transparent)]"
                      : "h-[3px] bg-line-strong",
                )}
              />
            </button>
          </li>
        ))}
      </ol>
      <span className="flex-none text-right font-mono text-xs whitespace-nowrap text-ink-2 tabular-nums">
        {p.step + 1} / {p.steps.length}
      </span>
    </div>
  );
}
