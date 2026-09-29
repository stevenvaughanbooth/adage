// The replay scrubber: every decision as a tick, learning runs as tall lamps, and a handle that
// rebuilds the map as it was at that time. Space plays it.

import { useEffect, useMemo, useRef, useState } from "react";
import type { LearnRun, TraceEvent } from "../api/types";
import { reducedMotion } from "../state/theme";
import { Icons } from "./glyphs";
import { cx, day } from "./ui";
import { prose } from "../lib/words";

const PLAY_MS = 12000;

export interface Span {
  from: number;
  to: number;
}

export function span(events: TraceEvent[], now: number): Span | null {
  if (!events.length) return null;
  const from = Math.min(...events.map((e) => e.ts));
  const to = Math.max(now, ...events.map((e) => e.ts));
  return { from, to: to > from ? to : from + 1 };
}

/** The newest learning change at or before `at`, for the middle label. */
function milestone(learning: LearnRun[], at: number): string | null {
  const runs = learning.filter((l) => l.ts <= at && (l.changes ?? []).some((c) => c.change !== "evidence"));
  const last = runs[runs.length - 1];
  const c = last?.changes?.find((x) => x.change !== "evidence");
  if (!last || !c) return null;
  return `${day(last.ts)} · ${c.route} ${c.to ?? c.change}`;
}

export function Replay({
  events,
  learning,
  at,
  onAt,
  onPlayEvent,
  compact = false,
}: {
  events: TraceEvent[];
  learning: LearnRun[];
  at: number | null;
  onAt: (t: number | null) => void;
  /** Each decision the playhead crosses while playing. */
  onPlayEvent?: (e: TraceEvent) => void;
  compact?: boolean;
}) {
  const [now] = useState(() => Date.now());
  const sp = useMemo(() => span(events, Math.max(now, ...events.map((e) => e.ts))), [events, now]);
  const [playing, setPlaying] = useState(false);
  const raf = useRef(0);
  const cross = useRef(onPlayEvent);
  cross.current = onPlayEvent;

  useEffect(() => {
    if (!playing || !sp) return;
    const reduced = reducedMotion();
    const start = performance.now();
    const t0 = at != null && at < sp.to ? at : sp.from;
    const sorted = [...events].sort((a, b) => a.ts - b.ts);
    let last = t0;
    if (reduced) {
      onAt(null);
      setPlaying(false);
      return;
    }
    const step = (t: number) => {
      const k = Math.min(1, (t - start) / (PLAY_MS * (1 - (t0 - sp.from) / (sp.to - sp.from))));
      const cur = t0 + (sp.to - t0) * k;
      for (const e of sorted) if (e.ts > last && e.ts <= cur) cross.current?.(e);
      last = cur;
      if (k >= 1) {
        onAt(null);
        setPlaying(false);
        return;
      }
      onAt(cur);
      raf.current = requestAnimationFrame(step);
    };
    raf.current = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf.current);
  }, [playing]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const onKey = (e: Event) => {
      if ((e as CustomEvent).detail === "toggle") setPlaying((p) => !p);
    };
    window.addEventListener("adage-replay", onKey);
    return () => window.removeEventListener("adage-replay", onKey);
  }, []);

  if (!sp) {
    return (
      <div className={cx("flex items-center gap-3.5 border-t border-line bg-panel px-4 font-mono text-[11.5px] text-ink-3", compact ? "h-12" : "h-16")}>
        No traces yet: the replay fills in as they land.
      </div>
    );
  }
  const pos = at == null ? 1 : (at - sp.from) / (sp.to - sp.from);
  const pct = (t: number) => ((t - sp.from) / (sp.to - sp.from)) * 100;
  const label = milestone(learning, at ?? sp.to);
  const big = new Set(learning.filter((l) => (l.changes ?? []).some((c) => c.change !== "evidence")).map((l) => l.ts));
  return (
    <div className={cx("flex items-center gap-3.5 border-t border-line bg-panel px-4", compact ? "h-14" : "h-16")} data-testid="replay">
      <button
        type="button"
        onClick={() => setPlaying((p) => !p)}
        aria-label={playing ? "Pause the replay" : "Replay how the map formed"}
        title="Replay how the map formed (space)"
        className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-line-strong bg-raised text-ink-1 hover:border-accent"
      >
        {playing ? <Icons.pause /> : <Icons.play />}
      </button>
      <div className="flex grow flex-col gap-1.5">
        <div className="relative h-[18px]">
          <div className="absolute inset-x-0 top-2 h-0.5 rounded-sm bg-line-strong" />
          <div className="absolute left-0 top-2 h-0.5 rounded-sm bg-accent" style={{ width: `${pos * 100}%` }} />
          {events.map((e, i) => (
            <div
              key={`${e.ts}-${i}`}
              className="absolute top-1 h-2.5 w-0.5 rounded-sm"
              style={{
                left: `${pct(e.ts)}%`,
                background: e.ts <= (at ?? sp.to) ? "color-mix(in oklab, var(--color-accent-soft) 55%, transparent)" : "var(--color-line-strong)",
              }}
            />
          ))}
          {learning.map((l, i) => (
            <div
              key={`l${l.ts}-${i}`}
              title={(l.changes ?? []).map((c) => (c.summary ? prose(c.summary) : c.route)).join("\n")}
              className="absolute top-px w-0.5 rounded-sm"
              style={{
                left: `${Math.max(0, Math.min(100, pct(l.ts)))}%`,
                height: big.has(l.ts) ? 16 : 12,
                background: big.has(l.ts) ? "var(--color-lamp-fire)" : "var(--color-line-strong)",
              }}
            />
          ))}
          <div
            className="pointer-events-none absolute top-0.5 -ml-[7px] h-3.5 w-3.5 rounded-full bg-ink-1 shadow-[0_0_0_4px_var(--sel-ring)]"
            style={{ left: `${pos * 100}%` }}
          />
          <input
            type="range"
            min={0}
            max={1000}
            value={Math.round(pos * 1000)}
            aria-label="Replay time"
            aria-valuetext={at == null ? "now, live" : new Date(at).toLocaleString()}
            onChange={(e) => {
              setPlaying(false);
              const k = Number(e.target.value) / 1000;
              onAt(k >= 0.999 ? null : sp.from + k * (sp.to - sp.from));
            }}
            className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
          />
        </div>
        <div className="flex justify-between gap-4 font-mono text-[10.5px] text-ink-3">
          <span className="whitespace-nowrap">{day(sp.from)} · first trace</span>
          {label && <span className="hidden truncate sm:inline">{label}</span>}
          <span>{at == null ? "now" : new Date(at).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</span>
        </div>
      </div>
    </div>
  );
}
