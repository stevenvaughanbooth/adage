// Glyphs and icons as inline SVG, coloured from the tokens only.

import type { CSSProperties, ReactNode } from "react";
import type { Strength } from "../lib/words";

/** A reflex: an arc with a dot at its peak, landing on a green dot (style guide ◠). */
export function ReflexGlyph({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 14 14" fill="none" aria-hidden="true" className="shrink-0">
      <path d="M2 12 Q7 -1 12 12" style={{ stroke: "var(--glyph-arc)" }} strokeWidth="1.4" strokeLinecap="round" />
      <circle cx="7" cy="5.5" r="1.6" style={{ fill: "var(--color-accent-soft)" }} />
      <circle cx="12" cy="12" r="1.6" style={{ fill: "var(--color-lamp-fire)" }} />
    </svg>
  );
}

/** An instinct: a dot in a ring (style guide ◉). */
export function InstinctGlyph({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 14 14" fill="none" aria-hidden="true" className="shrink-0">
      <circle cx="7" cy="7" r="5.2" style={{ stroke: "var(--glyph-arc)" }} strokeWidth="1.4" />
      <circle cx="7" cy="7" r="2.2" style={{ fill: "var(--color-accent-soft)" }} />
    </svg>
  );
}

/** A procedure card: a sheet with ruled lines (style guide ▤). */
export function CardGlyph({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 14 14" fill="none" aria-hidden="true" className="shrink-0">
      <rect x="1.8" y="1.8" width="10.4" height="10.4" rx="1.6" style={{ stroke: "var(--glyph-arc)" }} strokeWidth="1.3" />
      <path d="M4.3 5h5.4M4.3 7h5.4M4.3 9h3.4" style={{ stroke: "var(--color-accent-soft)" }} strokeWidth="1.2" strokeLinecap="round" />
    </svg>
  );
}

/** Quiet: uplift below the bar, waiting for a person (style guide ◌). Takes the lamp's place. */
export function QuietGlyph({ size = 8 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 10 10" fill="none" aria-hidden="true" className="shrink-0">
      <circle cx="5" cy="5" r="4" style={{ stroke: "var(--color-ink-3)" }} strokeWidth="1.4" strokeDasharray="1.6 1.5" />
    </svg>
  );
}

