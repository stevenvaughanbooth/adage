// ⌘K: reflexes, actions and views in one list. Decisions are copied as adage commands; the map
// stays read-only.

import { Command } from "cmdk";
import { useState } from "react";
import type { Graph } from "../api/types";
import { routeKey } from "../lib/graph";
import { STRENGTHS, type Strength, isInstinct, lifecycle, orderedVerbs, reviewCommand, strength, verb } from "../lib/words";
import { type View, useUrl } from "../state/url";
import { THEMES, setTheme } from "../state/theme";
import { KindGlyph, Lamp } from "./glyphs";
import { Modal, copyCommand } from "./ui";

function Hl({ text, q }: { text: string; q: string }) {
  const i = q ? text.toLowerCase().indexOf(q.toLowerCase()) : -1;
  if (i < 0) return <>{text}</>;
  return (
    <>
      {text.slice(0, i)}
      <b className="font-semibold text-accent-soft">{text.slice(i, i + q.length)}</b>
      {text.slice(i + q.length)}
    </>
  );
}

const GROUP = "[&_[cmdk-group-heading]]:lbl [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:pt-3 [&_[cmdk-group-heading]]:pb-1.5";
const ITEM =
  "flex h-10 cursor-pointer items-center gap-3 rounded-md px-3 text-sm text-ink-1 data-[selected=true]:bg-[var(--sel)] aria-disabled:opacity-50";

