// Evidence: what the reflexes changed, against the control arm. Every number carries its
// denominator; nothing here is estimated.

import { useMemo, useState } from "react";
import type { Graph, Route } from "../api/types";
import { KindGlyph, Lamp } from "../components/glyphs";
import { Segmented, cx, day } from "../components/ui";
import { armTraces, kpis, pct, perDay, share } from "../lib/evidence";
import { routeKey } from "../lib/graph";
import { isInstinct, lifecycle, plural, strength } from "../lib/words";
import { type Window, useUrl, windowStart } from "../state/url";

type SortKey = "saved" | "support" | "lb" | "strength" | "id";

function Chart({ data, markers }: { data: { day: number; treated: number; control: number }[]; markers: { ts: number; label: string }[] }) {
  const W = 700;
  const H = 280;
  const L = 40;
  const R = 12;
  const T = 24;
  const B = 36;
  if (data.length === 0) {
    return <p className="m-0 py-16 text-center text-sm text-ink-3">No traces in this window yet.</p>;
  }
  const max = Math.max(1, ...data.map((d) => Math.max(d.treated, d.control)));
  const top = Math.max(4, Math.ceil(max / 4) * 4);
  const x0 = data[0]?.day ?? 0;
  const x1 = Math.max(x0 + 86400e3, data[data.length - 1]?.day ?? 0);
  const x = (t: number) => L + ((t - x0) / (x1 - x0)) * (W - L - R);
  const y = (v: number) => T + (1 - v / top) * (H - T - B);
  const line = (k: "treated" | "control") => data.map((d) => `${x(d.day).toFixed(1)},${y(d[k]).toFixed(1)}`).join(" ");
  const ticks = [0, top / 4, top / 2, (3 * top) / 4, top];
  const last = data[data.length - 1];
  const desc = `Traces per day: with reflexes ${data.map((d) => d.treated).join(", ")}; control ${data.map((d) => d.control).join(", ")}.`;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full" role="img" aria-label={desc}>
      <g style={{ stroke: "var(--color-line-soft)" }}>
        {ticks.map((t) => (
          <line key={t} x1={L} x2={W - R} y1={y(t)} y2={y(t)} />
        ))}
      </g>
      <g className="font-mono" fontSize="10.5" style={{ fill: "var(--color-ink-3)" }}>
        {ticks.map((t) => (
          <text key={t} x={L - 10} y={y(t) + 4} textAnchor="end">
            {Math.round(t)}
          </text>
        ))}
        <text x={L} y={H - 12}>
          {day(x0)}
        </text>
        <text x={W - R} y={H - 12} textAnchor="end">
          {day(x1)}
        </text>
      </g>
      {markers.map((m, i) => (
        <g key={`${m.ts}${i}`}>
          <line x1={x(m.ts)} x2={x(m.ts)} y1={T} y2={H - B} style={{ stroke: "var(--color-lamp-fire)" }} strokeDasharray="2 4" />
          <text x={x(m.ts) + 6} y={T + 12 + (i % 3) * 14} fontSize="11.5" style={{ fill: "var(--color-lamp-fire)" }}>
            {m.label}
          </text>
        </g>
      ))}
      <polyline points={line("control")} fill="none" style={{ stroke: "var(--color-ink-3)" }} strokeWidth="2" strokeDasharray="5 5" />
      <polyline points={line("treated")} fill="none" style={{ stroke: "var(--color-accent-base)" }} strokeWidth="2.5" strokeLinejoin="round" />
      {data.map((d) => (
        <circle key={d.day} cx={x(d.day)} cy={y(d.treated)} r="3" style={{ fill: "var(--color-accent-base)" }} />
      ))}
      {last && (
        <text x={x(last.day) - 6} y={y(last.treated) - 10} textAnchor="end" className="font-mono" fontSize="11" style={{ fill: "var(--color-ink-1)" }}>
          {plural(last.treated, "trace")}
        </text>
      )}
    </svg>
  );
}

