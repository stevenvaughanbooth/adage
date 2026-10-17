import type { Graph } from "../api/types";
import { findRoute } from "../lib/graph";
import { isPlugin, setupCopy } from "../lib/setup";
import { useUrl } from "../state/url";

/** Where to act, worded for the surface the person uses (`adage.channel`): `/adage` in Claude Code
 * for a plugin user, the `adage` commands otherwise. Execution grants stay in that person-controlled surface. */
export function Setup({ graph }: { graph: Graph | null }) {
  const [url, setUrl] = useUrl();
  const selected = findRoute(graph, url.reflex);
  const copy = setupCopy(graph);
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
        <p className="font-mono text-sm text-ink-1" data-testid="setup-lead">
          {copy.lead}
        </p>
        <div className="rounded-lg border border-accent/40 bg-panel p-4 text-sm leading-relaxed">
          {graph?.demo ? (
            <p className="m-0">
              You are exploring sample data. In the Adage terminal app (adage ui), choose{" "}
              <strong>Open repository</strong> before connecting your real repository.
            </p>
          ) : (
            <p className="m-0">{copy.intro}</p>
          )}
        </div>
        {selected && (
          <p className="rounded-lg border border-prime/35 bg-panel p-4 text-sm">
            {copy.review(selected.id)}
          </p>
        )}
      </div>
      <ol className="m-0 grid max-w-5xl list-none gap-4 p-0 md:grid-cols-2">
        {copy.steps.map(({ title, action, detail }, i) => (
          <li
            key={title}
            className="rounded-lg border border-line-strong bg-panel p-5"
          >
            <span className="lbl">Step {i + 1}</span>
            <h2 className="display my-2 text-xl font-semibold">{title}</h2>
            <p className="text-sm leading-relaxed text-ink-2">{detail}</p>
            <p className="mb-0 text-sm">
              <span className="text-ink-3">{isPlugin(graph) ? "In Claude Code: " : "In a terminal: "}</span>
              <code className="font-mono text-ink-1">{action}</code>
            </p>
          </li>
        ))}
      </ol>
      <p className="m-0 max-w-3xl text-sm text-ink-2">
        Decide on reflexes with{" "}
        {isPlugin(graph) ? "/adage review in Claude Code" : "adage review, or in adage ui's Reflexes view"}.
        Execution grants have their own confirmation. Rewrites are controlled in
        Settings. Instincts
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
