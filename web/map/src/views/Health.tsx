// Health: the doctor's checks, as `adage doctor` prints them.

import { type ReactNode, useCallback, useEffect, useState } from "react";
import { loadHealth } from "../api/client";
import type { Health as HealthData, HealthCheck } from "../api/types";
import { Icons, RewriteGlyph } from "../components/glyphs";
import { CommandButton, Prose, cx } from "../components/ui";
import { plural } from "../lib/words";

const MARK: Record<HealthCheck["status"], { icon: () => ReactNode; tone: string; word: string }> = {
  pass: { icon: () => <Icons.check size={12} />, tone: "bg-fire/14 text-fire", word: "passed" },
  warn: { icon: () => <Icons.warn size={12} />, tone: "bg-prime/14 text-prime", word: "warning" },
  fail: { icon: () => <Icons.cross size={12} />, tone: "bg-inhibit/14 text-inhibit", word: "failed" },
};

/** A fix that is a command gets a copy button; prose stays prose. */
function fixCommand(fix: string): string | null {
  const m = fix.match(/`(adage [^`]+)`/) ?? fix.match(/^(adage \S.*)$/);
  return m?.[1] ?? null;
}

/** Settings, not problems: what the hooks change on this machine, and what intent reflexes embed with. */
function Runtime({ rt }: { rt: NonNullable<HealthData["runtime"]> }) {
  const rw = rt.rewrite;
  const em = rt.embeddings;
  return (
    <section aria-label="On this machine" className="flex flex-col gap-2">
      <h2 className="lbl m-0 font-normal">On this machine</h2>
      <ul className="m-0 grid list-none grid-cols-1 gap-3 p-0 xl:grid-cols-2">
        <li className="flex gap-3.5 rounded-lg border border-line-strong bg-panel px-4 py-3.5" data-testid="runtime-rewrites">
          <span className="mt-0.5 flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full bg-raised text-accent-soft">
            <RewriteGlyph size={13} />
          </span>
          <div className="flex min-w-0 grow flex-col gap-1">
            <span className="text-[14.5px] text-ink-1">Rewrites to the project’s environment: {rw.on ? "on" : "off"}</span>
            <span className="text-[13px] text-ink-2">
              <Prose text={rw.on
                ? "A Python test run under the system Python is pointed at the project’s verified .venv before it runs; the agent is told each time. Never in the control arm."
                : rw.marker && rw.config
                  ? "Off by the marker an older `adage delegate off` left in exec/rewrite-off."
                  : "Off (`runtime.rewrite = false`): test runs keep the interpreter the agent chose."} />
            </span>
            <div className="mt-1.5">
              <CommandButton
                command={rw.on ? "adage delegate rewrites off" : "adage delegate rewrites on"}
                why={rw.on ? "turn rewrites off" : "turn rewrites back on"}
              />
            </div>
          </div>
        </li>
        <li className="flex gap-3.5 rounded-lg border border-line-strong bg-panel px-4 py-3.5" data-testid="runtime-model">
          <span className={cx("mt-0.5 flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full", em.fallback ? "bg-prime/14 text-prime" : "bg-fire/14 text-fire")}>
            {em.fallback ? <Icons.warn size={12} /> : <Icons.check size={12} />}
            <span className="sr-only">{em.fallback ? "warning" : "passed"}</span>
          </span>
          <div className="flex min-w-0 grow flex-col gap-1">
            <span className="text-[14.5px] text-ink-1">{em.bundled ? "Bundled model" : "Intent model"}</span>
            <span className="font-mono text-[13px] break-words text-ink-2">{em.model}</span>
            <span className="text-[12.5px] text-ink-3">
              <Prose text={em.bundled
                ? "Built into this binary: intent reflexes work with nothing to download. `adage models fetch` is optional."
                : em.fallback
                  ? "No model installed or bundled: intent reflexes match on a hashed fallback."
                  : "Installed under the models directory."} />
            </span>
          </div>
        </li>
      </ul>
    </section>
  );
}

export function Health() {
  const [data, setData] = useState<HealthData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback((force: boolean) => {
    setBusy(true);
    loadHealth(force)
      .then((h) => (setData(h), setError(null)))
      .catch((e: unknown) => setError(String(e)))
      .finally(() => setBusy(false));
  }, []);
  useEffect(() => load(false), [load]);
  const s = data?.summary;
  const order = { fail: 0, warn: 1, pass: 2 } as const;
  const checks = [...(data?.checks ?? [])].sort((a, b) => order[a.status] - order[b.status]);
  return (
    <main aria-label="Health" className="scroll-thin flex min-h-0 grow flex-col gap-6 overflow-y-auto px-4 py-7 sm:px-10">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="lbl">Health · adage doctor</span>
          <h1 className="display m-0 text-[34px] leading-[1.05] font-semibold sm:text-[44px]">
            {!s ? (
              "Checking…"
            ) : s.fail ? (
              <>
                {plural(s.fail, "check")} <span className="text-inhibit">failed.</span>
              </>
            ) : s.warn ? (
              <>
                Working, with <span className="text-prime">{plural(s.warn, "warning")}.</span>
              </>
            ) : (
              <>
                Everything <span className="text-accent-soft">checks out.</span>
              </>
            )}
          </h1>
          {s && (
            <span className="font-mono text-xs text-ink-3">
              {s.pass} passed · {plural(s.warn, "warning")} · {plural(s.fail, "failure")}
            </span>
          )}
        </div>
        <div className="flex w-full flex-col gap-2 sm:w-[300px]">
          <button
            type="button"
            onClick={() => load(true)}
            disabled={busy}
            className="h-9 rounded-md border border-line-strong text-[13px] text-ink-1 hover:border-accent/45 disabled:opacity-60"
          >
            {busy ? "Checking…" : "Check again"}
          </button>
        </div>
      </div>
      {data?.runtime && <Runtime rt={data.runtime} />}
      {error && <p className="m-0 rounded-md border border-inhibit/35 bg-inhibit/8 px-3 py-2 text-sm">Could not run the checks: {error}</p>}
      <ul className="m-0 grid list-none grid-cols-1 gap-3 p-0 xl:grid-cols-2">
        {checks.map((c) => {
          const m = MARK[c.status];
          const cmd = c.fix ? fixCommand(c.fix) : null;
          return (
            <li key={c.check} className="flex gap-3.5 rounded-lg border border-line-strong bg-panel px-4 py-3.5">
              <span className={cx("mt-0.5 flex h-[22px] w-[22px] shrink-0 items-center justify-center rounded-full", m.tone)}>
                {m.icon()}
                <span className="sr-only">{m.word}</span>
              </span>
              <div className="flex min-w-0 grow flex-col gap-1">
                <span className="text-[14.5px] text-ink-1">{c.label ?? c.check}</span>
                <span className="text-[13px] break-words text-ink-2">
                  <Prose text={c.detail} />
                </span>
                {c.fix && (
                  <div className="mt-1.5 flex flex-col gap-1.5">
                    {cmd ? <CommandButton command={cmd} why={c.fix.replace(/`/g, "")} /> : <span className="text-xs text-ink-3">
                        fix: <Prose text={c.fix} />
                      </span>}
                  </div>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </main>
  );
}
