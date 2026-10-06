import { type KeyboardEvent, useMemo } from "react";
import type { Graph, Route } from "../api/types";
import { counts, routeKey, visible } from "../lib/graph";
import { type Showable, STRENGTHS, type Strength, evidenceShort, evidenceShortWhy, isCard, isInstinct, isQuiet, lifecycle, strength } from "../lib/words";
import { useUrl } from "../state/url";
import { CardGlyph, Icons, KindGlyph, StateMark } from "./glyphs";
import { FilterChip, Modal, cx } from "./ui";

interface Group {
  section?: string;
  title?: string;
  ver?: string;
  items: Route[];
}

function groupBy(rs: Route[], section: string, ver: (r: Route) => string): Group[] {
  const by = new Map<string, Route[]>();
  for (const r of rs) by.set(r.track, [...(by.get(r.track) ?? []), r]);
  return [...by.entries()].map(([title, items], i) => ({
    section: i === 0 ? section : undefined,
    title,
    ver: ver(items[0] as Route),
    items,
  }));
}

/** The tree: tracks, reflexes waiting for review, quiet ones, instincts, packs. */
export function tree(rs: Route[]): Group[] {
  const waiting = rs.filter((r) => lifecycle(r.state) === "nascent");
  // Quiet is not an install step: agents already do it, so it waits for a person to want it said.
  const quiet = waiting.filter(isQuiet);
  const nascent = waiting.filter((r) => !isQuiet(r));
  const rest = rs.filter((r) => lifecycle(r.state) !== "nascent");
  const learned = rest.filter((r) => !isInstinct(r) && !r.imported);
  const packs = rest.filter((r) => !isInstinct(r) && r.imported);
  const instincts = rest.filter(isInstinct);
  const out: Group[] = [];
  out.push(...groupBy(learned, "Learned reflexes", (r) => `@${r.version}`));
  if (nascent.length) out.push({ section: "Nascent · wait for review", items: nascent });
  if (quiet.length) out.push({ section: "Quiet · agents already do this", items: quiet });
  out.push(...groupBy(instincts, "Instincts", (r) => `@${r.version}`));
  out.push(...groupBy(packs, "Packs", (r) => `@${r.version} · imported`));
  return out;
}

const HIDEABLE: Showable[] = ["nascent", "quiet", "dormant", "extinguished"];

/** The filters hidden on the map, as a count for the Filter button on narrow screens. */
export function activeFilters(url: { q: string; strengths: Set<Strength>; show: Set<Showable>; cards?: boolean }): number {
  return (url.q ? 1 : 0) + STRENGTHS.filter((s) => !url.strengths.has(s)).length + url.show.size + (url.cards === false ? 1 : 0);
}

/** The rail on wide screens (1024 px and up). */
export function LeftRail({ graph }: { graph: Graph }) {
  return (
    <aside aria-label="Reflexes and instincts" className="hidden min-h-0 flex-col gap-3.5 border-r border-line bg-panel p-4 lg:flex">
      <RailBody graph={graph} inputId="filter" />
    </aside>
  );
}

/** The same filters and tree on tablets and phones: a drawer from the left. */
export function FilterSheet({ graph, open, onClose }: { graph: Graph; open: boolean; onClose: () => void }) {
  return (
    <Modal label="Filter and reflex tree" open={open} onClose={onClose} sheet focusInput={false}>
      <div className="flex shrink-0 items-center justify-between border-b border-line py-3 pr-3 pl-4">
        <h2 className="display m-0 text-lg font-semibold">Filter</h2>
        <button
          type="button"
          onClick={onClose}
          className="flex h-9 items-center gap-1.5 rounded-md border border-line-strong px-3 text-[13px] text-ink-1 hover:border-accent/45"
        >
          <Icons.cross size={13} />
          Close
        </button>
      </div>
      <div className="flex min-h-0 grow flex-col gap-3.5 p-4">
        <RailBody graph={graph} inputId="filter-sheet" onPick={onClose} />
      </div>
    </Modal>
  );
}

