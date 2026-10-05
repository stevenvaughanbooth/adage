import { useEffect, useRef, useState } from "react";
import { fetchJson } from "../api/client";
import { normalizeEvent } from "../api/normalize";
import type {
  CompleteTrace,
  Graph,
  RecordedDirective,
  TraceEvent,
  TraceSession,
} from "../api/types";
import { findRoute } from "../lib/graph";
import { traceKey } from "../lib/traces";
import { arm, judgement } from "../lib/words";
import { useUrl } from "../state/url";
import { Directive } from "./Inspector";
import { clock, cx } from "./ui";

function Recording({
  d,
  graph,
  withheld,
  control,
}: {
  d: RecordedDirective;
  graph: Graph;
  withheld?: boolean;
  control?: boolean;
}) {
  const [, setUrl] = useUrl();
  const route = d.route_key ? findRoute(graph, d.route_key) : undefined;
  return (
    <div className="flex min-w-0 flex-col gap-2 rounded-lg border border-line-strong p-3">
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <strong className="font-mono break-all">{d.route}</strong>
        <span className="text-ink-3">
          {d.track}@{d.version} · {d.attribution ?? "unattributed"}
        </span>
        {route && (
          <button
            type="button"
            className="text-accent-soft underline underline-offset-4"
            onClick={() => setUrl({ view: "map", reflex: d.route_key }, true)}
          >
            Inspect this reflex
          </button>
        )}
      </div>
      <span className="lbl">
        {withheld
          ? control
            ? "Control · not delivered"
            : "Candidate · not delivered"
          : "Recorded directive"}
      </span>
      {d.text ? (
        <Directive text={d.text} />
      ) : (
        <p className="m-0 text-xs text-ink-3">
          The exact directive was not recorded for this event.
        </p>
      )}
    </div>
  );
}

