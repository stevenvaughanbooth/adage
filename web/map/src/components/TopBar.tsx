import type { ReactNode } from "react";
import type { Graph } from "../api/types";
import type { Connection } from "../api/client";
import { counts, repos } from "../lib/graph";
import { STRENGTHS } from "../lib/words";
import { type View, useUrl } from "../state/url";
import { type ThemeMode, cycleTheme, useTheme } from "../state/theme";
import { Icons, Lamp } from "./glyphs";
import { cx } from "./ui";

const NAV: { view: View; label: string; key: string }[] = [
  { view: "map", label: "Map", key: "g m" },
  { view: "traces", label: "Traces", key: "g t" },
  { view: "evidence", label: "Evidence", key: "g e" },
  { view: "health", label: "Health", key: "g h" },
];

const THEME_ICON: Record<ThemeMode, () => ReactNode> = {
  system: () => <Icons.system size={15} />,
  night: () => <Icons.moon size={15} />,
  day: () => <Icons.sun size={15} />,
};

function tracesToday(g: Graph | null, repo: string | null): number {
  if (!g) return 0;
  const since = new Date();
  since.setHours(0, 0, 0, 0);
  const s = new Set(g.events.filter((e) => e.ts >= since.getTime() && (!repo || e.repo === repo)).map((e) => e.session));
  return s.size;
}

export function LivePill({ connection, graph, firstRun }: { connection: Connection; graph: Graph | null; firstRun: boolean }) {
  const [url] = useUrl();
  const n = tracesToday(graph, url.repo);
  const tone =
    connection === "offline"
      ? "border-inhibit/35 bg-inhibit/8"
      : firstRun || connection === "connecting"
        ? "border-accent/30 bg-accent/8"
        : "border-fire/25 bg-fire/8";
  const dot =
    connection === "offline"
      ? "var(--color-lamp-inhibit)"
      : firstRun || connection === "connecting"
        ? "var(--glyph-arc)"
        : "var(--color-lamp-fire)";
  const [word, rest] =
    connection === "offline"
      ? ["Offline", "· retrying"]
      : connection === "connecting"
        ? ["Connecting", ""]
        : firstRun
          ? ["Listening", "· waiting for the first trace"]
          : ["Live", `· ${n} trace${n === 1 ? "" : "s"} today`];
  return (
    <div
      className={cx("hidden h-7 shrink-0 items-center gap-2 rounded-full border px-3 text-[12.5px] text-ink-1 lg:flex", tone)}
      aria-live="polite"
      data-testid="live-pill"
    >
      <span
        className={cx("h-[7px] w-[7px] rounded-full", connection !== "offline" && "live-dot")}
        style={{ background: dot, boxShadow: `0 0 10px ${dot}` }}
        aria-hidden="true"
      />
      {word}
      {rest && <span className="text-ink-2">{rest}</span>}
    </div>
  );
}

export function TopBar({
  graph,
  connection,
  firstRun,
  onPalette,
}: {
  graph: Graph | null;
  connection: Connection;
  firstRun: boolean;
  onPalette: () => void;
}) {
  const [url, setUrl] = useUrl();
  const theme = useTheme();
  const all = graph ? repos(graph) : [];
  const c = graph ? counts(graph, url.repo) : null;
  const Theme = THEME_ICON[theme];
  return (
    <header className="flex h-14 shrink-0 items-center gap-4 border-b border-line px-5">
      <a href="/" className="wordmark" aria-label="Adage map, home" onClick={(e) => (e.preventDefault(), setUrl({ view: "map" }, true))}>
        Adage
      </a>
      {all.length > 1 ? (
        <label className="relative flex items-center">
          <span className="sr-only">Repository</span>
          <select
            value={url.repo ?? ""}
            onChange={(e) => setUrl({ repo: e.target.value || null, reflex: null }, true)}
            className="h-8 appearance-none rounded-md border border-line-strong bg-panel pr-7 pl-2.5 font-mono text-[12.5px] text-ink-1"
          >
            <option value="">all repositories</option>
            {all.map((r) => (
              <option key={r} value={r}>
                {r}
              </option>
            ))}
          </select>
          <span className="pointer-events-none absolute right-2 text-ink-2">
            <Icons.chevron size={12} />
          </span>
        </label>
      ) : (
        all[0] && <span className="font-mono text-[12.5px] text-ink-2">{all[0]}</span>
      )}
      <nav aria-label="Views" className="ml-1 flex gap-1">
        {NAV.map((n) => (
          <a
            key={n.view}
            href={`?view=${n.view}`}
            aria-current={url.view === n.view ? "page" : undefined}
            title={`${n.label} · ${n.key}`}
            onClick={(e) => {
              e.preventDefault();
              setUrl({ view: n.view }, true);
            }}
            className={cx(
              "border-b-2 px-3 py-2 text-sm transition-colors duration-100",
              url.view === n.view ? "border-accent font-medium text-ink-1" : "border-transparent text-ink-2 hover:text-ink-1",
            )}
          >
            {n.label}
          </a>
        ))}
      </nav>
      <div className="grow" />
      <LivePill connection={connection} graph={graph} firstRun={firstRun} />
      {c && !firstRun && (
        <div className="hidden items-center gap-3.5 font-mono text-xs text-ink-2 xl:flex" aria-label="Reflexes by strength">
          {STRENGTHS.map((s) => (
            <span key={s} className="flex items-center gap-1.5">
              <Lamp strength={s} />
              {c.strengths.get(s) ?? 0} {s}
            </span>
          ))}
        </div>
      )}
      <button
        type="button"
        onClick={onPalette}
        className="flex h-8 w-[220px] shrink items-center gap-2.5 rounded-md border border-line-strong bg-panel px-2.5 text-[13px] text-ink-2 hover:text-ink-1"
        aria-label="Search or jump to (Command K)"
      >
        <Icons.search />
        <span className="grow truncate text-left">Search or jump to…</span>
        <kbd>⌘K</kbd>
      </button>
      <button
        type="button"
        onClick={cycleTheme}
        aria-label={`Theme: ${theme}. Change theme`}
        title={`Theme: ${theme} (t)`}
        className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-line-strong bg-panel text-ink-2 hover:text-ink-1"
      >
        <Theme />
      </button>
    </header>
  );
}
