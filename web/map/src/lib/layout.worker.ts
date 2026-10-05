// ELK's layered layout, off the main thread. elkjs is large, so it loads only here, lazily: the
// map draws its instant column layout first and moves to this one when it arrives.

import "./elk-shim";
import ELK from "elkjs/lib/elk.bundled.js";

export interface LayoutRequest {
  seq: number;
  direction: "RIGHT" | "DOWN";
  nodes: { id: string; width: number; height: number; layer: number }[];
  edges: { id: string; source: string; target: string }[];
}

export interface LayoutResponse {
  seq: number;
  positions: Record<string, { x: number; y: number }>;
  error?: string;
}

const elk = new ELK();

self.onmessage = async (ev: MessageEvent<LayoutRequest>) => {
  const { seq, direction, nodes, edges } = ev.data;
  try {
    const out = await elk.layout({
      id: "root",
      layoutOptions: {
        "elk.algorithm": "layered",
        "elk.direction": direction,
        "elk.edgeRouting": "SPLINES",
        "elk.layered.spacing.nodeNodeBetweenLayers": direction === "RIGHT" ? "62" : "70",
        "elk.spacing.nodeNode": "24",
        "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
        "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES",
        "elk.layered.crossingMinimization.forceNodeModelOrder": "false",
        "elk.separateConnectedComponents": "false",
      },
      children: nodes.map((n) => ({
        id: n.id,
        width: n.width,
        height: n.height,
        layoutOptions: { "elk.layered.layering.layerChoiceConstraint": String(n.layer) },
      })),
      edges: edges.map((e) => ({ id: e.id, sources: [e.source], targets: [e.target] })),
    });
    const positions: Record<string, { x: number; y: number }> = {};
    for (const c of out.children ?? []) positions[c.id] = { x: c.x ?? 0, y: c.y ?? 0 };
    (self as unknown as Worker).postMessage({ seq, positions } satisfies LayoutResponse);
  } catch (e) {
    (self as unknown as Worker).postMessage({ seq, positions: {}, error: String(e) } satisfies LayoutResponse);
  }
};
