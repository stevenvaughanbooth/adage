import type { Route } from "../api/types";
import { exactRouteKey, measurement } from "../lib/traces";
import { useUrl } from "../state/url";
import { WithVsControl } from "./Inspector";

/** Version-specific counts link to the actual sessions and recorded exposure sequences. */
export function MeasurementLinks({ route }: { route: Route }) {
  const [url, setUrl] = useUrl();
  const m = measurement(route, url.version);
  const samples = m.measurement_sessions ?? [];
  return (
    <section
      aria-label="Contributing traces"
      className="flex flex-col gap-3 rounded-lg border border-line-strong p-3"
    >
      <label className="flex flex-col gap-1 text-xs text-ink-2">
        Measured version
        <select
          className="min-w-0 rounded-md border border-line-strong bg-panel p-2 text-ink-1"
          value={m.version}
          onChange={(e) => setUrl({ version: e.target.value }, true)}
        >
          <option value={route.version}>{route.version} · installed</option>
          {route.measurement_history?.map((h) => (
            <option key={h.version} value={h.version}>
              {h.version} · historical
            </option>
          ))}
        </select>
      </label>
      <WithVsControl r={{ ...route, arms: m.arms }} />
      <p className="m-0 text-xs leading-relaxed text-ink-3">
        {m.measurement_info?.description ??
          "Counts are available when a recorded exposure identifies this installed reflex and version."}
      </p>
      <div className="flex flex-wrap gap-2">
        {(["treated", "control"] as const).map((arm) => {
          const contributing = samples.filter(
              (s) => s.arm === arm && s.applied.length > 0,
            ),
            first = contributing[0];
          return (
            <button
              key={arm}
              type="button"
              disabled={!first}
              className="rounded-md border border-line-strong px-2.5 py-2 text-xs text-accent-soft disabled:opacity-45"
              onClick={() =>
                first &&
                setUrl(
                  {
                    view: "traces",
                    reflex: exactRouteKey(route),
                    version: m.version,
                    session: first.session,
                    traceRepo: first.repo,
                    sequence: first.applied[0] ?? null,
                    window: "all",
                  },
                  true,
                )
              }
            >
              {contributing.length} contributing{" "}
              {arm === "treated" ? "guided" : "control"} traces →
            </button>
          );
        })}
      </div>
      {m.version !== route.version && (
        <p className="m-0 text-xs text-ink-2">
          Historical counts belong to the recorded version. Open a trace for its
          exact response; the directive shown elsewhere is the installed
          version.
        </p>
      )}
    </section>
  );
}
