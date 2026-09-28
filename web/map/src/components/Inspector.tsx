// The right inspector: the exact directive, its evidence, with vs control, its lifecycle, and the
// commands a person runs to decide about it. The page itself never changes anything.

import type { ReactNode } from "react";
import type { ArmStats, Graph, Route } from "../api/types";
import { findRoute, routeKey } from "../lib/graph";
import {
  CONTROL_NOTE,
  LIVE_SOURCE,
  type Lifecycle,
  evidenceNote,
  evidenceSource,
  isInstinct,
  judgement,
  lifecycle,
  orderedVerbs,
  plural,
  reviewCommand,
  strength,
  verbWhy,
} from "../lib/words";
import { useUrl } from "../state/url";
import { KindGlyph, Lamp } from "./glyphs";
import { CommandButton, LifecycleChip, Pill, Prose, StrengthChip, cx } from "./ui";

function Section({ label, source, note, children }: { label: string; source?: string; note?: ReactNode; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2.5" aria-label={source ? `${source} · ${label}` : label}>
      <div className="flex flex-col gap-1">
        <h3 className="lbl m-0 font-normal">
          {source && <span className="text-accent-soft">{source} · </span>}
          {label}
        </h3>
        {note && <p className="m-0 text-[11.5px] leading-snug text-ink-3">{note}</p>}
      </div>
      {children}
    </section>
  );
}

/** The directive as the agent receives it; wrapper lines dim, commands in the accent. */
export function Directive({ text }: { text: string }) {
  const lines = text.split("\n");
  return (
    <pre
      tabIndex={0}
      aria-label="The directive, as the agent reads it"
      className="scroll-thin m-0 max-h-56 overflow-auto rounded-md border border-line-strong bg-base p-3 font-mono text-[11.5px] leading-[1.6] whitespace-pre-wrap text-ink-2">
      {lines.map((l, i) => {
        const tone = /^\s*<\/?adage-/.test(l)
          ? "text-ink-3"
          : /^\s{2,}\S/.test(l)
            ? "text-accent-soft"
            : i === 1
              ? "text-ink-1"
              : "";
        return (
          <span key={i} className={cx("block", tone)}>
            {l || " "}
          </span>
        );
      })}
    </pre>
  );
}

function Stat({ value, of, label }: { value: ReactNode; of?: ReactNode; label: string }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <span className="display text-[34px] leading-none font-semibold">
        {value}
        {of !== undefined && <span className="text-ink-3">{of}</span>}
      </span>
      <span className="text-xs text-ink-2">{label}</span>
    </div>
  );
}

/** The lower bound against the fire bar: where the evidence stands. */
function Gauge({ lb, bar }: { lb: number; bar: number }) {
  const pct = (x: number) => `${Math.max(0, Math.min(1, x)) * 100}%`;
  return (
    <div className="flex flex-col gap-1.5" role="img" aria-label={`Lower bound ${lb.toFixed(2)} against the fire bar ${bar.toFixed(2)}`}>
      <div className="relative h-2 rounded-full bg-raised">
        <div className="absolute inset-y-0 left-0 rounded-full bg-[var(--glyph-arc)]" style={{ width: pct(lb) }} />
        <div className="absolute -top-1 h-4 w-px bg-fire" style={{ left: pct(bar) }} />
      </div>
      <div className="relative h-3.5 font-mono text-[10px] text-ink-3">
        <span className="absolute left-0">0</span>
        <span className="absolute -translate-x-1/2 whitespace-nowrap" style={{ left: pct(bar) }}>
          fire bar {bar.toFixed(2)}
        </span>
        <span className="absolute right-0">1</span>
      </div>
    </div>
  );
}

