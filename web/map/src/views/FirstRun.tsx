// The first run: no traces yet. What is already on, what to run, and the map it becomes the
// moment the first trace lands (the stream refreshes the graph).

import { useEffect, useState } from "react";
import type { Connection } from "../api/client";
import { loadHealth } from "../api/client";
import type { Graph, Health } from "../api/types";
import { InstinctGlyph, Icons } from "../components/glyphs";
import { StrengthChip, cx } from "../components/ui";
import { useUrl } from "../state/url";
import { isInstinct, plural, strength } from "../lib/words";

function Step({ done, label, detail, last }: { done: boolean; label: string; detail: string; last?: boolean }) {
  return (
    <li className={cx("flex items-center gap-3.5 px-[18px] py-4", !last && "border-b border-line")}>
      {done ? (
        <span className="flex h-[22px] w-[22px] items-center justify-center rounded-full bg-fire/14 text-fire">
          <Icons.check size={12} />
          <span className="sr-only">done</span>
        </span>
      ) : (
        <span className="flex h-[22px] w-[22px] items-center justify-center rounded-full border-[1.5px] border-[var(--glyph-arc)]">
          <span className="live-dot h-2 w-2 rounded-full bg-[var(--glyph-arc)] shadow-[0_0_10px_var(--glyph-arc)]" />
          <span className="sr-only">waiting</span>
        </span>
      )}
      <span className="grow text-[15px]">{label}</span>
      <span className="text-right font-mono text-xs text-ink-2">{detail}</span>
    </li>
  );
}