function RailBody({ graph, inputId, onPick }: { graph: Graph; inputId: string; onPick?: () => void }) {
  const [url, setUrl] = useUrl();
  const c = counts(graph, url.repo);
  const inRepo = useMemo(
    () =>
      graph.routes.filter(
        (r) =>
          (!url.repo || r.repo === url.repo) &&
          visible(r, { repo: url.repo, strengths: new Set(STRENGTHS), show: new Set(HIDEABLE), query: url.q }),
      ),
    [graph, url.repo, url.q],
  );
  const groups = tree(inRepo);
  const filters = { repo: url.repo, strengths: url.strengths, show: url.show, query: url.q, cards: url.cards };

  const toggleStrength = (s: Strength) => {
    const next = new Set(url.strengths);
    if (next.has(s)) next.delete(s);
    else next.add(s);
    setUrl({ strengths: next });
  };
  const toggleShow = (l: Showable) => {
    const next = new Set(url.show);
    if (next.has(l)) next.delete(l);
    else next.add(l);
    setUrl({ show: next });
  };

  // Up and down move through the tree.
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const items = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button[data-reflex]")];
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = items[Math.max(0, Math.min(items.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)))];
    if (next) {
      e.preventDefault();
      next.focus();
    }
  };

  return (
    <>
      <div className="flex flex-col gap-1.5">
        <label htmlFor={inputId} className="lbl">
          Filter
        </label>
        <input
          id={inputId}
          type="search"
          value={url.q}
          onChange={(e) => setUrl({ q: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Escape" && url.q) {
              e.stopPropagation();
              setUrl({ q: "" });
            }
          }}
          placeholder="Reflex, command, file…"
          autoComplete="off"
          spellCheck={false}
          className="h-[34px] rounded-md border border-line-strong bg-base px-2.5 font-mono text-[12.5px] text-ink-1 placeholder:text-ink-3 focus:border-accent focus:outline-none"
        />
      </div>
      <div className="flex flex-wrap gap-1.5" role="group" aria-label="Show on the map">
        {STRENGTHS.map((s) => (
          <FilterChip key={s} strength={s} pressed={url.strengths.has(s)} onClick={() => toggleStrength(s)} count={c.strengths.get(s) ?? 0}>
            {s}
          </FilterChip>
        ))}
        {HIDEABLE.filter((l) => (l !== "extinguished" && l !== "quiet") || (c.lifecycles.get(l) ?? 0) > 0).map((l) => (
          <FilterChip key={l} pressed={url.show.has(l)} onClick={() => toggleShow(l)} count={c.lifecycles.get(l) ?? 0}>
            {l}
          </FilterChip>
        ))}
        {c.cards > 0 && (
          <FilterChip pressed={url.cards} onClick={() => setUrl({ cards: !url.cards })} count={c.cards}>
            <CardGlyph size={12} />
            cards
          </FilterChip>
        )}
      </div>
      <nav aria-label="Reflex tree" className="scroll-thin -mx-1 flex min-h-0 flex-col gap-3 overflow-y-auto px-1 pb-2" onKeyDown={onKey}>
        {groups.length === 0 && <p className="px-1 text-[13px] text-ink-3">{url.q ? `Nothing matches “${url.q}”.` : "No reflexes yet."}</p>}
        {groups.map((g, gi) => (
          <div key={`${g.section ?? ""}${g.title ?? ""}${gi}`} className="flex flex-col gap-0.5">
            {g.section && <div className="lbl mt-0.5 mb-1.5">{g.section}</div>}
            {g.title && (
              <div className="flex items-center gap-1.5 px-1.5 py-1 font-mono text-xs text-ink-2">
                <span className="truncate text-ink-1">{g.title}</span>
                <span className="shrink-0">{g.ver}</span>
              </div>
            )}
            {g.items.map((r) => {
              const key = routeKey(r);
              const sel = url.reflex === key || url.reflex === r.id;
              const s = strength(r.strength);
              const onMap = visible(r, filters);
              const l = lifecycle(r.state);
              return (
                <button
                  key={key}
                  type="button"
                  data-reflex={key}
                  aria-current={sel ? "true" : undefined}
                  title={`${onMap ? r.summary || r.id : `${r.id} · hidden on the map by the filters`}${isQuiet(r) ? " · quiet: agents already do this" : ""} · ${evidenceShortWhy(r)}`}
                  onClick={() => {
                    setUrl({ reflex: sel ? null : key, view: "map" }, true);
                    onPick?.();
                  }}
                  className={cx(
                    "flex h-[30px] w-full items-center gap-2 rounded-sm pr-2 text-left transition-colors duration-100",
                    g.title ? "pl-[22px]" : "pl-2",
                    sel ? "bg-[var(--sel)] shadow-[inset_0_0_0_1px_var(--sel-ring)]" : "hover:bg-raised",
                  )}
                >
                  <StateMark strength={s} hollow={l === "nascent" || l === "extinguished"} quiet={isQuiet(r)} size={7} />
                  <KindGlyph instinct={isInstinct(r)} card={isCard(r)} />
                  {/* Hidden on the map: dimmer ink, never below the contrast floor. */}
                  <span className={cx("grow truncate font-mono text-xs", onMap ? "text-ink-1" : "text-ink-3")}>{r.id}</span>
                  <span className={cx("shrink-0 font-mono text-[11px]", sel ? "text-ink-2" : "text-ink-3")}>
                    {evidenceShort(r)}
                  </span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>
    </>
  );
}