function Bar({ label, k, n, control, better }: { label: string; k: number; n: number; control?: boolean; better?: "high" | "low" }) {
  const share = n ? k / n : 0;
  return (
    <>
      <span className={cx("truncate", control ? "text-ink-3" : "text-ink-2")}>{label}</span>
      <div className="h-2 rounded-full bg-raised" title={better === "low" ? "lower is better" : undefined}>
        <div
          className="h-2 rounded-full"
          style={{ width: `${Math.max(n ? 2 : 0, share * 100)}%`, background: control ? "var(--control)" : "var(--glyph-arc)" }}
        />
      </div>
      <span className={cx("text-right font-mono tabular-nums", control ? "text-ink-2" : "text-ink-1")}>
        {k}/{n}
      </span>
    </>
  );
}

/** With the reflex (treated arm) vs control (holdout arm), in distinct traces where it applied. */
export function WithVsControl({ r }: { r: Route }) {
  const t: ArmStats = r.arms.treated ?? { sessions: 0, applied: 0, followed: 0, met: 0, hit_avoided: 0 };
  const c: ArmStats = r.arms.holdout ?? { sessions: 0, applied: 0, followed: 0, met: 0, hit_avoided: 0 };
  if (!t.applied && !c.applied) {
    return (
      <p className="m-0 text-[13px] text-ink-3">
        Not applied in any trace yet: <span className="font-mono">0/0</span> with it, <span className="font-mono">0/0</span> control.
      </p>
    );
  }
  const avoid = r.step.kind === "avoid";
  const obligation = t.met + c.met > 0;
  return (
    <div className="grid grid-cols-[112px_minmax(0,1fr)_48px] items-center gap-x-2.5 gap-y-1.5 text-xs">
      {avoid ? (
        <>
          <Bar label="hit the dead end" k={t.hit_avoided} n={t.applied} better="low" />
          <Bar label="control" k={c.hit_avoided} n={c.applied} control better="low" />
        </>
      ) : (
        <>
          <Bar label="took its path" k={t.followed} n={t.applied} />
          <Bar label="control" k={c.followed} n={c.applied} control />
        </>
      )}
      {obligation && (
        <>
          <Bar label="met the finish line" k={t.met} n={t.applied} />
          <Bar label="control" k={c.met} n={c.applied} control />
        </>
      )}
    </div>
  );
}

const STEPS: Lifecycle[] = ["nascent", "forming", "firm"];

export function LifecycleTrack({ r }: { r: Route }) {
  const l = lifecycle(r.state);
  const at = STEPS.indexOf(l);
  const off = at < 0; // dormant, weakened, extinguished
  return (
    <div className="flex flex-wrap items-center gap-2 text-xs text-ink-2">
      {STEPS.map((s, i) => {
        const reached = off ? true : i <= at;
        const current = !off && i === at;
        return (
          <span key={s} className="flex items-center gap-2">
            {i > 0 && <span className="h-px w-5 bg-line-strong" aria-hidden="true" />}
            <span
              aria-hidden="true"
              className="h-2 w-2 rounded-full"
              style={
                current && s === "firm"
                  ? { background: "var(--color-lamp-fire)", boxShadow: "0 0 8px var(--color-lamp-fire)" }
                  : reached
                    ? { background: "var(--glyph-arc)" }
                    : { border: "1.5px solid var(--color-ink-3)", boxSizing: "border-box" }
              }
            />
            <span className={current ? "text-ink-1" : ""} aria-current={current ? "step" : undefined}>
              {s}
            </span>
          </span>
        );
      })}
      {off && (
        <span className="flex items-center gap-2">
          <span className="h-px w-5 bg-line-strong" aria-hidden="true" />
          <span className="h-2 w-2 rounded-full border-[1.5px] border-ink-3" aria-hidden="true" />
          <span className="text-ink-1" aria-current="step">
            {l}
          </span>
          {r.evidence.stale_since && <span className="font-mono text-[10.5px]">since {r.evidence.stale_since}</span>}
        </span>
      )}
    </div>
  );
}

