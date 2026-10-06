import type { Graph } from "../api/types";
import { findRoute } from "../lib/graph";
import { useUrl } from "../state/url";

/** A contextual handoff to the terminal's single palette; the map remains a read-only observer. */
export function Setup({ graph }: { graph: Graph | null }) {
  const [url, setUrl] = useUrl();
  const selected = findRoute(graph, url.reflex);
  const steps = [
    [
      "Choose your repository",
      "Open repository",
      "Choose the checkout you want Adage to learn from. The terminal shows the selected repository before changes.",
    ],
    [
      "Connect your agent",
      "Connect Claude Code / Codex / OpenCode",
      "Choose your harness and review the hook installation. For Codex, accept the registered hooks in /hooks. Registered hooks and a received hook are separate states in Health.",
    ],
    [
      "Check earlier sessions",
      "History sources",
      "See detected Claude Code and Codex histories, or configure a history directory. For OpenCode, choose Import OpenCode history to copy local exports; repeat the import when you want newer sessions.",
    ],
    [
      "Learn, then review",
      "Condition now",
      "Preview which sources and sessions will be read. Review proposed reflexes and their exact before-and-after responses in the terminal before accepting a change.",
    ],
  ];
  return (
    <main
      aria-label="Setup"
      className="scroll-thin flex min-h-0 grow flex-col gap-6 overflow-y-auto px-5 py-8 sm:px-10"
    >
      <div className="max-w-3xl">
        <span className="lbl">
          {graph?.demo ? "Demo → your workspace" : "Setup · this machine"}
        </span>
        <h1 className="display my-3 text-4xl font-semibold">
          Connect once. <span className="text-accent-soft">Work as usual.</span>
        </h1>
        <p className="text-ink-2">
          Use the Adage terminal palette to set up and change your workspace.
          This browser explains the evidence and shows live traces.
        </p>
        <div className="rounded-lg border border-accent/40 bg-panel p-4 text-sm leading-relaxed">
          {graph?.demo ? (
            <p className="m-0">
              You are exploring sample data. Return to the Adage terminal and
              choose <strong>Open repository</strong> before connecting your
              real repository.
            </p>
          ) : (
            <p className="m-0">
              Return to the Adage terminal, open its command palette, and search
              for the action below. Each action guides you through the details
              and confirmation.
            </p>
          )}
          <p className="mb-0 text-ink-2">
            If you launched this browser directly, start{" "}
            <code className="font-mono text-ink-1">adage ui</code> once in your
            terminal. All setup actions are then available in its palette; no
            command copying is needed between steps.
          </p>
        </div>
        {selected && (
          <p className="rounded-lg border border-prime/35 bg-panel p-4 text-sm">
            To review{" "}
            <strong className="font-mono break-all">{selected.id}</strong>,
            select it in the terminal's Reflexes view and choose Review. The
            confirmation shows the exact proposed change for this installed
            version.
          </p>
        )}
      </div>
      <ol className="m-0 grid max-w-5xl list-none gap-4 p-0 md:grid-cols-2">
        {steps.map(([title, action, detail], i) => (
          <li
            key={title}
            className="rounded-lg border border-line-strong bg-panel p-5"
          >
            <span className="lbl">Step {i + 1}</span>
            <h2 className="display my-2 text-xl font-semibold">{title}</h2>
            <p className="text-sm leading-relaxed text-ink-2">{detail}</p>
            <p className="mb-0 text-sm">
              <span className="text-ink-3">Palette: </span>
              <strong>{action}</strong>
            </p>
          </li>
        ))}
      </ol>
      <p className="m-0 max-w-3xl text-sm text-ink-2">
        To make review decisions, select a reflex in the terminal and choose the
        matching review action. Execution grants have a separate Delegate reflex
        action and confirmation. Rewrites are controlled in Settings. Instincts
        come from installed packs and can work before conditioning. Reflexes
        form from repeated behavior in your traces. A healthy connection can
        receive hooks without an applicable reflex or a recorded decision yet.
      </p>
      <div className="flex flex-wrap gap-3">
        <button
          type="button"
          onClick={() => setUrl({ view: "health" }, true)}
          className="rounded-md border border-line-strong px-4 py-2 text-sm"
        >
          Check connection health
        </button>
        <button
          type="button"
          onClick={() => setUrl({ view: "how" }, true)}
          className="rounded-md border border-line-strong px-4 py-2 text-sm"
        >
          Explore how Adage works
        </button>
        <button
          type="button"
          onClick={() => setUrl({ view: "traces", session: null }, true)}
          className="rounded-md border border-line-strong px-4 py-2 text-sm"
        >
          Explore traces
        </button>
      </div>
    </main>
  );
}
