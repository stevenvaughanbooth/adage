// Traces: the live decision feed, newest first, with the replay scrubber over it.

import { useMemo, useState } from "react";
import type { Graph, TraceEvent } from "../api/types";
import { useStore } from "../api/client";
import { Lamp, RewriteGlyph } from "../components/glyphs";
import { SessionTrace } from "../components/SessionTrace";
import { measurement, relates, traceInventory, traceKey } from "../lib/traces";
import { Replay } from "../components/Replay";
import { Segmented, clock, cx, day } from "../components/ui";
import { findRoute, routeKey } from "../lib/graph";
import { arm, judgement, lifecycle, plural, pretoolWords, shortSession, strength } from "../lib/words";
import { WINDOWS, type Window, useUrl, windowStart } from "../state/url";

const EVENT: Record<string, string> = {
  SessionStart: "start",
  UserPromptSubmit: "prompt",
  PostToolBatch: "tools",
  PostToolUse: "tool",
  PostToolUseFailure: "tool failed",
  Stop: "stop",
  "session.start": "start",
  PreToolUse: "before a command",
};

/** A PreToolUse record: the command was rewritten (or advice given) before it ran. */
function PretoolLine({ e }: { e: TraceEvent }) {
  const withheld = Boolean(e.withheld);
  const rewrite = Boolean(e.pretool?.rewrite);
  const word = withheld ? (rewrite ? "would have rewritten" : "would have advised") : rewrite ? "rewrote" : "advised";
  return (
    <div className="flex flex-wrap items-center gap-1.5" data-testid="pretool-row">
      <span className="lbl mr-1">{EVENT[e.event] ?? e.event}</span>
      <span
        className={cx(
          "inline-flex h-6 items-center gap-1.5 rounded-full border px-2 text-[11.5px]",
          withheld ? "border-dashed border-line-strong text-ink-3" : "border-line-strong bg-raised text-ink-1",
        )}
        title={withheld ? "Control arm: recorded, not done" : "Adage changed the command before it ran; the agent saw a note"}
      >
        <span className="text-accent-soft">
          <RewriteGlyph size={12} />
        </span>
        {word}
      </span>
      <span className="text-xs text-ink-2">{pretoolWords(e.pretool, withheld)}</span>
    </div>
  );
}