/** A rewrite: the command was changed before it ran (style guide ↪). */
export function RewriteGlyph({ size = 14 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 14 14" fill="none" aria-hidden="true" className="shrink-0">
      <path d="M3 3v3.2a2.3 2.3 0 0 0 2.3 2.3H11" style={{ stroke: "currentColor" }} strokeWidth="1.4" strokeLinecap="round" />
      <path d="M8.6 6.1L11 8.5 8.6 10.9" style={{ stroke: "currentColor" }} strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function KindGlyph({ instinct, card = false, size }: { instinct: boolean; card?: boolean; size?: number }) {
  if (card) return <CardGlyph size={size} />;
  return instinct ? <InstinctGlyph size={size} /> : <ReflexGlyph size={size} />;
}

/** The lamp, or the quiet mark in its place. */
export function StateMark({ strength, hollow, quiet, glow, size = 8 }: { strength: Strength | null | undefined; hollow: boolean; quiet: boolean; glow?: boolean; size?: number }) {
  return quiet ? <QuietGlyph size={size + 1} /> : <Lamp strength={strength} hollow={hollow} glow={glow} size={size} />;
}

export const LAMP_VAR: Record<Strength, string> = {
  fire: "var(--color-lamp-fire)",
  prime: "var(--color-lamp-prime)",
  inhibit: "var(--color-lamp-inhibit)",
};

/** A strength lamp; hollow when the reflex is not delivered (nascent). */
export function Lamp({
  strength,
  hollow = false,
  glow = false,
  size = 8,
}: {
  strength: Strength | null | undefined;
  hollow?: boolean;
  glow?: boolean;
  size?: number;
}) {
  const c = strength ? LAMP_VAR[strength] : "var(--color-ink-3)";
  const style: CSSProperties = hollow
    ? { width: size, height: size, border: `1.5px solid var(--color-ink-3)`, boxSizing: "border-box" }
    : { width: size, height: size, background: c, color: c, boxShadow: glow ? `0 0 8px ${c}` : undefined };
  return <span aria-hidden="true" className="inline-block shrink-0 rounded-full" style={style} />;
}

function Icon({ children, size = 14, label }: { children: ReactNode; size?: number; label?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden={label ? undefined : true}
      role={label ? "img" : undefined}
      aria-label={label}
      className="shrink-0"
    >
      {children}
    </svg>
  );
}

export const Icons = {
  search: (p: { size?: number }) => (
    <Icon {...p}>
      <circle cx="7" cy="7" r="4.6" />
      <path d="M10.6 10.6L14 14" />
    </Icon>
  ),
  chevron: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M4 6l4 4 4-4" />
    </Icon>
  ),
  moon: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M13.5 9.5A5.5 5.5 0 0 1 6.5 2.5a5.5 5.5 0 1 0 7 7z" />
    </Icon>
  ),
  sun: (p: { size?: number }) => (
    <Icon {...p}>
      <circle cx="8" cy="8" r="3" />
      <path d="M8 1.5v1.6M8 12.9v1.6M1.5 8h1.6M12.9 8h1.6M3.4 3.4l1.1 1.1M11.5 11.5l1.1 1.1M3.4 12.6l1.1-1.1M11.5 4.5l1.1-1.1" />
    </Icon>
  ),
  system: (p: { size?: number }) => (
    <Icon {...p}>
      <rect x="2" y="2.5" width="12" height="8.5" rx="1.5" />
      <path d="M5.5 14h5M8 11v3" />
    </Icon>
  ),
  copy: (p: { size?: number }) => (
    <Icon {...p}>
      <rect x="5.5" y="5.5" width="8.5" height="8.5" rx="1.5" />
      <path d="M10.5 3.2V3a1 1 0 0 0-1-1H3a1 1 0 0 0-1 1v6.5a1 1 0 0 0 1 1h.3" />
    </Icon>
  ),
  check: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M3 8.3L6.4 11.5L13 4.5" />
    </Icon>
  ),
  filter: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M2.5 4h11M4.5 8h7M6.5 12h3" />
    </Icon>
  ),
  cross: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M4 4l8 8M12 4l-8 8" />
    </Icon>
  ),
  warn: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M8 3v6M8 12.2v.3" />
    </Icon>
  ),
  play: (p: { size?: number }) => (
    <svg width={p.size ?? 12} height={p.size ?? 12} viewBox="0 0 12 12" aria-hidden="true">
      <path d="M3 1.8L10 6L3 10.2z" fill="currentColor" />
    </svg>
  ),
  pause: (p: { size?: number }) => (
    <svg width={p.size ?? 12} height={p.size ?? 12} viewBox="0 0 12 12" aria-hidden="true">
      <rect x="2.5" y="2" width="2.5" height="8" rx="0.6" fill="currentColor" />
      <rect x="7" y="2" width="2.5" height="8" rx="0.6" fill="currentColor" />
    </svg>
  ),
  plus: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M8 3v10M3 8h10" />
    </Icon>
  ),
  minus: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M3 8h10" />
    </Icon>
  ),
  fit: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M2 6V2h4M10 2h4v4M14 10v4h-4M6 14H2v-4" />
    </Icon>
  ),
  live: (p: { size?: number }) => (
    <Icon {...p}>
      <path d="M8 8h.01M5.2 5.2a4 4 0 0 0 0 5.6M10.8 5.2a4 4 0 0 1 0 5.6" />
    </Icon>
  ),
  keyboard: (p: { size?: number }) => (
    <Icon {...p}>
      <rect x="1.5" y="4" width="13" height="8.5" rx="1.5" />
      <path d="M4.5 7h.01M7 7h.01M9.5 7h.01M12 7h.01M5 10h6" />
    </Icon>
  ),
};
