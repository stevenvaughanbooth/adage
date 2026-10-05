// The savings estimate (crates/adage-cli/src/savings.rs) and what the PreToolUse hook changed, per
// repository. Evidence, not a celebration: amounts stay in the default ink with their interval
// and denominators; only the verdict carries a mark (style guide, Colour). Below the gate (10
// control, 30 treated sessions) there are counts and progress, never dollars.

import type { ReactNode } from "react";
import type { Estimate, Graph, PretoolCounts, Savings } from "../api/types";
import { plural, usd, verdictWord } from "../lib/words";
import { RewriteGlyph } from "./glyphs";
import { CommandButton, cx } from "./ui";

function Mark({ verdict }: { verdict: Savings["verdict"] }) {
  const [glyph, tone] =
    verdict === "saved" ? ["✓", "text-fire"] : verdict === "adds_cost" ? ["✗", "text-inhibit"] : ["○", "text-ink-3"];
  return (
    <span className="inline-flex items-center gap-1.5 text-[13px] text-ink-1" data-testid="savings-verdict">
      <span aria-hidden="true" className={tone}>
        {glyph}
      </span>
      {verdictWord(verdict)}
    </span>
  );
}

/** Progress toward the gate: k of n, as a track in the de-emphasis ink. */
function Meter({ k, n, label }: { k: number; n: number; label: string }) {
  const share = n ? Math.min(1, k / n) : 0;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline gap-1.5">
        <span className="display text-[34px] leading-none font-semibold text-ink-1">{Math.min(k, n)}</span>
        <span className="font-mono text-[13px] text-ink-3">of {n}</span>
      </div>
      <div className="h-1.5 rounded-full bg-raised" role="img" aria-label={`${label}: ${k} of ${n}`}>
        <div className="h-1.5 rounded-full bg-[var(--control)]" style={{ width: `${share * 100}%` }} />
      </div>
      <span className="text-xs text-ink-2">{label}</span>
    </div>
  );
}

function Tile({ label, children, note }: { label: string; children: ReactNode; note?: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-1.5 rounded-lg border border-line-strong bg-base px-4 py-3.5">
      <span className="text-[12.5px] text-ink-2">{label}</span>
      {children}
      {note && <span className="font-mono text-[11px] leading-snug text-ink-3">{note}</span>}
    </div>
  );
}

/** One outcome past the gate, worded as `adage savings` words it. */
function Amount({ e, dollars, verdict }: { e: Estimate; dollars: boolean; verdict: Savings["verdict"] }) {
  const f = (x: number) => (dollars ? usd(x) : `${Math.abs(x).toFixed(1)}`);
  const unit = !dollars && <span className="text-[18px] text-ink-2"> turns</span>;
  const interval =
    verdict === "saved"
      ? `95% CI ${f(e.lo)}–${f(e.hi)}`
      : verdict === "adds_cost"
        ? `95% CI ${f(-e.hi)}–${f(-e.lo)}`
        : `95% CI crosses zero`;
  return (
    <>
      <span className="display text-[34px] leading-none font-semibold text-ink-1">
        {verdict === "saved" || verdict === "adds_cost" ? `~${f(e.delta)}` : `±${f((e.hi - e.lo) / 2)}`}
        {unit}
      </span>
      <span className="font-mono text-[11px] text-ink-3">
        per session · {interval} · {e.n_treated} vs {e.n_control} quiet
      </span>
    </>
  );
}

function Rewrites({ p, on }: { p: PretoolCounts | undefined; on: boolean | undefined }) {
  const n = p?.rewritten ?? 0;
  return (
    <Tile
      label="Rewrites to the project’s environment"
      note={
        <>
          {p?.advised ?? 0} advised instead · {p?.withheld ?? 0} withheld in control
        </>
      }
    >
      <span className="flex items-baseline gap-2">
        <span className="text-accent-soft">
          <RewriteGlyph size={16} />
        </span>
        <span className="display text-[34px] leading-none font-semibold text-ink-1" data-testid="rewrites-count">
          {n}
        </span>
        <span className="font-mono text-[12px] text-ink-3">{on === false ? "rewrites off" : "rewrites on"}</span>
      </span>
    </Tile>
  );
}