export function Palette({ graph, open, onClose, onShortcuts }: { graph: Graph | null; open: boolean; onClose: () => void; onShortcuts: () => void }) {
  const [url, setUrl] = useUrl();
  const [q, setQ] = useState("");
  const close = () => {
    setQ("");
    onClose();
  };
  const run = (f: () => void) => () => {
    f();
    close();
  };
  const routes = (graph?.routes ?? []).filter((r) => !url.repo || r.repo === url.repo);
  const go = (view: View) => run(() => setUrl({ view }, true));
  const only = (s: Strength) => run(() => setUrl({ strengths: new Set([s]), view: "map" }));
  const decisions = q.trim().length >= 2 ? routes.flatMap((r) => orderedVerbs(r).map((v) => ({ r, v }))) : [];
  return (
    <Modal label="Command palette" open={open} onClose={close}>
      <Command label="Search commands and reflexes" loop className="flex flex-col">
        <div className="flex h-14 items-center gap-3 border-b border-line px-[18px]">
          <span className="text-ink-2" aria-hidden="true">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round">
              <circle cx="7" cy="7" r="4.6" />
              <path d="M10.6 10.6L14 14" />
            </svg>
          </span>
          <Command.Input
            value={q}
            onValueChange={setQ}
            placeholder="Search reflexes, actions, views…"
            className="h-10 grow border-0 bg-transparent text-[17px] text-ink-1 outline-none placeholder:text-ink-3"
          />
          <kbd>esc</kbd>
        </div>
        <Command.List className="scroll-thin max-h-[min(460px,60vh)] overflow-y-auto p-2">
          <Command.Empty className="px-3 py-6 text-center text-sm text-ink-2">Nothing matches “{q}”.</Command.Empty>
          {routes.length > 0 && (
            <Command.Group heading="Reflexes" className={GROUP}>
              {routes.map((r) => {
                const l = lifecycle(r.state);
                return (
                  <Command.Item
                    key={routeKey(r)}
                    value={`${r.id} ${r.trigger.label} ${r.step.label} ${r.track} ${routeKey(r)}`}
                    onSelect={run(() => setUrl({ reflex: routeKey(r), view: "map" }, true))}
                    className={`${ITEM} h-11`}
                  >
                    <Lamp strength={strength(r.strength)} hollow={l === "nascent"} glow={l !== "nascent"} />
                    <KindGlyph instinct={isInstinct(r)} />
                    <span className="shrink-0 font-mono text-[13.5px]">
                      <Hl text={r.id} q={q.trim()} />
                    </span>
                    <span className="grow truncate text-[12.5px] text-ink-2">
                      {r.step.kind === "avoid" ? `${r.step.avoid} → ${r.step.label}` : `${r.trigger.label.replace(/`/g, "")} → ${r.step.label}`}
                    </span>
                    <kbd>↵</kbd>
                  </Command.Item>
                );
              })}
            </Command.Group>
          )}
          <Command.Group heading="Actions" className={GROUP}>
            {decisions.map(({ r, v }) => (
              <Command.Item
                key={`${routeKey(r)}:${v}`}
                value={`${v} ${r.id} ${verb(v)}`}
                onSelect={run(() => void copyCommand(reviewCommand(v, r)))}
                className={ITEM}
              >
                <span className="grow truncate">
                  {v[0]?.toUpperCase()}
                  {v.slice(1)} <Hl text={r.id} q={q.trim()} />…
                </span>
                <span className="font-mono text-xs text-ink-3">copies adage review {v}</span>
              </Command.Item>
            ))}
            {STRENGTHS.map((s) => (
              <Command.Item key={s} value={`show only ${s} strength filter`} onSelect={only(s)} className={ITEM}>
                <Lamp strength={s} />
                <span className="grow">Show only {s} reflexes</span>
                <span className="font-mono text-xs text-ink-3">filter · {s}</span>
              </Command.Item>
            ))}
            <Command.Item value="show all strengths reset filters" onSelect={run(() => setUrl({ strengths: new Set(STRENGTHS), q: "" }))} className={ITEM}>
              <span className="grow">Show every strength</span>
              <span className="font-mono text-xs text-ink-3">filter · reset</span>
            </Command.Item>
            <Command.Item
              value="show nascent reflexes waiting for review"
              onSelect={run(() => setUrl({ show: new Set([...url.show, "nascent"]), view: "map" }))}
              className={ITEM}
            >
              <span className="grow">Show nascent reflexes (waiting for review)</span>
              <span className="font-mono text-xs text-ink-3">filter · nascent</span>
            </Command.Item>
            <Command.Item
              value="aggregated expanded graph mode toggle"
              onSelect={run(() => setUrl({ mode: url.mode === "aggregated" ? "expanded" : "aggregated", view: "map" }))}
              className={ITEM}
            >
              <span className="grow">{url.mode === "aggregated" ? "Expand every reflex" : "Aggregate shared stimuli and responses"}</span>
              <kbd>a</kbd>
            </Command.Item>
            {THEMES.map((t) => (
              <Command.Item key={t} value={`theme ${t} appearance`} onSelect={run(() => setTheme(t))} className={ITEM}>
                <span className="grow">Theme: {t}</span>
              </Command.Item>
            ))}
            <Command.Item value="copy adage condition learn" onSelect={run(() => void copyCommand("adage condition"))} className={ITEM}>
              <span className="grow">Condition from earlier traces</span>
              <span className="font-mono text-xs text-ink-3">copies adage condition</span>
            </Command.Item>
          </Command.Group>
          <Command.Group heading="Go to" className={GROUP}>
            <Command.Item value="go map view" onSelect={go("map")} className={ITEM}>
              <span className="grow">Map</span>
              <kbd>g</kbd>
              <kbd>m</kbd>
            </Command.Item>
            <Command.Item value="go traces replay timeline" onSelect={go("traces")} className={ITEM}>
              <span className="grow">Traces · replay the map</span>
              <kbd>g</kbd>
              <kbd>t</kbd>
            </Command.Item>
            <Command.Item value="go evidence" onSelect={go("evidence")} className={ITEM}>
              <span className="grow">Evidence</span>
              <kbd>g</kbd>
              <kbd>e</kbd>
            </Command.Item>
            <Command.Item value="go health doctor" onSelect={go("health")} className={ITEM}>
              <span className="grow">Health</span>
              <kbd>g</kbd>
              <kbd>h</kbd>
            </Command.Item>
            <Command.Item value="keyboard shortcuts help" onSelect={run(onShortcuts)} className={ITEM}>
              <span className="grow">Keyboard shortcuts</span>
              <kbd>?</kbd>
            </Command.Item>
          </Command.Group>
        </Command.List>
        <div className="flex h-10 items-center gap-4 border-t border-line px-[18px] text-xs text-ink-3">
          <span>
            <kbd>↑</kbd> <kbd>↓</kbd> move
          </span>
          <span>
            <kbd>↵</kbd> open
          </span>
          <span>
            <kbd>?</kbd> all shortcuts
          </span>
          <span className="grow" />
          <span className="hidden truncate md:inline">Changes are copied as adage commands; the map stays read-only</span>
        </div>
      </Command>
    </Modal>
  );
}
