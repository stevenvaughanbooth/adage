// The centre canvas: stimulus → reflex → response, laid out left to right, live.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  MiniMap,
  ReactFlow,
  ReactFlowProvider,
  ViewportPortal,
  useReactFlow,
  useStoreApi,
} from "@xyflow/react";
import type { Graph, TraceEvent } from "../api/types";
import { subscribeDecisions } from "../api/client";
import { buildMap, columnHeads, counts, focus, neighbourhood, routeKey, findRoute, NODE_H, NODE_W } from "../lib/graph";
import { useLayout } from "../lib/layout";
import { plural } from "../lib/words";
import { reducedMotion } from "../state/theme";
import { useUrl } from "../state/url";
import { Icons } from "./glyphs";
import { activeFilters } from "./LeftRail";
import { type EdgeData, type FlowEdge, type FlowNode, edgeTypes, nodeTypes } from "./nodes";
import { Replay } from "./Replay";
import { Segmented, cx } from "./ui";

interface Pulse {
  n: number;
  control: boolean;
}

/** Pulses for decisions, by reflex key; cleared after the motion finishes. */
function usePulses(graph: Graph) {
  const [pulses, setPulses] = useState<Map<string, Pulse>>(new Map());
  const seq = useRef(0);
  const byId = useRef(new Map<string, string[]>());
  byId.current = useMemo(() => {
    const m = new Map<string, string[]>();
    for (const r of graph.routes) m.set(`${r.repo}::${r.id}`, [...(m.get(`${r.repo}::${r.id}`) ?? []), routeKey(r)]);
    return m;
  }, [graph.routes]);
  const fire = useCallback((e: TraceEvent) => {
    const control = e.arm === "control";
    const ids = control ? e.would : e.directed;
    if (!ids.length) return;
    setPulses((prev) => {
      const next = new Map(prev);
      for (const id of ids) for (const k of byId.current.get(`${e.repo}::${id}`) ?? []) next.set(k, { n: ++seq.current, control });
      return next;
    });
    const n = seq.current;
    setTimeout(() => {
      setPulses((prev) => {
        const next = new Map(prev);
        for (const [k, p] of next) if (p.n <= n) next.delete(k);
        return next;
      });
    }, 1900);
  }, []);
  useEffect(() => subscribeDecisions(fire), [fire]);
  return { pulses, fire };
}

