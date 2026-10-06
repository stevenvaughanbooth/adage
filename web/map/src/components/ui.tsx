// Small shared pieces: chips, segmented controls, copy buttons, the toast, dialogs.

import { type KeyboardEvent, type ReactNode, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useUrl } from "../state/url";
import { Icons, Lamp } from "./glyphs";
import { type Lifecycle, type Strength, prose } from "../lib/words";

export function cx(...c: (string | false | null | undefined)[]): string {
  return c.filter(Boolean).join(" ");
}

const LAMP_CHIP: Record<Strength, string> = {
  fire: "bg-fire/10 border-fire/35",
  prime: "bg-prime/10 border-prime/35",
  inhibit: "bg-inhibit/10 border-inhibit/35",
};

/** fire / prime / inhibit with its lamp: strength is always spelled out, never colour alone. */
export function StrengthChip({ strength, hollow = false }: { strength: Strength | null | undefined; hollow?: boolean }) {
  if (!strength) return null;
  return (
    <span
      className={cx(
        "inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-xs text-ink-1",
        hollow ? "border-line-strong" : LAMP_CHIP[strength],
      )}
    >
      <Lamp strength={strength} hollow={hollow} glow={!hollow} size={7} />
      {strength}
    </span>
  );
}

export function Pill({ children, mono = false, title }: { children: ReactNode; mono?: boolean; title?: string }) {
  return (
    <span
      title={title}
      className={cx(
        "inline-flex h-6 max-w-full items-center truncate rounded-full border border-line-strong px-2.5 text-ink-2",
        mono ? "font-mono text-[11px]" : "text-xs",
      )}
    >
      {children}
    </span>
  );
}

export function LifecycleChip({ lifecycle }: { lifecycle: Lifecycle | "quiet" }) {
  return <Pill>{lifecycle}</Pill>;
}

/** A toggle chip for the filters. */
export function FilterChip({
  pressed,
  onClick,
  strength,
  children,
  count,
}: {
  pressed: boolean;
  onClick: () => void;
  strength?: Strength;
  children: ReactNode;
  count?: number;
}) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      onClick={onClick}
      className={cx(
        "inline-flex h-[26px] items-center gap-1.5 rounded-full border px-2.5 text-xs transition-colors duration-100",
        pressed && strength ? LAMP_CHIP[strength] + " text-ink-1" : "",
        pressed && !strength ? "border-accent/45 bg-accent/12 text-ink-1" : "",
        !pressed ? "border-line-strong bg-transparent text-ink-2 hover:text-ink-1" : "",
      )}
    >
      {strength && <Lamp strength={strength} hollow={!pressed} size={7} />}
      {children}
      {count !== undefined && <span className={pressed ? "text-ink-1" : "text-ink-3"}>{count}</span>}
    </button>
  );
}

