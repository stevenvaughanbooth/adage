// Traces: the live decision feed, newest first, with the replay scrubber over it.

import { useMemo, useState } from "react";
import type { Graph, TraceEvent } from "../api/types";
import { useStore } from "../api/client";
import { Lamp } from "../components/glyphs";
import { Replay } from "../components/Replay";
import { Segmented, clock, cx, day } from "../components/ui";
import { findRoute, routeKey } from "../lib/graph";
import { arm, judgement, lifecycle, plural, shortSession, strength } from "../lib/words";
import { WINDOWS, type Window, useUrl, windowStart } from "../state/url";

const EVENT: Record<string, string> = {
  SessionStart: "start",
  UserPromptSubmit: "prompt",
  PostToolBatch: "tools",
  PostToolUse: "tool",
  PostToolUseFailure: "tool failed",
  Stop: "stop",
  "session.start": "start",
};

function Row({ e, graph, fresh }: { e: TraceEvent; graph: Graph; fresh: boolean }) {
  const [, setUrl] = useUrl();
  const control = e.arm === "holdout";
  const chip = (id: string, would: boolean) => {
    const r = graph.routes.find((x) => x.id === id && x.repo === e.repo);
    const s = strength(r?.aspect);
    return (
      <button
        key={`${would ? "w" : "d"}${id}`}
        type="button"
        onClick={() => r && setUrl({ reflex: routeKey(r), view: "map" }, true)}
        title={would ? "Would have been directed (control arm: nothing was said)" : "Directed"}
        className={cx(
          "inline-flex h-6 items-center gap-1.5 rounded-full border px-2 font-mono text-[11.5px] text-ink-1 hover:border-accent/45",
          would ? "border-dashed border-line-strong" : "border-line-strong bg-raised",
        )}
      >
        <Lamp strength={s} hollow={would || lifecycle(r?.state) === "nascent"} size={7} />
        {would && <span className="text-ink-3">would</span>}
        {id}
      </button>
    );
  };
  const directed = new Set(e.directed);
  return (
    <li className={cx("grid grid-cols-[76px_104px_minmax(0,1fr)] items-start gap-3 border-b border-line px-5 py-2.5", fresh && "rise")}>
      <span className="pt-1 font-mono text-[11.5px] text-ink-3" title={new Date(e.ts).toLocaleString()}>
        {clock(e.ts)}
      </span>
      <span className="flex flex-col gap-0.5 pt-0.5">
        <span className={cx("text-xs", control ? "text-ink-3" : "text-ink-1")}>{arm(e.arm)}</span>
        <span className="font-mono text-[11px] text-ink-3">{shortSession(e.session)}</span>
      </span>
      <div className="flex min-w-0 flex-col gap-1.5">
        <div className="flex flex-wrap items-center gap-1.5">
          <span className="lbl mr-1">{EVENT[e.event] ?? e.event}</span>
          {e.directed.map((id) => chip(id, false))}
          {e.would.filter((id) => !directed.has(id)).map((id) => chip(id, true))}
          {e.continue && <span className="text-xs text-prime">asked the agent to continue</span>}
        </div>
        {e.judged.length > 0 && (
          <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-ink-2">
            {e.judged.map(([k, id], i) => (
              <span key={`${k}${id}${i}`}>
                <span className={k === "derailed" ? "text-inhibit" : k === "followed" || k === "obligation_met" ? "text-fire" : ""}>{judgement(k)}</span>{" "}
                <span className="font-mono text-ink-1">{id}</span>
                {control && k === "followed" && <span className="text-ink-3"> unaided</span>}
              </span>
            ))}
          </div>
        )}
        {e.observed.length > 0 && (
          <div className="truncate font-mono text-[11px] text-ink-3" title={e.observed.join("\n")}>
            {e.observed.join(" · ")}
          </div>
        )}
      </div>
    </li>
  );
}

