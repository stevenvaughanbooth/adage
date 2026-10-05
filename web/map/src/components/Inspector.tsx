// The right inspector: the exact directive, its evidence, with vs control, its lifecycle, and the
// commands a person runs to decide about it. The page itself never changes anything.

import type { ReactNode } from "react";
import type { ArmStats, Card, CardCommand, Graph, Route, Uplift } from "../api/types";
import { MeasurementLinks } from "./MeasurementLinks";
import { relates } from "../lib/traces";
import { findRoute, routeKey } from "../lib/graph";
import {
  CONTROL_NOTE,
  LIVE_SOURCE,
  type Lifecycle,
  UPLIFT_MIN,
  evidenceNote,
  evidenceSource,
  isCard,
  isInstinct,
  isQuiet,
  judgement,
  lifecycle,
  orderedVerbs,
  plural,
  reviewCommand,
  signed,
  strength,
  verbWhy,
} from "../lib/words";
import { useUrl } from "../state/url";
import { CardGlyph, KindGlyph, Lamp, QuietGlyph } from "./glyphs";
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

function Bar({ label, k, n, control, better }: { label: string; k: number | null; n: number; control?: boolean; better?: "high" | "low" }) {
  const share = n && k !== null ? k / n : 0;
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
        {k === null ? "unavailable" : `${k}/${n}`}
      </span>
    </>
  );
}