export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  size = "md",
}: {
  label: string;
  value: T;
  options: { value: T; label: ReactNode; title?: string }[];
  onChange: (v: T) => void;
  size?: "sm" | "md";
}) {
  return (
    <div role="group" aria-label={label} className="flex rounded-md border border-line-strong bg-panel p-[3px]">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          title={o.title}
          aria-pressed={value === o.value}
          onClick={() => onChange(o.value)}
          className={cx(
            "rounded-sm px-2.5 transition-colors duration-100",
            size === "sm" ? "h-6 text-xs" : "h-[26px] text-[12.5px]",
            value === o.value ? "bg-[var(--sel)] text-ink-1 shadow-[inset_0_0_0_1px_var(--sel-ring)]" : "text-ink-2 hover:text-ink-1",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

// The toast: one line, announced politely.
let toastText = "";
let toastSeq = 0;
const toastListeners = new Set<() => void>();
export function toast(text: string) {
  toastText = text;
  toastSeq++;
  for (const l of toastListeners) l();
}
export function Toast() {
  const seq = useSyncExternalStore(
    (f) => {
      toastListeners.add(f);
      return () => toastListeners.delete(f);
    },
    () => toastSeq,
  );
  const [shown, setShown] = useState(false);
  useEffect(() => {
    if (!seq) return;
    setShown(true);
    const t = setTimeout(() => setShown(false), 2400);
    return () => clearTimeout(t);
  }, [seq]);
  return (
    <div
      role="status"
      aria-live="polite"
      className={cx(
        "pointer-events-none fixed bottom-24 left-1/2 z-50 -translate-x-1/2 rounded-md border border-line-strong bg-raised px-3.5 py-2 font-mono text-xs text-ink-1 shadow-[var(--shadow)] transition-all duration-200",
        shown ? "translate-y-0 opacity-100" : "translate-y-2 opacity-0",
      )}
    >
      {shown ? toastText : ""}
    </div>
  );
}

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // An older browser, or no permission: select it in a hidden field and copy.
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.setAttribute("readonly", "");
    ta.className = "sr-only";
    document.body.appendChild(ta);
    ta.select();
    let ok = false;
    try {
      ok = document.execCommand("copy");
    } catch {
      ok = false;
    }
    ta.remove();
    return ok;
  }
}

export async function copyCommand(cmd: string) {
  const ok = await copyText(cmd);
  toast(ok ? `Copied · ${cmd}` : `Copy this: ${cmd}`);
}

/** A command the reader can run, with a copy button: the map itself never changes anything. */
export function CommandButton({ command, primary = false, why }: { command: string; primary?: boolean; why?: string }) {
  const [, setUrl] = useUrl();
  const [done, setDone] = useState(false);
  useEffect(() => {
    if (!done) return;
    const t = setTimeout(() => setDone(false), 1400);
    return () => clearTimeout(t);
  }, [done]);
  return (
    <div className="flex flex-col gap-1.5">
      <button type="button" onClick={() => setUrl({ view: "setup" }, true)} className={cx("rounded-md border px-3 py-2 text-left text-sm", primary ? "border-accent/45 bg-[var(--sel)]" : "border-line-strong")}>
        <span className="block text-accent-soft">{why || "Manage this in the terminal"}</span>
        <span className="text-xs text-ink-3">Open terminal action guide →</span>
      </button>
      <details><summary className="cursor-pointer text-xs text-ink-3">CLI alternative</summary>
    <button
      type="button"
      title={why ? `${why} · copies the command` : "Copies the command"}
      onClick={() => {
        void copyCommand(command);
        setDone(true);
      }}
      className={cx(
        "group flex min-h-10 w-full items-center gap-2.5 rounded-md border px-3 py-2 text-left transition-colors duration-100",
        primary
          ? "border-accent/45 bg-[var(--sel)] text-ink-1 hover:border-accent"
          : "border-line-strong text-ink-1 hover:border-accent/45 hover:bg-raised",
      )}
    >
      <span className="text-accent-soft">{done ? <Icons.check /> : <Icons.copy />}</span>
      <span className="min-w-0 flex-1">
        <span className="block truncate font-mono text-xs">{command}</span>
        {why && <span className="block truncate text-[11.5px] text-ink-3">{why}</span>}
      </span>
      <span className="sr-only">Copy</span>
    </button>
      </details>
    </div>
  );
}

const FOCUSABLE = "input, button:not([disabled]), a[href], select, textarea, [tabindex]:not([tabindex='-1'])";

/** A modal dialog: focus moves in and stays, Escape and the backdrop close it, focus returns after. */
export function Modal({
  label,
  open,
  onClose,
  children,
  className,
  top = 64,
  sheet = false,
  focusInput = true,
}: {
  label: string;
  open: boolean;
  onClose: () => void;
  children: ReactNode;
  className?: string;
  top?: number;
  /** A drawer from the left edge, full height, instead of a centred dialog. */
  sheet?: boolean;
  /** Focus the first field on open (the palette's search); otherwise the first control. */
  focusInput?: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const back = useRef<Element | null>(null);
  const withInput = useRef(focusInput);
  withInput.current = focusInput;
  useEffect(() => {
    if (!open) return;
    back.current = document.activeElement;
    const el = ref.current;
    // A field first (the palette's search), else the first control; never a tabindex="-1" wrapper.
    const first = (withInput.current ? el?.querySelector<HTMLElement>("input") : null) ?? el?.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? el)?.focus();
    return () => {
      (back.current as HTMLElement | null)?.focus?.();
    };
  }, [open]);
  if (!open) return null;
  // Tab and Shift-Tab stay inside the dialog while it is open.
  const trap = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key !== "Tab" || !ref.current) return;
    const items = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((x) => x.offsetParent !== null);
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return;
    if (e.shiftKey && (document.activeElement === first || document.activeElement === ref.current)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };
  return (
    <div className="fixed inset-0 z-40" onKeyDown={trap}>
      <div className="absolute inset-0 bg-base/60 backdrop-blur-[2px]" onClick={onClose} aria-hidden="true" />
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-label={label}
        tabIndex={-1}
        style={sheet ? undefined : { top }}
        className={cx(
          sheet
            ? "sheet absolute inset-y-0 left-0 flex w-[min(360px,calc(100vw-40px))] flex-col overflow-hidden border-r border-line-strong bg-panel shadow-[var(--shadow)] outline-none"
            : "rise absolute left-1/2 flex w-[min(640px,calc(100vw-32px))] -translate-x-1/2 flex-col overflow-hidden rounded-xl border border-line-strong bg-panel shadow-[var(--shadow)] outline-none",
          className,
        )}
      >
        {children}
      </div>
    </div>
  );
}

/** Stored prose in the lexicon's words, with `commands` set in mono. */
export function Prose({ text }: { text: string }) {
  return (
    <>
      {prose(text)
        .split("`")
        .map((part, i) =>
          i % 2 === 1 ? (
            <code key={i} className="font-mono text-[0.95em] text-ink-1">
              {part}
            </code>
          ) : (
            part
          ),
        )}
    </>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd>{children}</kbd>;
}

export function relTime(ts: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ts) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 48) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
export function day(ts: number): string {
  const d = new Date(ts);
  return `${MONTHS[d.getMonth()]} ${d.getDate()}`;
}
export function clock(ts: number): string {
  const d = new Date(ts);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}