function Overview({ graph }: { graph: Graph }) {
  const [url] = useUrl();
  const changes = (graph.learning ?? [])
    .filter((l) => !url.repo || !l.repo || l.repo === url.repo)
    .flatMap((l) => (l.changes ?? []).map((c) => ({ ts: l.ts, c })))
    .slice(-6)
    .reverse();
  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-col gap-2">
        <h2 className="lbl m-0 font-normal">The map</h2>
        <p className="m-0 text-[13.5px] leading-relaxed font-light text-ink-2">
          Each arc is a reflex: a <span className="text-ink-1">stimulus</span> Adage watches for, and the{" "}
          <span className="text-ink-1">response</span> it tells the agent. Select one to see exactly what the agent reads and the
          evidence behind it.
        </p>
      </div>
      <Section label="Strength">
        <ul className="m-0 flex list-none flex-col gap-2 p-0 text-[13px] text-ink-2">
          <li className="flex items-center gap-2">
            <Lamp strength="fire" glow /> <span className="text-ink-1">fire</span> do this: strong evidence
          </li>
          <li className="flex items-center gap-2">
            <Lamp strength="prime" glow /> <span className="text-ink-1">prime</span> consider this
          </li>
          <li className="flex items-center gap-2">
            <Lamp strength="inhibit" glow /> <span className="text-ink-1">inhibit</span> don’t: a known dead end
          </li>
        </ul>
      </Section>
      {changes.length > 0 && (
        <Section label="Recently conditioned">
          <ul className="m-0 flex list-none flex-col gap-2 p-0">
            {changes.map(({ ts, c }, i) => (
              <li key={`${ts}${i}`} className="flex flex-col gap-0.5">
                <span className="font-mono text-xs text-ink-1">{c.route}</span>
                <span className="text-xs text-ink-3">{c.summary && <Prose text={c.summary} />}</span>
              </li>
            ))}
          </ul>
        </Section>
      )}
    </div>
  );
}