function Canvas({ graph, onFilter }: { graph: Graph; onFilter?: () => void }) {
  const [url, setUrl] = useUrl();
  const [at, setAt] = useState<number | null>(url.at);
  const { pulses, fire } = usePulses(graph);
  const rf = useReactFlow();
  const store = useStoreApi();
  const wrap = useRef<HTMLDivElement>(null);
  const reduced = reducedMotion();

  // Keep the replay position in the URL once it settles.
  useEffect(() => {
    const t = setTimeout(() => setUrl({ at }), 300);
    return () => clearTimeout(t);
  }, [at, setUrl]);

  const selected = findRoute(graph, url.reflex);
  const selKey = selected ? routeKey(selected) : null;
  const full = useMemo(
    () =>
      buildMap(graph, {
        mode: url.mode,
        filters: { repo: url.repo, strengths: url.strengths, show: url.show, query: url.q, cards: url.cards },
        at,
      }),
    [graph, url.mode, url.repo, url.strengths, url.show, url.q, url.cards, at],
  );
  const selNode = selKey ? `r:${selKey}` : null;
  const keep = useMemo(() => neighbourhood(full.edges, selNode, url.focus), [full.edges, selNode, url.focus]);
  const map = useMemo(() => focus(full, keep), [full, keep]);
  // Layout ignores replay state, so scrubbing never moves anything.
  const shape = useMemo(() => ({ nodes: map.nodes, edges: map.edges }), [map.nodes.map((n) => n.id).join(), map.edges.map((e) => e.id).join(), url.dir]); // eslint-disable-line react-hooks/exhaustive-deps
  const placed = useLayout(shape.nodes, shape.edges, url.dir);
  const arc = useMemo(() => {
    if (!selKey) return null;
    const s = new Set<string>();
    for (const e of map.edges) if (e.route === selKey) (s.add(e.source), s.add(e.target), s.add(e.id));
    return s;
  }, [map.edges, selKey]);

  const vertical = url.dir === "DOWN";
  const nodes: FlowNode[] = useMemo(
    () =>
      map.nodes.map((n) => {
        const pulse = Math.max(0, ...n.routes.map((k) => (n.kind === "reflex" || n.kind === "instinct" ? (pulses.get(k)?.n ?? 0) : 0)));
        return {
          id: n.id,
          type: n.kind,
          position: placed.positions[n.id] ?? { x: 0, y: 0 },
          data: { node: n, selected: n.id === selNode, dim: Boolean(arc && !arc.has(n.id)), pulse, vertical },
          width: NODE_W,
          height: NODE_H,
          ariaLabel: `${n.tag.toLowerCase()}: ${n.title}${n.sub ? `, ${n.sub}` : ""}`,
        } satisfies FlowNode;
      }),
    [map.nodes, placed, selNode, arc, pulses, vertical],
  );
  const edges: FlowEdge[] = useMemo(
    () =>
      map.edges.map((e) => {
        const p = pulses.get(e.route);
        const target = map.nodes.find((n) => n.id === e.target);
        const source = map.nodes.find((n) => n.id === e.source);
        const data: EdgeData = {
          hot: Boolean(arc?.has(e.id)),
          dim: Boolean(arc && !arc.has(e.id)),
          pulse: p && !reduced ? p.n : 0,
          control: Boolean(p?.control),
          unformed: Boolean(target?.unformed || source?.unformed),
          strength: e.strength,
        };
        return { id: e.id, source: e.source, target: e.target, type: "arc", data, focusable: false };
      }),
    [map.edges, map.nodes, arc, pulses, reduced],
  );

  // Fit when the set of nodes changes (not on every live update).
  const fitKey = shape.nodes.map((n) => n.id).join() + url.dir + (placed === undefined ? "" : Object.keys(placed.positions).length);
  const firstFit = useRef(true);
  useEffect(() => {
    const t = setTimeout(() => {
      void rf.fitView({ padding: 0.12, maxZoom: 1.1, duration: firstFit.current || reduced ? 0 : 300 });
      firstFit.current = false;
    }, 30);
    return () => clearTimeout(t);
  }, [fitKey]); // eslint-disable-line react-hooks/exhaustive-deps

  // Bring the selected reflex into view when it is chosen elsewhere (the tree, the palette).
  useEffect(() => {
    if (!selNode || !wrap.current) return;
    const p = placed.positions[selNode];
    if (!p) return;
    const { transform } = store.getState();
    const [tx, ty, z] = transform;
    const r = wrap.current.getBoundingClientRect();
    const sx = p.x * z + tx;
    const sy = p.y * z + ty;
    const margin = 40;
    if (sx < margin || sy < 90 || sx + NODE_W * z > r.width - margin || sy + NODE_H * z > r.height - 150) {
      void rf.setCenter(p.x + NODE_W / 2, p.y + NODE_H / 2, { zoom: Math.max(z, 0.8), duration: reduced ? 0 : 350 });
    }
  }, [selNode]); // eslint-disable-line react-hooks/exhaustive-deps

  // Keys the canvas owns: zoom, fit, and the replay.
  useEffect(() => {
    const onKey = (e: Event) => {
      const k = (e as CustomEvent<string>).detail;
      if (k === "zoom-in") void rf.zoomIn({ duration: 150 });
      if (k === "zoom-out") void rf.zoomOut({ duration: 150 });
      if (k === "fit") void rf.fitView({ padding: 0.12, maxZoom: 1.1, duration: reduced ? 0 : 300 });
    };
    window.addEventListener("adage-canvas", onKey);
    return () => window.removeEventListener("adage-canvas", onKey);
  }, [rf, reduced]);

  const select = (id: string | null) => {
    const n = map.nodes.find((x) => x.id === id);
    const key = n?.routes[0] ?? null;
    setUrl({ reflex: key === selKey ? null : key }, true);
  };

  const heads = url.dir === "RIGHT" ? columnHeads(map, placed) : [];
  const c = counts(graph, url.repo);
  const nFilters = activeFilters(url);
  const traces = new Set(graph.events.filter((e) => !url.repo || e.repo === url.repo).map((e) => e.session)).size;
  const sessions = url.repo ? traces : Math.max(graph.sessions.total, traces);

  return (
    <main aria-label="Reflex map" className="canvas-grid relative flex min-h-0 min-w-0 flex-col overflow-hidden">
      <h1 className="sr-only">Reflex map</h1>
      <div ref={wrap} className="relative min-h-0 grow">
        {map.nodes.length === 0 ? (
          <div className="flex h-full items-center justify-center p-8 text-center">
            <div className="flex max-w-sm flex-col items-center gap-3">
              <p className="display text-2xl font-semibold text-ink-1">Nothing on the map with these filters.</p>
              <p className="text-sm text-ink-2">
                {graph.routes.length ? "Clear the filter or turn a strength back on." : "No reflexes yet."}
              </p>
              <button
                type="button"
                className="h-8 rounded-md border border-line-strong px-3 text-[13px] text-ink-1 hover:border-accent"
                onClick={() => setUrl({ q: "", strengths: new Set(["fire", "prime", "inhibit"]), focus: 0 })}
              >
                Clear the filters
              </button>
            </div>
          </div>
        ) : (
          <ReactFlow<FlowNode, FlowEdge>
            nodes={nodes}
            edges={edges}
            nodeTypes={nodeTypes}
            edgeTypes={edgeTypes}
            nodesDraggable={false}
            nodesConnectable={false}
            elementsSelectable={false}
            edgesFocusable={false}
            onlyRenderVisibleElements={nodes.length > 300}
            minZoom={0.15}
            maxZoom={2}
            proOptions={{ hideAttribution: true }}
            onNodeClick={(_, n) => select(n.id)}
            onPaneClick={() => selKey && setUrl({ reflex: null }, true)}
            onKeyDown={(e) => {
              const id = (e.target as HTMLElement).closest?.(".react-flow__node")?.getAttribute("data-id");
              if (id && (e.key === "Enter" || e.key === " ")) {
                e.preventDefault();
                select(id);
              }
            }}
            aria-label="Reflex map: stimulus, reflex, response"
          >
            <ViewportPortal>
              {heads.map((h) => (
                <span
                  key={h.label}
                  className="lbl pointer-events-none absolute whitespace-nowrap"
                  style={{ transform: `translate(${h.x}px, ${h.y - 30}px)` }}
                >
                  {h.label}
                </span>
              ))}
            </ViewportPortal>
            <MiniMap
              pannable
              zoomable
              ariaLabel="Minimap"
              className="max-sm:hidden!"
              position="bottom-right"
              style={{ width: 168, height: 104, margin: 16 }}
              nodeClassName={(n) => {
                const d = (n as FlowNode).data;
                if (d.selected) return "mm-sel";
                if (d.node.strength === "inhibit") return "mm-inhibit";
                return "mm-node";
              }}
              nodeBorderRadius={2}
            />
          </ReactFlow>
        )}

        <div className="pointer-events-none absolute inset-x-4 top-3.5 flex items-center gap-2">
          <div className="pointer-events-auto flex flex-wrap items-center gap-2">
            {onFilter && (
              <button
                type="button"
                onClick={onFilter}
                aria-haspopup="dialog"
                title="Filter, strengths and the reflex tree (/)"
                className={cx(
                  "flex h-[34px] items-center gap-1.5 rounded-md border bg-panel px-3 text-[12.5px] lg:hidden",
                  nFilters ? "border-accent/45 text-ink-1" : "border-line-strong text-ink-2 hover:text-ink-1",
                )}
              >
                <Icons.filter size={14} />
                Filter
                {nFilters > 0 && <span className="font-mono text-[11px] text-accent-soft">· {nFilters}</span>}
              </button>
            )}
            <Segmented
              label="Graph mode"
              value={url.mode}
              onChange={(mode) => setUrl({ mode })}
              options={[
                { value: "aggregated", label: "Aggregated", title: "Shared stimuli and responses drawn once (a)" },
                { value: "expanded", label: "Expanded", title: "Every reflex on its own arc (a)" },
              ]}
            />
            <button
              type="button"
              aria-pressed={url.focus > 0}
              disabled={!selKey}
              title={selKey ? "Show only what is near the selected reflex (f)" : "Select a reflex to focus on it"}
              onClick={() => setUrl({ focus: (url.focus + 1) % 4 })}
              className={cx(
                "h-[34px] rounded-md border bg-panel px-3 text-[12.5px] transition-colors duration-100 disabled:cursor-not-allowed disabled:opacity-60",
                url.focus > 0 ? "border-accent/45 text-ink-1" : "border-line-strong text-ink-2 hover:text-ink-1",
              )}
            >
              {url.focus > 0 ? `Focus · ${plural(url.focus, "hop")}` : "Focus · off"}
            </button>
            <button
              type="button"
              onClick={() => setUrl({ dir: url.dir === "RIGHT" ? "DOWN" : "RIGHT" })}
              title="Layout direction"
              className="h-[34px] rounded-md border border-line-strong bg-panel px-3 text-[12.5px] text-ink-2 hover:text-ink-1"
            >
              {url.dir === "RIGHT" ? "Left → right" : "Top → down"}
            </button>
          </div>
          <div className="grow" />
          <span className="hidden rounded-md bg-base/70 px-2 py-1 text-[12.5px] text-ink-2 backdrop-blur-sm lg:inline">
            {plural(c.reflexes, "reflex", "reflexes")} · {plural(c.instincts, "instinct")} · {plural(sessions, "trace")}
          </span>
        </div>

        {map.nodes.length > 0 && (
          <div className="absolute bottom-4 left-4 flex flex-col overflow-hidden rounded-md border border-line-strong bg-panel">
            <button type="button" aria-label="Zoom in" title="Zoom in (+)" onClick={() => void rf.zoomIn({ duration: 150 })} className="flex h-8 w-8 items-center justify-center text-ink-2 hover:text-ink-1">
              <Icons.plus />
            </button>
            <button type="button" aria-label="Zoom out" title="Zoom out (−)" onClick={() => void rf.zoomOut({ duration: 150 })} className="flex h-8 w-8 items-center justify-center border-t border-line text-ink-2 hover:text-ink-1">
              <Icons.minus />
            </button>
            <button
              type="button"
              aria-label="Fit to screen"
              title="Fit (0)"
              onClick={() => void rf.fitView({ padding: 0.12, maxZoom: 1.1, duration: reduced ? 0 : 300 })}
              className="flex h-8 w-8 items-center justify-center border-t border-line text-ink-2 hover:text-ink-1"
            >
              <Icons.fit />
            </button>
          </div>
        )}
      </div>
      <Replay
        events={graph.events.filter((e) => !url.repo || e.repo === url.repo)}
        learning={(graph.learning ?? []).filter((l) => !url.repo || !l.repo || l.repo === url.repo)}
        at={at}
        onAt={setAt}
        onPlayEvent={fire}
      />
    </main>
  );
}

export function MapCanvas({ graph, onFilter }: { graph: Graph; onFilter?: () => void }) {
  return (
    <ReactFlowProvider>
      <Canvas graph={graph} onFilter={onFilter} />
    </ReactFlowProvider>
  );
}