function One({ s, p, on }: { s: Savings; p: PretoolCounts | undefined; on: boolean | undefined }) {
  const c = s.counts;
  const cost = s.cost_usd ?? null;
  const turns = s.model_turns ?? null;
  return (
    <section aria-label={`Savings · ${s.repo}`} className="flex flex-col gap-3.5 rounded-lg border border-line-strong bg-panel px-5 py-[18px]">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="lbl">
          Savings · {s.repo} · vs sessions where Adage stayed quiet
        </span>
        <Mark verdict={s.verdict} />
      </div>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {!s.gate_met ? (
          <>
            <Tile label="Quiet sessions (control)">
              <Meter k={s.n_control} n={s.need.control} label="Adage stays quiet in 1 of 10 so its effect can be measured" />
            </Tile>
            <Tile label="Sessions with Adage">
              <Meter k={s.n_treated} n={s.need.treated} label="finished sessions with reflexes" />
            </Tile>
          </>
        ) : (
          <>
            <Tile label={s.verdict === "adds_cost" ? "Adds per session" : s.verdict === "saved" ? "Saves per session" : "Per session"} note={cost ? `API-list equivalent, priced as of ${s.pricing_as_of ?? "—"}` : "no dollar figure: model not in the price table"}>
              {cost ? <Amount e={cost} dollars verdict={s.verdict} /> : turns ? <Amount e={turns} dollars={false} verdict={s.verdict} /> : <span className="text-ink-3">—</span>}
            </Tile>
            <Tile label="Ended asking you for help">
              <span className="display text-[34px] leading-none font-semibold text-ink-1">
                {Math.round((s.asked_for_help_treated ?? 0) * 100)}%
              </span>
              <span className="font-mono text-[11px] text-ink-3">with Adage · {Math.round((s.asked_for_help_control ?? 0) * 100)}% without</span>
            </Tile>
          </>
        )}
        <Tile label={`Last ${c.days} days`} note={`${plural(c.directives, "directive")} · ${c.followed} followed`}>
          <span className="flex items-baseline gap-2">
            <span className="display text-[34px] leading-none font-semibold text-ink-1">{c.sessions}</span>
            <span className="font-mono text-[12px] text-ink-3">{c.sessions === 1 ? "session" : "sessions"}</span>
          </span>
          <span className="font-mono text-[11px] text-ink-3">
            {plural(c.dead_ends_headed_off, "dead end")} headed off · {plural(c.finish_lines_met, "finish line")} met
          </span>
        </Tile>
        <Rewrites p={p} on={on} />
      </div>
      {s.lines.map((l, i) => (
        <p key={i} className={cx("m-0 font-mono text-[11.5px] leading-snug", i ? "text-ink-3" : "text-ink-2")}>
          {l.trim()}
        </p>
      ))}
      {s.guardrail && <p className="m-0 font-mono text-[11.5px] text-ink-3">{s.guardrail}</p>}
      {on === false && <CommandButton command="adage delegate rewrites on" why="rewrites are off; this turns them back on" />}
    </section>
  );
}

/** The estimate for the repository in view (every repository when none is picked). */
export function SavingsPanel({ graph, repo }: { graph: Graph; repo: string | null }) {
  const all = graph.savings.filter((s) => !repo || s.repo === repo);
  const on = graph.runtime?.rewrite.on;
  if (!all.length) return null;
  return (
    <div className="flex flex-col gap-4">
      {all.map((s) => (
        <One key={s.repo} s={s} p={graph.pretool.find((p) => p.repo === s.repo)} on={on} />
      ))}
    </div>
  );
}
