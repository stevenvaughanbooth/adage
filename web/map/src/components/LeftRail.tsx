import { type KeyboardEvent, useMemo } from "react";
import type { Graph, Route } from "../api/types";
import { counts, routeKey, visible } from "../lib/graph";
import { type Lifecycle, STRENGTHS, type Strength, isInstinct, lifecycle, strength } from "../lib/words";
import { useUrl } from "../state/url";
import { KindGlyph, Lamp } from "./glyphs";
import { FilterChip, cx } from "./ui";

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

/** The tree: tracks, reflexes waiting for review, instincts, packs. */
export function tree(rs: Route[]): Group[] {
  const nascent = rs.filter((r) => lifecycle(r.state) === "nascent");
  const rest = rs.filter((r) => lifecycle(r.state) !== "nascent");
  const learned = rest.filter((r) => !isInstinct(r) && !r.imported);
  const packs = rest.filter((r) => !isInstinct(r) && r.imported);
  const instincts = rest.filter(isInstinct);
  const out: Group[] = [];
  out.push(...groupBy(learned, "Tracks", (r) => `@${r.version}`));
  if (nascent.length) out.push({ section: "Nascent · wait for review", items: nascent });
  out.push(...groupBy(instincts, "Instincts", (r) => `@${r.version}`));
  out.push(...groupBy(packs, "Packs", (r) => `@${r.version} · imported`));
  return out;
}

const HIDEABLE: Lifecycle[] = ["nascent", "dormant", "extinguished"];

export function LeftRail({ graph }: { graph: Graph }) {
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
  const filters = { repo: url.repo, strengths: url.strengths, show: url.show, query: url.q };

  const toggleStrength = (s: Strength) => {
    const next = new Set(url.strengths);
    if (next.has(s)) next.delete(s);
    else next.add(s);
    setUrl({ strengths: next });
  };
  const toggleShow = (l: Lifecycle) => {
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
    <aside aria-label="Reflexes and instincts" className="flex min-h-0 flex-col gap-3.5 border-r border-line bg-panel p-4">
      <div className="flex flex-col gap-1.5">
        <label htmlFor="filter" className="lbl">
          Filter
        </label>
        <input
          id="filter"
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
        {HIDEABLE.filter((l) => l !== "extinguished" || (c.lifecycles.get(l) ?? 0) > 0).map((l) => (
          <FilterChip key={l} pressed={url.show.has(l)} onClick={() => toggleShow(l)} count={c.lifecycles.get(l) ?? 0}>
            {l}
          </FilterChip>
        ))}
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
              const s = strength(r.aspect);
              const onMap = visible(r, filters);
              const l = lifecycle(r.state);
              return (
                <button
                  key={key}
                  type="button"
                  data-reflex={key}
                  aria-current={sel ? "true" : undefined}
                  title={onMap ? r.summary || r.id : `${r.id} · hidden on the map by the filters`}
                  onClick={() => setUrl({ reflex: sel ? null : key, view: "map" }, true)}
                  className={cx(
                    "flex h-[30px] w-full items-center gap-2 rounded-sm pr-2 text-left transition-colors duration-100",
                    g.title ? "pl-[22px]" : "pl-2",
                    sel ? "bg-[var(--sel)] shadow-[inset_0_0_0_1px_var(--sel-ring)]" : "hover:bg-raised",
                    !onMap && "opacity-55",
                  )}
                >
                  <Lamp strength={s} hollow={l === "nascent" || l === "extinguished"} size={7} />
                  <KindGlyph instinct={isInstinct(r)} />
                  <span className="grow truncate font-mono text-xs text-ink-1">{r.id}</span>
                  <span className="shrink-0 font-mono text-[11px] text-ink-3">
                    {r.evidence.support}/{r.evidence.opportunities}
                  </span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>
    </aside>
  );
}
