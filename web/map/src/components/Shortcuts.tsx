import { SHORTCUTS } from "../state/keys";
import { Modal } from "./ui";

export function Shortcuts({ open, onClose }: { open: boolean; onClose: () => void }) {
  const groups = ["Everywhere", "Go to", "Map"] as const;
  return (
    <Modal label="Keyboard shortcuts" open={open} onClose={onClose} top={80}>
      <div className="flex items-center justify-between border-b border-line px-5 py-4">
        <h2 className="display m-0 text-xl font-semibold">Keyboard shortcuts</h2>
        <button type="button" onClick={onClose} className="rounded-md px-2 py-1 text-xs text-ink-2 hover:text-ink-1">
          <kbd>esc</kbd> <span className="sr-only">Close</span>
        </button>
      </div>
      <div className="grid gap-x-8 gap-y-5 p-5 sm:grid-cols-2">
        {groups.map((g) => (
          <section key={g} className="flex flex-col gap-2" aria-label={g}>
            <h3 className="lbl m-0 font-normal">{g}</h3>
            <dl className="m-0 flex flex-col gap-1.5">
              {SHORTCUTS.filter((s) => s.group === g).map((s) => (
                <div key={s.label} className="flex items-center justify-between gap-4 text-[13px]">
                  <dt className="text-ink-2">{s.label}</dt>
                  <dd className="m-0 flex shrink-0 gap-1">
                    {s.keys.map((k) => (
                      <kbd key={k}>{k}</kbd>
                    ))}
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
      </div>
    </Modal>
  );
}
