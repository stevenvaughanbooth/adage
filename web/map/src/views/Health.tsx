// Health: the doctor's checks, as `adage doctor` prints them.

import { type ReactNode, useCallback, useEffect, useState } from "react";
import { loadHealth } from "../api/client";
import type { Health as HealthData, HealthCheck } from "../api/types";
import { Icons } from "../components/glyphs";
import { CommandButton, cx } from "../components/ui";
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
    <main aria-label="Health" className="scroll-thin flex min-h-0 grow flex-col gap-6 overflow-y-auto px-10 py-7">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="lbl">Health · adage doctor</span>
          <h1 className="display m-0 text-[44px] leading-[1.05] font-semibold">
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
        <div className="flex w-[300px] flex-col gap-2">
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
                <span className="text-[14.5px] text-ink-1">{c.check}</span>
                <span className="text-[13px] break-words text-ink-2">{c.detail}</span>
                {c.fix && (
                  <div className="mt-1.5 flex flex-col gap-1.5">
                    {cmd ? <CommandButton command={cmd} why={c.fix.replace(/`/g, "")} /> : <span className="text-xs text-ink-3">fix: {c.fix}</span>}
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