/** With the reflex (treated arm) vs the control arm, in distinct traces where it applied. */
export function WithVsControl({ r }: { r: Route }) {
  const t: ArmStats = r.arms.treated ?? { sessions: 0, applied: 0, followed: 0, met: 0, hit_avoided: 0 };
  const c: ArmStats = r.arms.control ?? { sessions: 0, applied: 0, followed: 0, met: 0, hit_avoided: 0 };
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

/**
 * Uplift: how often agents take the step when told (p_follow) against unaided (p_self), on one
 * 0–1 scale. The gap is the uplift; the bar it must clear is marked beside the numbers.
 */
export function UpliftBar({ u, bar }: { u: Uplift; bar: number }) {
  const pct = (x: number) => `${Math.max(0, Math.min(1, x)) * 100}%`;
  const lo = Math.min(u.p_self, u.p_follow);
  const hi = Math.max(u.p_self, u.p_follow);
  return (
    <div className="flex flex-col gap-1.5">
      <div
        className="relative h-2 rounded-full bg-raised"
        role="img"
        aria-label={`Followed ${u.p_follow.toFixed(2)} when told, ${u.p_self.toFixed(2)} unaided: uplift ${signed(u.mean)} against the ${bar.toFixed(2)} bar`}
      >
        <div className="absolute inset-y-0 rounded-full bg-[var(--sel)]" style={{ left: pct(lo), width: `calc(${pct(hi)} - ${pct(lo)})` }} />
        <div className="absolute -top-1 h-4 w-[3px] -translate-x-1/2 rounded-sm bg-[var(--control)]" style={{ left: pct(u.p_self) }} />
        <div className="absolute -top-1 h-4 w-[3px] -translate-x-1/2 rounded-sm bg-[var(--glyph-arc)]" style={{ left: pct(u.p_follow) }} />
      </div>
      <div className="flex flex-wrap justify-between gap-x-3 font-mono text-[10.5px] text-ink-3">
        <span>
          unaided <span className="text-ink-2">{u.p_self.toFixed(2)}</span> · told <span className="text-ink-2">{u.p_follow.toFixed(2)}</span>
        </span>
        <span>
          uplift <span className="text-ink-1">{signed(u.mean)}</span> (lb {signed(u.lower)}) · bar {bar.toFixed(2)}
        </span>
      </div>
    </div>
  );
}

/** A quiet reflex: why it is not delivered, and the one command that delivers it anyway. */
function QuietPanel({ r, bar }: { r: Route; bar: number }) {
  const u = r.evidence.uplift;
  return (
    <section aria-label="Quiet" className="flex flex-col gap-2.5 rounded-md border border-line-strong bg-base px-3 py-2.5">
      <h3 className="m-0 flex items-center gap-2 text-[13px] font-medium text-ink-1">
        <QuietGlyph size={10} />
        Quiet: agents already do this
      </h3>
      <p className="m-0 text-xs leading-snug text-ink-2">
        {u ? (
          <>
            Telling the agent changes too little: uplift <span className="font-mono text-ink-1">{signed(u.mean)}</span> is below the{" "}
            <span className="font-mono text-ink-1">{bar.toFixed(2)}</span> bar, so Adage does not deliver it. It is not an install step; it waits for
            you.
          </>
        ) : (
          <>Its uplift is below the bar, so Adage does not deliver it. It is not an install step; it waits for you.</>
        )}
      </p>
      {u && <UpliftBar u={u} bar={bar} />}
      {u?.line && (
        <p className="m-0 font-mono text-[11px] leading-snug text-ink-3">
          <Prose text={u.line} />
        </p>
      )}
      <CommandButton command={reviewCommand("approve", r)} why="deliver it anyway" />
    </section>
  );
}

function Verified({ c }: { c: CardCommand }) {
  if (c.verified === true) return <span className="text-fire" title="Replayed on a clean checkout (adage train cards): it worked">✓ verified</span>;
  if (c.verified === false) return <span className="text-inhibit" title="Replayed on a clean checkout: it failed">✗ failed on a clean checkout</span>;
  return <span className="text-ink-3" title="Not replayed yet: adage train cards">not replayed</span>;
}

/** A procedure card: files in order, commands that worked, commands to avoid, the finishing check, delivery. */
export function CardView({ card }: { card: Card }) {
  return (
    <div className="flex flex-col gap-3 text-xs">
      <p className="m-0 text-ink-2">
        From <span className="font-mono text-ink-1">{plural(card.sessions, "trace")}</span> of this family,{" "}
        <span className="font-mono text-ink-1">{card.passed}</span> passed.
      </p>
      <div className="flex flex-col gap-1">
        <span className="lbl">Files, in order</span>
        <ol className="m-0 flex list-decimal flex-col gap-0.5 pl-5 font-mono text-[11.5px] text-ink-1">
          {card.files.map((f) => (
            <li key={f.path}>
              {f.path} <span className="text-ink-3">· {f.sessions}/{card.sessions}</span>
            </li>
          ))}
        </ol>
      </div>
      {card.commands.length > 0 && (
        <div className="flex flex-col gap-1">
          <span className="lbl">Commands that worked</span>
          <ul className="m-0 flex list-none flex-col gap-1 p-0">
            {card.commands.map((c) => (
              <li key={c.command} className="flex flex-wrap items-baseline gap-x-2">
                <code className="font-mono text-[11.5px] text-accent-soft">{c.command}</code>
                <span className="font-mono text-[10.5px] text-ink-3">
                  {c.runs - c.failed}/{c.runs} runs ·
                </span>
                <span className="text-[11px]">
                  <Verified c={c} />
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {card.avoid.length > 0 && (
        <div className="flex flex-col gap-1">
          <span className="lbl">Mostly failed</span>
          <ul className="m-0 flex list-none flex-col gap-1 p-0">
            {card.avoid.map((c) => (
              <li key={c.command} className="flex flex-col">
                <code className="font-mono text-[11.5px] text-ink-2 line-through decoration-inhibit">{c.command}</code>
                {c.error && <span className="font-mono text-[10.5px] text-ink-3">{c.error}</span>}
              </li>
            ))}
          </ul>
        </div>
      )}
      {card.finish && (
        <p className="m-0 text-ink-2">
          Finish with <code className="font-mono text-[11.5px] text-accent-soft">{card.finish}</code>
        </p>
      )}
      {card.threshold != null && (
        <p className="m-0 text-ink-3">
          Delivered once a session: at a prompt at least <span className="font-mono text-ink-2">{card.threshold.toFixed(3)}</span> like the family’s
          {card.grey_from != null && (
            <>
              , or from <span className="font-mono text-ink-2">{card.grey_from.toFixed(3)}</span> (the grey zone) at the first edit of one of its files
            </>
          )}
          .
        </p>
      )}
      <p className="m-0 text-ink-3">Prime at most, never strengthened. Never shared: a card is first-hand only.</p>
    </div>
  );
}

const STEPS: Lifecycle[] = ["nascent", "forming", "firm"];

export function LifecycleTrack({ r }: { r: Route }) {
  const l = lifecycle(r.state);
  const at = STEPS.indexOf(l);
  const off = at < 0; // dormant, extinguished
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
          {r.evidence.dormant_since && <span className="font-mono text-[10.5px]">since {r.evidence.dormant_since}</span>}
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
      <aside id="reflex-inspector" aria-label="Inspector" tabIndex={0} className="scroll-thin flex min-h-0 flex-col gap-5 overflow-y-auto border-t border-line bg-panel p-5 lg:border-t-0 lg:border-l">
        <Overview graph={graph} />
      </aside>
    );
  }
  const s = strength(r.strength);
  const l = lifecycle(r.state);
  const inst = isInstinct(r);
  const e = r.evidence;
  const executionOnly = Boolean(r.execution_only);
  const verbs = orderedVerbs(r);
  const bar = graph.bars?.fire_lower_bound ?? 0.8;
  const upliftBar = graph.bars?.uplift_min ?? UPLIFT_MIN;
  const quiet = isQuiet(r);
  const card = isCard(r) ? r.step.card : undefined;
  const why = e.why;
  const waits = (r.review?.reasons ?? []).filter((x) => !(quiet && x.kind === "quiet"));
  const traces = graph.events.filter((ev) => relates(ev, r));
  const nTraces = new Set(traces.map((t) => t.session)).size;
  const recent = traces.slice(-4).reverse();
  return (
    <aside id="reflex-inspector" aria-label="Inspector" tabIndex={-1} className="scroll-thin flex min-h-0 flex-col gap-[18px] overflow-y-auto border-t border-line bg-panel p-5 lg:border-t-0 lg:border-l" key={routeKey(r)}>
      <div className="flex flex-col gap-2">
        <span className="lbl">{inst ? "Instinct" : card ? "Reflex · procedure card" : "Reflex"}</span>
        <h2 className="m-0 flex min-w-0 items-center gap-2 font-mono text-[17px] font-medium">
          <KindGlyph instinct={inst} card={Boolean(card)} size={18} />
          <span className="truncate" title={r.id}>
            {r.id}
          </span>
        </h2>
        <div className="flex flex-wrap gap-1.5">
          {executionOnly ? <Pill>CPU execution candidate</Pill> : <StrengthChip strength={s} hollow={l === "nascent" || l === "extinguished"} />}
          <LifecycleChip lifecycle={l} />
          {quiet && (
            <span className="inline-flex h-6 items-center gap-1.5 rounded-full border border-line-strong px-2.5 text-xs text-ink-1">
              <QuietGlyph size={9} />
              quiet
            </span>
          )}
          {card && (
            <span className="inline-flex h-6 items-center gap-1.5 rounded-full border border-line-strong px-2.5 text-xs text-ink-2" title="A procedure card never travels in a pack">
              <CardGlyph size={12} />
              never shared
            </span>
          )}
          <Pill mono title={r.reference}>
            {r.track}@{r.version}
          </Pill>
          {r.imported && <Pill>pack</Pill>}
        </div>
        {(r.summary || r.origin.label) && (
          <p className="m-0 text-[13px] leading-snug text-ink-2">
            {r.summary}
            {r.origin.label && <span className="block text-ink-3"><Prose text={r.origin.label} /></span>}
          </p>
        )}
        {waits.length > 0 && (
          <p className="m-0 rounded-md border border-prime/30 bg-prime/8 px-2.5 py-1.5 text-xs text-ink-2">
            <span className="text-ink-1">Waits for review:</span> <Prose text={waits.map((x) => x.why).join("; ")} />
          </p>
        )}
      </div>

      {quiet && !executionOnly && <QuietPanel r={r} bar={upliftBar} />}

      {r.trigger.kind === "context" && r.trigger.items && r.trigger.items.length > 0 && (
        <Section label="Stimulus: a run of events" note={r.trigger.within ? `Each within ${plural(r.trigger.within, "event")} of the one before; the last one fires it.` : undefined}>
          <ol className="m-0 flex list-decimal flex-col gap-0.5 pl-5 text-xs text-ink-2">
            {r.trigger.items.map((it, i) => (
              <li key={`${it.kind}${i}`}>
                <Prose text={it.label} />
              </li>
            ))}
          </ol>
        </Section>
      )}

      {executionOnly ? (
        <Section label="CPU execution" note="This routine stays silent without a valid local execution grant. Evidence approval alone does not authorize it to run.">
          <code className="break-words text-xs text-accent-soft">{r.step.label}</code>
          <CommandButton command="adage delegate status" why="Check current execution authority from this repository." />
        </Section>
      ) : (
        <Section label={quiet ? "What the agent would see" : "What the agent sees"}>
          <Directive text={r.directive ?? ""} />
        </Section>
      )}

      {card && (
        <Section label="Procedure card">
          <CardView card={card} />
        </Section>
      )}

      {r.step.kind === "sequence" && r.step.commands && r.step.commands.length > 0 && (
        <Section label="Steps in order">
          <ol className="m-0 flex list-decimal flex-col gap-0.5 pl-5 font-mono text-[11.5px] text-accent-soft">
            {r.step.commands.map((c, i) => (
              <li key={`${c}${i}`}>{c}</li>
            ))}
          </ol>
        </Section>
      )}

      <Section label="Evidence" source={evidenceSource(r)} note={executionOnly ? "Immediate successful actions on a later model turn. These counts establish a recurring routine, not avoided work or permission." : evidenceNote(r)}>
        {executionOnly && e.execution ? (
          <div className="grid grid-cols-3 gap-2">
            <Stat value={e.execution.successful} of={`/${e.execution.opportunities}`} label="supported continuations" />
            <Stat value={e.execution.sessions} label="unsteered sessions" />
            <Stat value="—" label="savings not established" />
          </div>
        ) : card ? (
          <div className="grid grid-cols-3 gap-2">
            <Stat value={card.passed} of={`/${card.sessions}`} label="traces passed" />
            <Stat value={card.files.length} label="files" />
            <Stat value={card.commands.filter((c) => c.verified === true).length} of={`/${card.commands.length}`} label="verified" />
          </div>
        ) : e.basis === "value" && why ? (
          <>
            <div className="grid grid-cols-3 gap-2">
              <Stat value={`~${why.gain.toFixed(2)}`} label="calls saved" />
              <Stat value={why.conf.toFixed(2)} label="P(saves)" />
              <Stat value={why.conf_lb.toFixed(2)} label="90% lower bound" />
            </div>
            <p className="m-0 font-mono text-[11px] text-ink-3">
              {e.opportunities} triggers · {e.support} took the step{e.support === 0 ? " · a desire path: its strength rests on the value estimate" : ""}
            </p>
          </>
        ) : (
          <>
            <div className="grid grid-cols-3 gap-2">
              <Stat value={e.support} of={`/${e.opportunities}`} label="traces took it" />
              <Stat value={e.lower_bound.toFixed(2)} label="lower bound" />
              <Stat value={Number.isInteger(e.turns_saved) ? e.turns_saved : e.turns_saved.toFixed(1)} label="turns saved" />
            </div>
            <Gauge lb={e.lower_bound} bar={bar} />
          </>
        )}
        {e.describe && !executionOnly && (
          <p className="m-0 font-mono text-[11px] leading-snug text-ink-3" data-testid="evidence-describe">
            <Prose text={e.describe} />
          </p>
        )}
        {e.effect_evidence && (
          <p className="m-0 text-[11.5px] text-ink-3">
            Classified observe from the traces: <Prose text={e.effect_evidence} />
          </p>
        )}
        {e.outcome_lift && e.outcome_lift.with > 0 && (
          <p className="m-0 font-mono text-[11px] text-ink-3">
            outcome lift {signed(e.outcome_lift.diff)}: {e.outcome_lift.with_passed}/{e.outcome_lift.with} passed with the step,{" "}
            {e.outcome_lift.without_passed}/{e.outcome_lift.without} without
          </p>
        )}
      </Section>

      {!executionOnly && !quiet && e.uplift && (
        <Section label="Uplift" note="How much more often agents take the step when told than unaided: what delivering it changes.">
          <UpliftBar u={e.uplift} bar={upliftBar} />
        </Section>
      )}

      {r.obligation && (
        <Section label="Obligation">
          <p className="m-0 text-xs leading-snug text-ink-2">
            <Prose text={r.obligation} />
          </p>
        </Section>
      )}

      <Section label="vs control" source={LIVE_SOURCE} note={CONTROL_NOTE}>
        <MeasurementLinks route={r} />
        <p className="m-0 font-mono text-[11px] text-ink-3">
          live · directed {r.live.emitted} · {judgement("followed")} {r.live.followed} · {judgement("ignored")} {r.live.ignored}
          {r.live.misfired ? ` · ${judgement("misfired")} ${r.live.misfired}` : ""}
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
                <button type="button" className="truncate text-accent-soft underline" onClick={() => setUrl({ view: "traces", session: t.session, traceRepo: t.repo, sequence: t.sequence ?? null }, true)}>{t.session.slice(0, 8)}</button>
                <span className={t.arm === "control" ? "text-ink-3" : "text-ink-1"}>
                  {t.arm === "control" ? "control" : t.directed.includes(r.id) ? "directed" : t.judged.find(([, id]) => id === r.id)?.[0] ? judgement(t.judged.find(([, id]) => id === r.id)?.[0] ?? "") : "applied"}
                </span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      <div className="grow" />
      <div className="flex flex-col gap-2">
        {executionOnly && r.review?.next && (
          <CommandButton command={r.review.next} why="Review and approve the exact automatic action at your terminal." primary />
        )}
        {verbs.slice(0, 1).map((v) => (
          <CommandButton key={v} command={reviewCommand(v, r)} why={executionOnly && v === "approve" ? "Accept the evidence; this does not grant execution permission." : verbWhy(v)} primary={!executionOnly} />
        ))}
        {verbs.length > 1 && (
          <details className="group">
            <summary className="cursor-pointer list-none py-1 text-xs text-ink-2 hover:text-ink-1">
              {plural(verbs.length - 1, "more decision")} ▾
            </summary>
            <div className="mt-1 flex flex-col gap-1.5">
              {verbs.slice(1).map((v) => (
                <CommandButton key={v} command={reviewCommand(v, r)} why={executionOnly && v === "approve" ? "Accept the evidence; this does not grant execution permission." : verbWhy(v)} />
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