export function Traces({ graph }: { graph: Graph }) {
  const [url, setUrl] = useUrl();
  const { streamed } = useStore();
  const [at, setAt] = useState<number | null>(null);
  const [armFilter, setArm] = useState<"all" | "treated" | "holdout">("all");
  const [loadedWith] = useState(streamed);
  const now = Math.max(Date.now(), ...graph.events.map((e) => e.ts));
  const selected = findRoute(graph, url.reflex);
  const events = useMemo(() => {
    const from = windowStart(url.window, now);
    return graph.events.filter(
      (e) =>
        (!url.repo || e.repo === url.repo) &&
        e.ts >= from &&
        (armFilter === "all" || e.arm === armFilter) &&
        (!selected || (e.repo === selected.repo && (e.directed.includes(selected.id) || e.would.includes(selected.id) || e.judged.some(([, id]) => id === selected.id)))),
    );
  }, [graph.events, url.repo, url.window, armFilter, selected, now]);
  const shown = (at == null ? events : events.filter((e) => e.ts <= at)).slice().reverse();
  const sessions = new Set(events.map((e) => e.session)).size;
  const freshFrom = graph.events.length - Math.max(0, streamed - loadedWith);
  const fresh = new Set(graph.events.slice(freshFrom));
  const byDay = new Map<string, TraceEvent[]>();
  for (const e of shown.slice(0, 500)) byDay.set(day(e.ts), [...(byDay.get(day(e.ts)) ?? []), e]);

  return (
    <main aria-label="Traces" className="flex min-h-0 grow flex-col">
      <div className="flex flex-wrap items-end justify-between gap-4 px-10 pt-7 pb-5">
        <div className="flex flex-col gap-1.5">
          <span className="lbl">
            Traces · {plural(sessions, "trace")} · {plural(events.length, "decision")}
          </span>
          <h1 className="display m-0 text-[40px] leading-[1.05] font-semibold">
            {selected ? (
              <>
                Where <span className="font-mono text-[34px] text-accent-soft">{selected.id}</span> applied
              </>
            ) : (
              <>
                Every decision, <span className="text-accent-soft">as it lands.</span>
              </>
            )}
          </h1>
        </div>
        <div className="flex items-center gap-2">
          {selected && (
            <button type="button" onClick={() => setUrl({ reflex: null })} className="h-8 rounded-md border border-line-strong px-3 text-[12.5px] text-ink-2 hover:text-ink-1">
              All reflexes
            </button>
          )}
          <Segmented
            label="Arm"
            size="sm"
            value={armFilter}
            onChange={setArm}
            options={[
              { value: "all", label: "Both arms" },
              { value: "treated", label: "With reflexes" },
              { value: "holdout", label: "Control" },
            ]}
          />
          <Segmented<Window>
            label="Window"
            size="sm"
            value={url.window}
            onChange={(w) => setUrl({ window: w })}
            options={WINDOWS.map((w) => ({ value: w, label: w === "all" ? "All" : w.replace("d", " d").replace("h", " h") }))}
          />
        </div>
      </div>
      <div className="border-t border-line">
        <Replay events={events} learning={graph.learning ?? []} at={at} onAt={setAt} compact />
      </div>
      <div className="scroll-thin min-h-0 grow overflow-y-auto border-t border-line bg-panel">
        {shown.length === 0 ? (
          <p className="px-10 py-10 text-sm text-ink-2">No decisions in this window yet. They appear here as agents work.</p>
        ) : (
          [...byDay.entries()].map(([d, es]) => (
            <section key={d} aria-label={d}>
              <h2 className="lbl sticky top-0 z-10 m-0 border-b border-line bg-panel px-5 py-2 font-normal">{d}</h2>
              <ol className="m-0 list-none p-0">
                {es.map((e, i) => (
                  <Row key={`${e.ts}${e.session}${i}`} e={e} graph={graph} fresh={fresh.has(e)} />
                ))}
              </ol>
            </section>
          ))
        )}
      </div>
    </main>
  );
}
