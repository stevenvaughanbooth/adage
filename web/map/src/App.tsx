import { useEffect, useState } from "react";
import { start, useStore } from "./api/client";
import { Inspector } from "./components/Inspector";
import { LeftRail } from "./components/LeftRail";
import { MapCanvas } from "./components/MapCanvas";
import { Palette } from "./components/Palette";
import { Shortcuts } from "./components/Shortcuts";
import { TopBar } from "./components/TopBar";
import { Toast, copyCommand } from "./components/ui";
import { buildMap, findRoute, routeKey } from "./lib/graph";
import { isInstinct, orderedVerbs, reviewCommand } from "./lib/words";
import { useKeys } from "./state/keys";
import { cycleTheme } from "./state/theme";
import { useUrl } from "./state/url";
import { Evidence } from "./views/Evidence";
import { FirstRun } from "./views/FirstRun";
import { Health } from "./views/Health";
import { Traces } from "./views/Traces";

const TITLES = { map: "Map", traces: "Traces", evidence: "Evidence", health: "Health" } as const;

export function App() {
  const { graph, error, connection } = useStore();
  const [url, setUrl] = useUrl();
  const [palette, setPalette] = useState(false);
  const [sheet, setSheet] = useState(false);
  useEffect(() => start(), []);
  useEffect(() => {
    document.title = `${TITLES[url.view]} · Adage map`;
  }, [url.view]);

  // First run: nothing learned and no traces yet (instincts may already be on).
  const firstRun = Boolean(graph && graph.sessions.total === 0 && graph.events.length === 0 && !graph.routes.some((r) => !isInstinct(r)));

  useKeys((k, e) => {
    const dispatch = (name: string, detail: string) => window.dispatchEvent(new CustomEvent(name, { detail }));
    switch (k) {
      case "mod+k":
        setPalette((p) => !p);
        return true;
      case "escape":
        if (palette || sheet) return;
        if (url.reflex) setUrl({ reflex: null }, true);
        return;
      case "?":
        setSheet(true);
        return true;
      case "g m":
        setUrl({ view: "map" }, true);
        return true;
      case "g t":
        setUrl({ view: "traces" }, true);
        return true;
      case "g e":
        setUrl({ view: "evidence" }, true);
        return true;
      case "g h":
        setUrl({ view: "health" }, true);
        return true;
      case "t":
        cycleTheme();
        return true;
    }
    if (url.view !== "map" || !graph) return;
    switch (k) {
      case "/":
        document.getElementById("filter")?.focus();
        return true;
      case "a":
        setUrl({ mode: url.mode === "aggregated" ? "expanded" : "aggregated" });
        return true;
      case "f":
        if (url.reflex) setUrl({ focus: (url.focus + 1) % 4 });
        return true;
      case "0":
        dispatch("adage-canvas", "fit");
        return true;
      case "+":
      case "=":
        dispatch("adage-canvas", "zoom-in");
        return true;
      case "-":
      case "−":
        dispatch("adage-canvas", "zoom-out");
        return true;
      case "space":
        dispatch("adage-replay", "toggle");
        return true;
      case "c": {
        const r = findRoute(graph, url.reflex);
        const v = r ? orderedVerbs(r)[0] : undefined;
        if (r && v) void copyCommand(reviewCommand(v, r));
        return true;
      }
      case "j":
      case "k": {
        const m = buildMap(graph, {
          mode: url.mode,
          filters: { repo: url.repo, strengths: url.strengths, show: url.show, query: url.q },
        });
        const keys = m.nodes.filter((n) => n.kind === "reflex" || n.kind === "instinct").map((n) => n.routes[0] ?? "");
        if (!keys.length) return true;
        const cur = findRoute(graph, url.reflex);
        const i = cur ? keys.indexOf(routeKey(cur)) : -1;
        const next = keys[(i + (k === "j" ? 1 : -1) + keys.length) % keys.length] ?? null;
        setUrl({ reflex: next }, true);
        void e;
        return true;
      }
    }
  });

  return (
    <div className="flex h-full min-w-[1024px] flex-col overflow-hidden">
      <a href="#content" className="sr-only focus:not-sr-only focus:absolute focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:bg-raised focus:px-3 focus:py-2">
        Skip to content
      </a>
      <TopBar graph={graph} connection={connection} firstRun={firstRun} onPalette={() => setPalette(true)} />
      <div id="content" className="flex min-h-0 grow flex-col">
        {!graph ? (
          <Loading error={error} />
        ) : url.view === "map" ? (
          firstRun ? (
            <FirstRun graph={graph} connection={connection} />
          ) : (
            <div className="grid min-h-0 grow grid-cols-[248px_minmax(0,1fr)_336px] xl:grid-cols-[280px_minmax(0,1fr)_380px]">
              <LeftRail graph={graph} />
              <MapCanvas graph={graph} />
              <Inspector graph={graph} />
            </div>
          )
        ) : url.view === "traces" ? (
          <Traces graph={graph} />
        ) : url.view === "evidence" ? (
          <Evidence graph={graph} />
        ) : (
          <Health />
        )}
      </div>
      <Palette graph={graph} open={palette} onClose={() => setPalette(false)} onShortcuts={() => setSheet(true)} />
      <Shortcuts open={sheet} onClose={() => setSheet(false)} />
      <Toast />
    </div>
  );
}

function Loading({ error }: { error: string | null }) {
  return (
    <main className="canvas-grid flex grow items-center justify-center p-8" aria-busy={!error}>
      {error ? (
        <div className="flex max-w-md flex-col gap-3 text-center">
          <p className="display m-0 text-2xl font-semibold">The map could not load its data.</p>
          <p className="m-0 font-mono text-xs text-ink-2">{error}</p>
          <p className="m-0 text-sm text-ink-2">
            Is <code className="font-mono text-accent-soft">adage map</code> still running? The page retries on its own.
          </p>
        </div>
      ) : (
        <p className="lbl">Loading the map…</p>
      )}
    </main>
  );
}