/** A complete session, fetched independently of the recent activity window. URL owns selection. */
export function SessionTrace({
  graph,
  session,
}: {
  graph: Graph;
  session: TraceSession;
}) {
  const [url, setUrl] = useUrl();
  const key = traceKey(session);
  const [loaded, setLoaded] = useState<{
    key: string;
    trace: CompleteTrace;
  } | null>(null);
  const [status, setStatus] = useState<{
    key: string;
    error: string | null;
    loading: boolean;
  }>({ key, error: null, loading: true });
  const [attempt, setAttempt] = useState(0);
  const inspector = useRef<HTMLElement>(null);
  useEffect(() => {
    let active = true;
    setStatus({ key, error: null, loading: true });
    const p = new URLSearchParams({
      session: session.session,
      repo: session.repo,
    });
    fetchJson<CompleteTrace>(`/api/v1/trace?${p}`)
      .then((trace) => {
        if (
          !trace.complete ||
          trace.session !== session.session ||
          trace.repo !== session.repo ||
          !Array.isArray(trace.events)
        )
          throw new Error(
            "The server returned a different or incomplete trace.",
          );
        if (active) {
          setLoaded({
            key,
            trace: { ...trace, events: trace.events.map(normalizeEvent) },
          });
          setStatus({ key, error: null, loading: false });
        }
      })
      .catch((error: unknown) => {
        if (active)
          setStatus({
            key,
            error: error instanceof Error ? error.message : String(error),
            loading: false,
          });
      });
    return () => {
      active = false;
    };
  }, [key, session.session, session.repo, session.events_total, attempt]);
  const full = loaded?.key === key ? loaded.trace : null;
  const events =
    full?.events ??
    (graph.trace_events ?? graph.events).filter((e) => traceKey(e) === key);
  const index =
    url.sequence === null
      ? 0
      : events.findIndex((e) => e.sequence === url.sequence);
  const event = events[index];
  const loading = status.key !== key || status.loading;
  const error = status.key === key ? status.error : null;
  const choose = (e: TraceEvent | undefined, focus = false) => {
    if (!e || e.sequence === undefined) return;
    setUrl({ sequence: e.sequence }, true);
    if (focus)
      requestAnimationFrame(() => {
        inspector.current?.focus();
        inspector.current?.scrollIntoView({
          block: "nearest",
          behavior: "instant",
        });
      });
  };
  useEffect(() => {
    if (url.sequence === null && events[0]?.sequence !== undefined)
      setUrl({ sequence: events[0].sequence });
  }, [key, events[0]?.sequence, url.sequence, setUrl]);
  const outcomes = event
    ? events.flatMap((later) =>
        (later.judgements ?? [])
          .filter(
            (j) =>
              j.exposure_sequence === event.sequence &&
              j.route_key &&
              [...(event.directives ?? []), ...(event.would_have ?? [])].some(
                (d) => d.route_key === j.route_key && d.version === j.version,
              ),
          )
          .map((j) => ({ j, later })),
      )
    : [];
  return (
    <section
      aria-label="Complete session trace"
      className="flex min-h-[32rem] min-w-0 shrink-0 grow flex-col border-t border-line bg-panel lg:min-h-0 lg:shrink"
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-line px-4 py-3 text-xs sm:px-5">
        <span className="min-w-0 break-all font-mono">
          {session.repo} · {session.session}
        </span>
        <span className="text-ink-3">{arm(session.arm)}</span>
        <span role="status">
          {loading
            ? "Loading complete trace…"
            : error
              ? "Complete trace unavailable"
              : `${events.length} of ${full?.events_total ?? session.events_total} recorded events loaded`}
        </span>
        {error && (
          <>
            <span className="text-inhibit">{error}</span>
            <button
              type="button"
              className="text-accent-soft underline"
              onClick={() => setAttempt((n) => n + 1)}
            >
              Retry complete trace
            </button>
          </>
        )}
      </div>
      {events.length > 0 && (
        <div className="border-b border-line p-4">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2 text-xs text-ink-2">
            <span>Decision graph · select an event to read what happened</span>
            <button
              type="button"
              disabled={!event}
              className="text-accent-soft underline underline-offset-4"
              onClick={() => {
                inspector.current?.focus();
                inspector.current?.scrollIntoView({ block: "nearest" });
              }}
            >
              Jump to selected event details
            </button>
          </div>
          <div
            className="scroll-thin flex gap-2 overflow-x-auto pb-2"
            role="group"
            aria-label="Session decision graph"
          >
            {events.map((e, i) => (
              <button
                type="button"
                key={e.sequence ?? i}
                aria-pressed={index === i}
                aria-label={`Event ${e.sequence ?? i}: ${e.event}`}
                onClick={() => choose(e, true)}
                className={cx(
                  "flex min-w-28 shrink-0 flex-col gap-1 rounded-lg border p-3 text-left text-xs",
                  index === i
                    ? "border-accent bg-accent/10"
                    : "border-line-strong hover:border-accent/50",
                )}
              >
                <span className="font-mono text-ink-3">
                  {e.sequence ?? i} · {clock(e.ts)}
                </span>
                <strong>{e.event}</strong>
                <span className="text-ink-2">
                  {e.directed.length
                    ? `${e.directed.length} directed`
                    : e.would.length
                      ? e.arm === "control"
                        ? "control: withheld"
                        : "candidate withheld"
                      : e.judged.length
                        ? "outcome recorded"
                        : "observed"}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}
      <section
        ref={inspector}
        tabIndex={-1}
        aria-label="Selected event details"
        className="scroll-thin flex min-h-64 grow flex-col gap-4 overflow-y-auto p-4 outline-offset-[-2px] sm:p-5 lg:min-h-0"
      >
        {!event ? (
          <p className="text-sm text-ink-2">
            {loading
              ? "Loading the selected event…"
              : url.sequence !== null
                ? `Event ${url.sequence} is not in this recorded session. Your selection has been kept.`
                : "No recorded events in this session."}
          </p>
        ) : (
          <>
            <div className="flex flex-wrap items-center justify-between gap-3">
              <h2 className="display m-0 text-xl font-semibold">
                Event {event.sequence} · {event.event}
              </h2>
              <div className="flex gap-2">
                <button
                  type="button"
                  disabled={index <= 0}
                  className="rounded-md border border-line-strong px-3 py-1.5 text-xs disabled:opacity-40"
                  onClick={() => choose(events[index - 1])}
                >
                  Previous event
                </button>
                <button
                  type="button"
                  disabled={index < 0 || index >= events.length - 1}
                  className="rounded-md border border-line-strong px-3 py-1.5 text-xs disabled:opacity-40"
                  onClick={() => choose(events[index + 1])}
                >
                  Next event
                </button>
              </div>
            </div>
            {event.observed.length > 0 && (
              <div>
                <h3 className="lbl">Observed action</h3>
                <pre className="m-0 overflow-x-auto whitespace-pre-wrap break-words text-xs text-ink-2">
                  {event.observed.join("\n")}
                </pre>
              </div>
            )}
            <div className="grid gap-3 lg:grid-cols-2">
              {(event.directives ?? []).map((d, i) => (
                <Recording key={`d${i}`} d={d} graph={graph} />
              ))}
              {(event.would_have ?? [])
                .filter(
                  (d) =>
                    !event.directives?.some(
                      (sent) =>
                        sent.route === d.route &&
                        sent.track === d.track &&
                        sent.version === d.version,
                    ),
                )
                .map((d, i) => (
                  <Recording
                    key={`w${i}`}
                    d={d}
                    graph={graph}
                    withheld
                    control={event.arm === "control"}
                  />
                ))}
            </div>
            {event.response && (
              <div>
                <h3 className="lbl">Recorded hook response</h3>
                <Directive text={event.response} />
              </div>
            )}
            {!event.response &&
              !event.directives?.length &&
              !event.would_have?.length && (
                <p className="m-0 text-sm text-ink-3">
                  {event.directed.length || event.would.length
                    ? "This older record has no exact response or version attribution."
                    : "No reflex response was recorded at this event."}
                </p>
              )}
            {(event.judgements?.length ?? 0) > 0 && (
              <div>
                <h3 className="lbl">Judgements at this event</h3>
                {event.judgements!.map((j, i) => (
                  <p key={i} className="text-sm text-ink-2">
                    {j.route} · {judgement(j.kind)}
                    {j.exposure_sequence != null && (
                      <>
                        {" "}
                        ·{" "}
                        <button
                          type="button"
                          className="text-accent-soft underline"
                          onClick={() =>
                            choose(
                              events.find(
                                (e) => e.sequence === j.exposure_sequence,
                              ),
                            )
                          }
                        >
                          View exposure {j.exposure_sequence}
                        </button>
                      </>
                    )}
                  </p>
                ))}
              </div>
            )}
            {outcomes.length > 0 && (
              <div>
                <h3 className="lbl">What followed this response</h3>
                {outcomes.map(({ j, later }, i) => (
                  <p key={i} className="text-sm text-ink-2">
                    {j.route} · {judgement(j.kind)} ·{" "}
                    <button
                      type="button"
                      className="text-accent-soft underline"
                      onClick={() => choose(later)}
                    >
                      Read event {later.sequence}
                    </button>
                  </p>
                ))}
              </div>
            )}
            <p className="m-0 text-xs leading-relaxed text-ink-3">
              “Not observed” means no matching action was observed within the
              next three tool batches. It does not prove the agent ignored the
              response. Hooks and visibility differ by harness; control
              responses are withheld on purpose.
            </p>
          </>
        )}
      </section>
    </section>
  );
}
