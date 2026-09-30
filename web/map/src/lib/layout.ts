// The layout bridge: the instant column layout now, ELK's from the worker when it is ready.

import { useEffect, useMemo, useRef, useState } from "react";
import { type Direction, type MapEdge, type MapNode, type Placed, NODE_H, NODE_W, columnLayout } from "./graph";
import type { LayoutRequest, LayoutResponse } from "./layout.worker";

let worker: Worker | null = null;
let seq = 0;
const waiting = new Map<number, (r: LayoutResponse) => void>();

function getWorker(): Worker | null {
  if (worker) return worker;
  try {
    worker = new Worker(new URL("./layout.worker.ts", import.meta.url), { type: "module", name: "layout" });
    worker.onmessage = (ev: MessageEvent<LayoutResponse>) => {
      waiting.get(ev.data.seq)?.(ev.data);
      waiting.delete(ev.data.seq);
    };
  } catch {
    worker = null;
  }
  return worker;
}

const LAYER: Record<MapNode["kind"], number> = { stimulus: 0, reflex: 1, instinct: 1, response: 2 };

function elkLayout(nodes: MapNode[], edges: MapEdge[], direction: Direction): Promise<Placed | null> {
  const w = getWorker();
  if (!w || nodes.length === 0) return Promise.resolve(null);
  const id = ++seq;
  const req: LayoutRequest = {
    seq: id,
    direction,
    nodes: nodes.map((n) => ({ id: n.id, width: NODE_W, height: NODE_H, layer: LAYER[n.kind] })),
    edges: edges.map((e) => ({ id: e.id, source: e.source, target: e.target })),
  };
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      waiting.delete(id);
      resolve(null);
    }, 8000);
    waiting.set(id, (r) => {
      clearTimeout(timer);
      resolve(r.error ? null : { positions: r.positions });
    });
    w.postMessage(req);
  });
}

/** Positions for the map: columns at once, then ELK's layered layout. */
export function useLayout(nodes: MapNode[], edges: MapEdge[], direction: Direction, enabled = true): Placed {
  const first = useMemo(() => columnLayout({ nodes, edges }, direction), [nodes, edges, direction]);
  const [refined, setRefined] = useState<{ key: string; placed: Placed } | null>(null);
  const key = useMemo(
    () => direction + "|" + nodes.map((n) => n.id).join(",") + "|" + edges.map((e) => e.id).join(","),
    [nodes, edges, direction],
  );
  const latest = useRef(key);
  latest.current = key;
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    void elkLayout(nodes, edges, direction).then((p) => {
      if (live && p && latest.current === key) setRefined({ key, placed: p });
    });
    return () => {
      live = false;
    };
  }, [key, enabled]); // eslint-disable-line react-hooks/exhaustive-deps
  if (refined && refined.key === key) {
    // Every node must have a place; fall back to the columns for any the worker missed.
    return { positions: { ...first.positions, ...refined.placed.positions } };
  }
  return first;
}