function Row({ e, graph, fresh }: { e: TraceEvent; graph: Graph; fresh: boolean }) {
  const [, setUrl] = useUrl();
  const control = e.arm === "control";
  const chip = (id: string, would: boolean) => {
    const recording = [...(e.directives ?? []), ...(e.would_have ?? [])].find((d) => d.route === id);
    const r = recording?.route_key ? findRoute(graph, recording.route_key) : undefined;
    const s = strength(r?.strength);
    return (
      <button
        key={`${would ? "w" : "d"}${id}`}
        type="button"
        onClick={() => r && setUrl({ reflex: routeKey(r), view: "map" }, true)}
        disabled={!r}
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
    <li className={cx("grid grid-cols-[64px_minmax(0,1fr)] items-start gap-x-3 gap-y-1 border-b border-line px-4 py-2.5 sm:grid-cols-[76px_104px_minmax(0,1fr)] sm:px-5", fresh && "rise")}>
      <span className="pt-1 font-mono text-[11.5px] text-ink-3" title={new Date(e.ts).toLocaleString()}>
        {clock(e.ts)}
      </span>
      <span className="flex flex-col gap-0.5 pt-0.5">
        <span className={cx("text-xs", control ? "text-ink-3" : "text-ink-1")}>{arm(e.arm)}</span>
        <button type="button" className="text-left font-mono text-[11px] text-accent-soft underline underline-offset-2" onClick={() => setUrl({ session: e.session, traceRepo: e.repo, sequence: e.sequence ?? null }, true)} aria-label={`Open complete session ${e.session}`}>{shortSession(e.session)}</button>
      </span>
      <div className="col-span-2 flex min-w-0 flex-col gap-1.5 sm:col-span-1">
        {e.kind === "pretool" && <PretoolLine e={e} />}
        <div className={cx("flex flex-wrap items-center gap-1.5", e.kind === "pretool" && "hidden")}>
          <span className="lbl mr-1">{EVENT[e.event] ?? e.event}</span>
          {e.directed.map((id) => chip(id, false))}
          {e.would.filter((id) => !directed.has(id)).map((id) => chip(id, true))}
          {e.continue && <span className="text-xs text-prime">asked the agent to continue</span>}
        </div>
        {e.judged.length > 0 && (
          <div className="flex flex-wrap gap-x-3 gap-y-1 text-xs text-ink-2">
            {e.judged.map(([k, id], i) => (
              <span key={`${k}${id}${i}`}>
                <span className={k === "ignored" || k === "misfired" ? "text-inhibit" : k === "followed" || k === "obligation_met" ? "text-fire" : ""}>{judgement(k)}</span>{" "}
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
  const [armFilter, setArm] = useState<"all" | "treated" | "control">("all");
  const [loadedWith] = useState(streamed);
  const [search, setSearch] = useState("");
  const now = Math.max(Date.now(), ...graph.events.map((e) => e.ts));
  const selected = findRoute(graph, url.reflex);
  const inventory = traceInventory(graph);
  const conditioning = (graph.learning_history?.repositories ?? []).filter(r => !url.repo || r.repo === url.repo);
  const selectedMeasurement = selected ? measurement(selected, url.version) : null;
  const contributing = selectedMeasurement?.measurement_sessions;
  const choices = inventory.filter((s) => (!url.repo || s.repo === url.repo) && (!s.end_ts || s.end_ts >= windowStart(url.window, now)) &&
    (armFilter === "all" || s.arm === armFilter) && `${s.repo} ${s.session}`.toLowerCase().includes(search.toLowerCase()) &&
    (!selected || (contributing ? contributing.some((c) => traceKey(c) === traceKey(s)) : (graph.trace_events ?? graph.events).some((e) => traceKey(e) === traceKey(s) && relates(e, selected, url.version)))));
  const activeSession = url.session ? inventory.find((s) => s.session === url.session && s.repo === url.traceRepo) ?? { session: url.session, repo: url.traceRepo ?? url.repo ?? "", arm: null, events_total: 0 } : null;
  const events = useMemo(() => {
    const from = windowStart(url.window, now);
    return graph.events.filter(
      (e) =>
        (!url.repo || e.repo === url.repo) &&
        e.ts >= from &&
        (armFilter === "all" || e.arm === armFilter) &&
        (!selected || relates(e, selected, url.version)),
    );
  }, [graph.events, url.repo, url.window, armFilter, selected, url.version, now]);
  const shown = (at == null ? events : events.filter((e) => e.ts <= at)).slice().reverse();
  const freshFrom = graph.events.length - Math.max(0, streamed - loadedWith);
  const fresh = new Set(graph.events.slice(freshFrom));
  const byDay = new Map<string, TraceEvent[]>();
  for (const e of shown.slice(0, 500)) byDay.set(day(e.ts), [...(byDay.get(day(e.ts)) ?? []), e]);

  return (
    <main aria-label="Traces" className="scroll-thin flex min-h-0 grow flex-col overflow-y-auto lg:overflow-hidden">
      <div className="flex flex-wrap items-end justify-between gap-4 px-4 pt-7 pb-5 sm:px-10">
        <div className="flex flex-col gap-1.5">
          <span className="lbl">
            Recorded activity · {plural(choices.length, "recorded session")} · {plural(events.length, "recent decision")}
          </span>
          <h1 className="display m-0 text-[32px] leading-[1.05] font-semibold sm:text-[40px]">
            {selected ? (
              <>
                Where <span className="font-mono text-[26px] break-all text-accent-soft sm:text-[34px]">{selected.id}</span> applied
              </>
            ) : (
              <>
                Every decision, <span className="text-accent-soft">as it lands.</span>
              </>
            )}
          </h1>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {selected && (
            <button type="button" onClick={() => setUrl({ reflex: null, version: null })} className="h-8 rounded-md border border-line-strong px-3 text-[12.5px] text-ink-2 hover:text-ink-1">
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
              { value: "control", label: "Control" },
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
      <section aria-label="Learning history" className="border-t border-line px-4 py-3 text-xs text-ink-2 sm:px-10">
        <details>
          <summary className="cursor-pointer font-medium text-ink-1">Last conditioning · {conditioning.length === 1 ? `${conditioning[0]!.sessions_used ?? "Unrecorded"} sessions used · ${conditioning[0]!.files_read ?? "unrecorded"} files read` : `${conditioning.length} repositories with retained counts`}</summary>
          <p>Agent histories, including discovered subagent histories, are learning input. They are not automatically recorded hook sessions in the activity below. These counts describe the latest retained run per repository, not a lifetime total.</p>
          {conditioning.map(r => <div key={r.repo} className="mb-2 rounded border border-line p-2">
            <p className="m-0 break-words font-medium text-ink-1">{r.repo} · {new Date(r.last_conditioned_at).toLocaleString()}</p>
            <p className="my-1">{r.sessions_used ?? "Not recorded"} sessions used · {r.files_read ?? "Not recorded"} files read · {r.sessions_steered ?? "Not recorded"} steered sessions · {r.sessions_duplicates ?? "Not recorded"} duplicate sessions</p>
          </div>)}
          {!conditioning.length && <p>No retained conditioning counts for this selection. Older runs may not have recorded them.</p>}
          {graph.learning_history?.description && <p className="mb-0">{graph.learning_history.description}</p>}
        </details>
      </section>
      <div className="flex flex-wrap items-end gap-3 border-t border-line px-4 py-3 sm:px-10">
        <label className="flex min-w-0 flex-col gap-1 text-xs text-ink-2">Find a session<input type="search" value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Session or repository" className="h-9 rounded-md border border-line-strong bg-panel px-3 text-ink-1" /></label>
        <label className="flex min-w-0 grow flex-col gap-1 text-xs text-ink-2">Recorded session<select value={activeSession ? traceKey(activeSession) : ""} onChange={(e) => { const s = inventory.find((s) => traceKey(s) === e.target.value); setUrl({ session: s?.session ?? null, traceRepo: s?.repo ?? null, sequence: null }, true); }} className="h-9 min-w-0 max-w-full rounded-md border border-line-strong bg-panel px-3 text-ink-1">
          <option value="">Recent activity across sessions</option>
          {activeSession && !choices.some((s) => traceKey(s) === traceKey(activeSession)) && <option value={traceKey(activeSession)}>{activeSession.session} · selection outside these filters</option>}
          {choices.map((s) => <option key={traceKey(s)} value={traceKey(s)}>{s.session} · {s.repo} · {s.events_total} events</option>)}
        </select></label>
        {activeSession && <button type="button" onClick={() => setUrl({ session: null, sequence: null, traceRepo: null }, true)} className="h-9 rounded-md border border-line-strong px-3 text-xs">Recent activity</button>}
        {selected && <p className="m-0 w-full text-xs text-ink-2">Contributing sessions for {selected.id} · {selectedMeasurement?.version}. Select a session to read its recorded response and follow the outcome. {selectedMeasurement?.measurement_info?.description}</p>}
        {!choices.length && <p role="status" className="m-0 w-full text-xs text-ink-3">No recorded sessions match these filters. Your selected session stays open.</p>}
      </div>
      {activeSession ? <SessionTrace key={traceKey(activeSession)} graph={graph} session={activeSession} /> : <>
      <div className="border-t border-line">
        <Replay events={events} learning={graph.learning ?? []} at={at} onAt={setAt} compact />
      </div>
      <div className="scroll-thin min-h-0 grow overflow-y-auto border-t border-line bg-panel">
        {shown.length === 0 ? (
          <p className="px-4 py-10 text-sm text-ink-2 sm:px-10">No decisions in this window yet. They appear here as agents work.</p>
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
      </>}
    </main>
  );
}