export function Inspector({ graph }: { graph: Graph }) {
  const [url, setUrl] = useUrl();
  const r = findRoute(graph, url.reflex);
  if (!r) {
    return (
      <aside aria-label="Inspector" tabIndex={0} className="scroll-thin flex min-h-0 flex-col gap-5 overflow-y-auto border-t border-line bg-panel p-5 lg:border-t-0 lg:border-l">
        <Overview graph={graph} />
      </aside>
    );
  }
  const s = strength(r.aspect);
  const l = lifecycle(r.state);
  const inst = isInstinct(r);
  const e = r.evidence;
  const verbs = orderedVerbs(r);
  const bar = graph.bars?.fire_lower_bound ?? 0.8;
  const traces = graph.events.filter((ev) => ev.repo === r.repo && (ev.directed.includes(r.id) || ev.would.includes(r.id) || ev.judged.some(([, id]) => id === r.id)));
  const nTraces = new Set(traces.map((t) => t.session)).size;
  const recent = traces.slice(-4).reverse();
  return (
    <aside aria-label="Inspector" className="scroll-thin flex min-h-0 flex-col gap-[18px] overflow-y-auto border-t border-line bg-panel p-5 lg:border-t-0 lg:border-l" key={routeKey(r)}>
      <div className="flex flex-col gap-2">
        <span className="lbl">{inst ? "Instinct" : "Reflex"}</span>
        <h2 className="m-0 flex min-w-0 items-center gap-2 font-mono text-[17px] font-medium">
          <KindGlyph instinct={inst} size={18} />
          <span className="truncate" title={r.id}>
            {r.id}
          </span>
        </h2>
        <div className="flex flex-wrap gap-1.5">
          <StrengthChip strength={s} hollow={l === "nascent" || l === "extinguished"} />
          <LifecycleChip lifecycle={l} />
          <Pill mono title={r.reference}>
            {r.track}@{r.version}
          </Pill>
          {r.imported && <Pill>pack</Pill>}
        </div>
        {(r.summary || r.origin) && (
          <p className="m-0 text-[13px] leading-snug text-ink-2">
            {r.summary}
            {r.origin && <span className="block text-ink-3"><Prose text={r.origin} /></span>}
          </p>
        )}
        {r.review && r.review.reasons.length > 0 && (
          <p className="m-0 rounded-md border border-prime/30 bg-prime/8 px-2.5 py-1.5 text-xs text-ink-2">
            <span className="text-ink-1">Waits for review:</span> <Prose text={r.review.reasons.map((x) => x.why).join("; ")} />
          </p>
        )}
      </div>

      <Section label="What the agent sees">
        <Directive text={r.directive} />
      </Section>

      <Section label="Evidence" source={evidenceSource(r)} note={evidenceNote(r)}>
        <div className="grid grid-cols-3 gap-2">
          <Stat value={e.support} of={`/${e.opportunities}`} label="traces took it" />
          <Stat value={e.lower_bound.toFixed(2)} label="lower bound" />
          <Stat value={Number.isInteger(e.turns_saved) ? e.turns_saved : e.turns_saved.toFixed(1)} label="turns saved" />
        </div>
        <Gauge lb={e.lower_bound} bar={bar} />
      </Section>

      <Section label={`With the ${inst ? "instinct" : "reflex"} vs control`} source={LIVE_SOURCE} note={CONTROL_NOTE}>
        <WithVsControl r={r} />
        <p className="m-0 font-mono text-[11px] text-ink-3">
          live · directed {r.live.emitted} · {judgement("followed")} {r.live.followed} · {judgement("derailed")} {r.live.derailed}
          {" · "}control would have {r.live.would_have}
        </p>
      </Section>

      <Section label="Lifecycle">
        <LifecycleTrack r={r} />
      </Section>

      {recent.length > 0 && (
        <Section label="Recent traces">
          <ul className="m-0 flex list-none flex-col gap-1 p-0 font-mono text-[11.5px] text-ink-2">
            {recent.map((t, i) => (
              <li key={`${t.ts}${i}`} className="flex gap-2">
                <span className="text-ink-3">{new Date(t.ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</span>
                <span className="truncate">{t.session.slice(0, 8)}</span>
                <span className={t.arm === "holdout" ? "text-ink-3" : "text-ink-1"}>
                  {t.arm === "holdout" ? "control" : t.directed.includes(r.id) ? "directed" : t.judged.find(([, id]) => id === r.id)?.[0] ? judgement(t.judged.find(([, id]) => id === r.id)?.[0] ?? "") : "applied"}
                </span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      <div className="grow" />
      <div className="flex flex-col gap-2">
        {verbs.slice(0, 1).map((v) => (
          <CommandButton key={v} command={reviewCommand(v, r)} why={verbWhy(v)} primary />
        ))}
        {verbs.length > 1 && (
          <details className="group">
            <summary className="cursor-pointer list-none py-1 text-xs text-ink-2 hover:text-ink-1">
              {plural(verbs.length - 1, "more decision")} ▾
            </summary>
            <div className="mt-1 flex flex-col gap-1.5">
              {verbs.slice(1).map((v) => (
                <CommandButton key={v} command={reviewCommand(v, r)} why={verbWhy(v)} />
              ))}
            </div>
          </details>
        )}
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => setUrl({ view: "traces" }, true)}
            className="flex h-9 grow items-center justify-center rounded-md border border-line-strong text-[13px] text-ink-1 hover:border-accent/45"
          >
            Open {plural(nTraces, "trace")}
          </button>
          <button
            type="button"
            onClick={() => setUrl({ focus: url.focus ? 0 : 1 })}
            aria-pressed={url.focus > 0}
            className="flex h-9 grow items-center justify-center rounded-md border border-line-strong text-[13px] text-ink-1 hover:border-accent/45"
          >
            {url.focus ? "Show everything" : "Focus on its arc"}
          </button>
        </div>
      </div>
    </aside>
  );
}
