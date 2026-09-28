// The map's node and edge types: stimulus → reflex (or instinct) → response.

import { memo } from "react";
import {
  BaseEdge,
  type Edge,
  type EdgeProps,
  Handle,
  type Node,
  type NodeProps,
  Position,
  getBezierPath,
  useStore,
} from "@xyflow/react";
import type { MapNode } from "../lib/graph";
import type { Strength } from "../lib/words";
import { KindGlyph, LAMP_VAR, Lamp } from "./glyphs";
import { cx } from "./ui";

export interface NodeData extends Record<string, unknown> {
  node: MapNode;
  selected: boolean;
  /** Not on the selected reflex's arc while one is selected. */
  dim: boolean;
  /** Increments each time the reflex fires, to replay the flash. */
  pulse: number;
  vertical: boolean;
}
export type FlowNode = Node<NodeData>;

export interface EdgeData extends Record<string, unknown> {
  hot: boolean;
  dim: boolean;
  pulse: number;
  control: boolean;
  unformed: boolean;
  strength: Strength | null | undefined;
}
export type FlowEdge = Edge<EdgeData>;

/** Far out, cards become lamp-only pills. */
const zoomedOut = (s: { transform: [number, number, number] }) => s.transform[2] < 0.5;

function Card({ data }: { data: NodeData }) {
  const { node: n, selected, dim, pulse, vertical } = data;
  const compact = useStore(zoomedOut);
  const reflexish = n.kind === "reflex" || n.kind === "instinct";
  const hollow = n.lifecycle === "nascent" || n.lifecycle === "extinguished";
  const handles = (
    <>
      <Handle type="target" position={vertical ? Position.Top : Position.Left} isConnectable={false} />
      <Handle type="source" position={vertical ? Position.Bottom : Position.Right} isConnectable={false} />
    </>
  );
  if (compact) {
    return (
      <div
        className={cx(
          "flex h-[76px] w-[200px] items-center justify-center gap-3 rounded-full border bg-panel transition-opacity duration-200",
          selected ? "border-accent" : "border-line-strong",
          (dim || n.unformed) && "opacity-35",
        )}
      >
        {handles}
        {reflexish ? <Lamp strength={n.strength} hollow={hollow} glow size={26} /> : <span className="lbl text-[18px]">{n.tag.split(" ")[0]}</span>}
      </div>
    );
  }
  return (
    <div
      key={pulse}
      className={cx(
        "flex h-[76px] w-[200px] flex-col gap-[5px] rounded-[10px] border bg-panel px-3 py-2.5 transition-[opacity,border-color,box-shadow] duration-200",
        selected
          ? "border-[1.5px] border-accent shadow-[0_0_0_4px_var(--sel),0_8px_30px_var(--sel)]"
          : "border-line-strong hover:border-accent/45",
        n.unformed && "border-dashed opacity-40",
        dim && !n.unformed && "opacity-40",
        pulse > 0 && "node-flash",
      )}
      data-kind={n.kind}
      data-testid={reflexish ? `node-${n.title}` : undefined}
    >
      {handles}
      <div className="truncate font-mono text-[10px] leading-3 tracking-[0.12em] text-ink-3">{n.tag}</div>
      <div className="flex h-4 min-w-0 items-center gap-1.5">
        {reflexish && <Lamp strength={n.strength} hollow={hollow} glow={!hollow} />}
        {reflexish && <KindGlyph instinct={n.kind === "instinct"} />}
        <span className={cx("truncate text-[12.5px] leading-4 text-ink-1", n.kind === "stimulus" ? "font-body" : "font-mono")}>{n.title}</span>
      </div>
      <div
        className={cx(
          "truncate font-mono text-[11px] leading-[14px]",
          n.strike ? "text-inhibit line-through" : "text-ink-2",
        )}
      >
        {n.sub || " "}
      </div>
    </div>
  );
}

export const MapCard = memo(function MapCard({ data }: NodeProps<FlowNode>) {
  return <Card data={data} />;
});

export const nodeTypes = { stimulus: MapCard, reflex: MapCard, instinct: MapCard, response: MapCard };

/** An arc: dim at rest, violet when its reflex is selected; a pulse travels it when it fires. */
export const ArcEdge = memo(function ArcEdge(p: EdgeProps<FlowEdge>) {
  const [path] = getBezierPath({
    sourceX: p.sourceX,
    sourceY: p.sourceY,
    sourcePosition: p.sourcePosition,
    targetX: p.targetX,
    targetY: p.targetY,
    targetPosition: p.targetPosition,
  });
  const d = p.data;
  const hot = Boolean(d?.hot);
  const color = hot ? "var(--color-accent-soft)" : "var(--edge)";
  const lamp = d?.strength ? LAMP_VAR[d.strength] : "var(--color-accent-soft)";
  return (
    <>
      <BaseEdge
        id={p.id}
        path={path}
        style={{
          stroke: color,
          strokeWidth: hot ? 2.2 : 1.4,
          opacity: d?.dim ? 0.25 : d?.unformed ? 0.35 : 1,
          strokeDasharray: d?.unformed ? "4 5" : undefined,
          transition: "stroke 200ms, opacity 200ms",
        }}
      />
      {d && d.pulse > 0 && (
        <g key={d.pulse} aria-hidden="true">
          <path d={path} pathLength={1} className="edge-draw" style={{ stroke: lamp, strokeWidth: 2.4, fill: "none", opacity: d.control ? 0.45 : 0.9 }} />
          <circle r={d.control ? 3 : 4} style={{ fill: "var(--color-ink-1)", filter: `drop-shadow(0 0 5px ${lamp})` }} opacity={d.control ? 0.6 : 1}>
            <animateMotion dur="0.9s" path={path} fill="freeze" keyPoints="0;1" keyTimes="0;1" calcMode="spline" keySplines="0.25 1 0.5 1" />
            <animate attributeName="opacity" from="1" to="0" begin="0.75s" dur="0.4s" fill="freeze" />
          </circle>
        </g>
      )}
    </>
  );
});

export const edgeTypes = { arc: ArcEdge };