export function FirstRun({ graph, connection }: { graph: Graph; connection: Connection }) {
  const [, setUrl] = useUrl();
  const [health, setHealth] = useState<Health | null>(null);
  const [healthError, setHealthError] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const [checking, setChecking] = useState(true);
  useEffect(() => {
    let active = true;
    setChecking(true);
    setHealthError(false);
    loadHealth(true).then(
      (result) => { if (active) { setHealth(result); setChecking(false); } },
      () => { if (active) { setHealthError(true); setChecking(false); } },
    );
    return () => { active = false; };
  }, [attempt, connection]);
  const hooks = health?.checks.filter((c) => c.check.toLowerCase().startsWith("hooks")) ?? [];
  // A harness that is not installed passes with "nothing to check": that is not a hook.
  const installed = hooks.filter((c) => c.status === "pass" && !/nothing to check|not set up|not installed|not registered/i.test(c.detail));
  const hooksOk = installed.length > 0 && hooks.every((c) => c.status === "pass");
  const receipts = health?.checks.filter((c) => c.check.startsWith("hook activity:") && c.status === "pass") ?? [];
  const instincts = graph.routes.filter(isInstinct);
  const tracks = [...new Set(instincts.map((r) => `${r.track}@${r.version}`))];
  const port = typeof location !== "undefined" ? location.host : "127.0.0.1:8765";
  return (
    <main aria-label="First run" className="canvas-grid relative flex min-h-0 grow flex-col overflow-hidden">
      <svg className="pointer-events-none absolute inset-0 h-full w-full" viewBox="0 0 1440 844" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
        <g opacity="0.5" fill="none" style={{ stroke: "color-mix(in oklab, var(--color-accent-soft) 35%, transparent)" }} strokeWidth="1.2" strokeDasharray="4 5">
          <rect x="820" y="150" width="190" height="70" rx="10" />
          <rect x="1080" y="150" width="190" height="70" rx="10" />
          <rect x="820" y="270" width="190" height="70" rx="10" />
          <rect x="1080" y="270" width="190" height="70" rx="10" />
          <rect x="820" y="390" width="190" height="70" rx="10" />
          <path d="M1010 185 L1080 185" />
          <path d="M1010 305 L1080 305" />
          <path d="M1010 425 C1045 425 1045 305 1080 305" />
        </g>
        <defs>
          <linearGradient id="fr" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" style={{ stopColor: "var(--color-accent-base)", stopOpacity: 0 }} />
            <stop offset="1" style={{ stopColor: "var(--color-accent-soft)", stopOpacity: 0.8 }} />
          </linearGradient>
        </defs>
        <path d="M600 185 C700 185 720 185 820 185" pathLength={1} className="edge-draw" fill="none" stroke="url(#fr)" strokeWidth="2" />
        <circle cx="820" cy="185" r="4" style={{ fill: "var(--color-accent-soft)" }} />
      </svg>

      <div className="scroll-thin relative flex min-h-0 grow flex-col gap-10 overflow-y-auto px-5 py-10 sm:px-8 sm:py-14 lg:flex-row lg:items-start lg:justify-between lg:px-24 lg:py-[110px]">
        <div className="flex w-full max-w-[620px] flex-col gap-7">
          <div className="flex flex-col gap-3.5">
            <span className="lbl">First run{graph.routes[0]?.repo ? ` · ${graph.routes[0].repo}` : ""}</span>
            <h1 className="display rise m-0 text-4xl leading-none font-semibold sm:text-5xl xl:text-[64px]">
              Your first reflex forms <span className="text-accent-soft">here.</span>
            </h1>
            <p className="m-0 max-w-[54ch] text-[17px] leading-[1.55] font-light text-ink-2">
              Connect your agent, then work in this repository. Its traces appear here, and repeated moves can become
              reflexes with their evidence attached. The map shows what Adage has learned; opening it does not install hooks.
            </p>
          </div>
          <ol className="m-0 flex list-none flex-col rounded-lg border border-line-strong bg-panel p-0">
            <Step
              done={hooksOk && !checking && !healthError}
              label={hooksOk && !checking && !healthError ? "Hooks registered" : "Hooks"}
              detail={
                checking ? "checking…" : healthError ? "check unavailable" : health
                  ? hooksOk
                    ? installed.map((h) => h.check.replace(/^hooks\s*:?\s*\(?/i, "").replace(/\)$/, "")).join(" · ") || "installed"
                    : installed.length ? "some hooks need attention: open Health" : "not set up: open Setup"
                  : "checking…"
              }
            />
            <Step done={receipts.length > 0} label="Agent connection" detail={receipts.length ? "Hook received" : hooksOk ? "Awaiting first hook" : "Connect a harness first"} />
            <Step
              done={instincts.length > 0}
              label={instincts.length ? `${plural(instincts.length, "instinct")} already active` : "Instincts"}
              detail={tracks.join(" · ") || "none bound yet"}
            />
            <Step done={false} label="Waiting for traces" detail="Repeated steps build evidence" last />
          </ol>
          <div className="flex flex-col gap-2.5">
            <div className="flex flex-wrap items-center gap-3 text-sm text-ink-2" role="status">
              {healthError && <span>Could not check hooks. Is the local server still running?</span>}
              <button type="button" disabled={checking} onClick={() => setAttempt((n) => n + 1)} className="text-accent-soft underline underline-offset-4 disabled:opacity-50">
                {checking ? "Checking setup…" : "Recheck setup"}
              </button>
            </div>
            <button type="button" onClick={() => setUrl({ view: "setup" }, true)} className="rounded-lg border border-accent/45 bg-panel px-4 py-3 text-left text-accent-soft">
              Set up my repository →
            </button>
            <span className="text-sm text-ink-2">Connect your agent, check history sources, and preview conditioning from the terminal palette. OpenCode history is imported explicitly.</span>
          </div>
        </div>

        {instincts.length > 0 && (
          <div className="flex w-full max-w-[560px] flex-col gap-3 lg:mt-[410px]">
            <span className="lbl">Active from day 0</span>
            {instincts.slice(0, 3).map((r) => (
              <div key={`${r.reference}${r.id}`} className="flex items-center gap-3 rounded-lg border border-line-strong bg-panel px-4 py-3.5">
                <InstinctGlyph size={18} />
                <div className="flex min-w-0 grow flex-col gap-0.5">
                  <span className="truncate font-mono text-[13.5px]">{r.id}</span>
                  <span className="truncate text-[12.5px] text-ink-2">{r.summary || `${r.trigger.label.replace(/`/g, "")} → ${r.step.label}`}</span>
                </div>
                <StrengthChip strength={strength(r.strength)} />
              </div>
            ))}
            <span className="text-[12.5px] text-ink-3">Instincts start as prime. Your own traces confirm them to fire.</span>
          </div>
        )}
      </div>

      <div className="relative flex h-10 shrink-0 items-center gap-4 border-t border-line bg-panel px-5 font-mono text-[11.5px] text-ink-3">
        <span>{port} · read-only · this machine only</span>
        <span className="grow" />
        <span className="hidden sm:inline">
          {connection === "live" ? "live stream connected" : connection === "offline" ? "live stream offline: is adage map running? Retrying" : "connecting…"}
        </span>
      </div>
    </main>
  );
}