export function Evidence({ graph }: { graph: Graph }) {
  const [url, setUrl] = useUrl();
  const [sort, setSort] = useState<{ key: SortKey; desc: boolean }>({ key: "saved", desc: true });
  const now = Math.max(Date.now(), ...graph.events.map((e) => e.ts));
  const routes = graph.routes.filter((r) => (!url.repo || r.repo === url.repo) && lifecycle(r.state) !== "extinguished");
  const events = graph.events.filter((e) => (!url.repo || e.repo === url.repo) && e.ts >= windowStart(url.window, now));
  const ks = kpis(routes);
  const arms = armTraces(graph, events);
  const days = perDay(events);
  const markers = (graph.learning ?? [])
    .filter((l) => l.ts >= (days[0]?.day ?? Infinity) && (!url.repo || !l.repo || l.repo === url.repo))
    .flatMap((l) =>
      (l.changes ?? [])
        .filter((c) => c.change !== "evidence" && (c.to === "green" || c.to === "fire"))
        .map((c) => ({ ts: l.ts, label: `${c.route} fires` })),
    )
    .slice(-3);
  const path = ks[0];
  const tShare = path ? share(path.treated) : null;
  const cShare = path ? share(path.control) : null;

  const rows = useMemo(() => {
    const val = (r: Route): number | string => {
      switch (sort.key) {
        case "saved":
          return r.evidence.turns_saved;
        case "support":
          return r.evidence.support;
        case "lb":
          return r.evidence.lower_bound;
        case "strength":
          return ["fire", "prime", "inhibit"].indexOf(strength(r.aspect) ?? "");
        default:
          return r.id;
      }
    };
    return [...routes].sort((a, b) => {
      const va = val(a);
      const vb = val(b);
      const c = typeof va === "number" && typeof vb === "number" ? va - vb : String(va).localeCompare(String(vb));
      return (sort.desc ? -c : c) || a.id.localeCompare(b.id);
    });
  }, [routes, sort]);

  const th = (key: SortKey, label: string) => (
    <th
      scope="col"
      aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : "none"}
      className="border-b border-line-strong px-3 py-2.5 text-left font-mono text-[10.5px] font-medium tracking-[0.12em] text-ink-3 uppercase"
    >
      <button
        type="button"
        className="uppercase hover:text-ink-1"
        onClick={() => setSort((s) => ({ key, desc: s.key === key ? !s.desc : key !== "id" }))}
      >
        {label}
        {sort.key === key ? (sort.desc ? " ↓" : " ↑") : ""}
      </button>
    </th>
  );

  return (
    <main aria-label="Evidence" className="scroll-thin flex min-h-0 grow flex-col gap-[22px] overflow-y-auto px-10 py-7">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="lbl">
            Evidence · {plural(arms.treated, "trace")} with reflexes · {arms.control} control
          </span>
          <h1 className="display m-0 text-[44px] leading-[1.05] font-semibold">
            {tShare !== null && cShare !== null ? (
              <>
                With reflexes, agents took the known path in <span className="text-accent-soft">{pct(path!.treated)}</span> of traces.
              </>
            ) : tShare !== null ? (
              <>
                Agents took the known path in <span className="text-accent-soft">{pct(path!.treated)}</span> of traces.
              </>
            ) : (
              <>
                Evidence builds <span className="text-accent-soft">with every trace.</span>
              </>
            )}
          </h1>
        </div>
        <div className="flex items-center gap-3">
          <span className="font-mono text-xs text-ink-3">distinct traces · control is the holdout arm</span>
          <Segmented<Window>
            label="Window"
            value={url.window === "24h" ? "7d" : url.window}
            onChange={(w) => setUrl({ window: w })}
            options={[
              { value: "7d", label: "7 d" },
              { value: "30d", label: "30 d" },
              { value: "all", label: "All" },
            ]}
          />
        </div>
      </div>

      <div className="grid grid-cols-2 gap-4 xl:grid-cols-4">
        {ks.map((k) => (
          <div key={k.label} className="flex flex-col gap-2 rounded-lg border border-line-strong bg-panel px-5 py-[18px]">
            <span className="text-[13px] text-ink-2">{k.label}</span>
            <div className="flex items-baseline gap-2.5">
              <span className="display text-[52px] leading-none font-semibold text-ink-1">{pct(k.treated)}</span>
              <span className="font-mono text-[13px] text-ink-2">vs {pct(k.control)} control</span>
            </div>
            <span className="font-mono text-[11.5px] text-ink-3">
              {k.treated.k}/{k.treated.n} · control {k.control.k}/{k.control.n} · {k.note}
            </span>
          </div>
        ))}
        <div className="flex flex-col gap-2 rounded-lg border border-line-strong bg-panel px-5 py-[18px]">
          <span className="text-[13px] text-ink-2">Traces in this window</span>
          <div className="flex items-baseline gap-2.5">
            <span className="display text-[52px] leading-none font-semibold text-ink-1">{arms.treated}</span>
            <span className="font-mono text-[13px] text-ink-2">vs {arms.control} control</span>
          </div>
          <span className="font-mono text-[11.5px] text-ink-3">{graph.stop_requests} finish-line requests · all time</span>
        </div>
      </div>

      <div className="grid min-h-0 grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)]">
        <section aria-label="Traces per day" className="flex flex-col gap-3 rounded-lg border border-line-strong bg-panel px-5 py-[18px]">
          <div className="flex items-center justify-between">
            <span className="lbl">Traces per day · {url.window === "all" ? "all time" : `last ${url.window.replace("d", " days")}`}</span>
            <div className="flex gap-4 text-[12.5px] text-ink-2">
              <span className="flex items-center gap-1.5">
                <span className="h-[2.5px] w-3.5 rounded-sm bg-accent" />
                with reflexes
              </span>
              <span className="flex items-center gap-1.5">
                <span className="w-3.5 border-t-2 border-dashed border-ink-3" />
                control
              </span>
            </div>
          </div>
          <Chart data={days} markers={markers} />
        </section>

        <section aria-label="Reflexes by turns saved" className="flex flex-col overflow-hidden rounded-lg border border-line-strong bg-panel px-2 pt-2">
          <div className="px-3 pt-2.5 pb-1">
            <span className="lbl">Reflexes by {sort.key === "saved" ? "turns saved" : sort.key === "lb" ? "lower bound" : sort.key}</span>
          </div>
          <div className="scroll-thin overflow-x-auto">
            <table className="w-full border-collapse">
              <thead>
                <tr>
                  {th("id", "Reflex")}
                  {th("strength", "Strength")}
                  {th("support", "Support")}
                  {th("lb", "LB")}
                  {th("saved", "Saved")}
                  <th scope="col" className="border-b border-line-strong px-3 py-2.5 text-left font-mono text-[10.5px] font-medium tracking-[0.12em] text-ink-3 uppercase">
                    With · control
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const s = strength(r.aspect);
                  const t = r.arms.treated;
                  const c = r.arms.holdout;
                  const avoid = r.step.kind === "avoid";
                  const k = (a: typeof t) => (a ? (avoid ? a.hit_avoided : a.followed) : 0);
                  return (
                    <tr key={routeKey(r)} className="hover:bg-raised">
                      <td className="border-b border-line px-3 py-[11px]">
                        <button
                          type="button"
                          onClick={() => setUrl({ reflex: routeKey(r), view: "map" }, true)}
                          className="flex items-center gap-2 text-left font-mono text-[12.5px] text-ink-1 hover:text-accent-soft"
                        >
                          <KindGlyph instinct={isInstinct(r)} />
                          {r.id}
                        </button>
                      </td>
                      <td className="border-b border-line px-3 py-[11px] text-[13.5px] text-ink-2">
                        <span className="inline-flex items-center gap-1.5">
                          <Lamp strength={s} hollow={lifecycle(r.state) === "nascent"} />
                          {s}
                        </span>
                      </td>
                      <td className="border-b border-line px-3 py-[11px] font-mono text-[12.5px] text-ink-2">
                        {r.evidence.support}/{r.evidence.opportunities}
                      </td>
                      <td className="border-b border-line px-3 py-[11px] font-mono text-[12.5px] text-ink-2">{r.evidence.lower_bound.toFixed(2)}</td>
                      <td className="border-b border-line px-3 py-[11px] font-mono text-[12.5px] text-ink-1">{r.evidence.turns_saved.toFixed(1)}</td>
                      <td className={cx("border-b border-line px-3 py-[11px] font-mono text-[12px] whitespace-nowrap", "text-ink-2")} title={avoid ? "hit the dead end" : "took its path"}>
                        {t?.applied ? `${k(t)}/${t.applied}` : "—"} · {c?.applied ? `${k(c)}/${c.applied}` : "—"}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </section>
      </div>
    </main>
  );
}
