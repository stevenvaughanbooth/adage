import { useEffect, useState } from "react";
import { refresh } from "../api/client";
import { confirmReview, reviewAccess, reviewPreview, type ReviewAccess, type ReviewPreview, type ReviewResult } from "../api/review";
import type { Route } from "../api/types";
import { lifecycle, orderedVerbs, reviewCommand, strength, verbWhy } from "../lib/words";
import { setUrl } from "../state/url";
import { CommandButton, Modal } from "./ui";

const labels: Record<string, string> = { approve: "Approve reflex", strengthen: "Strengthen advice", weaken: "Weaken advice", extinguish: "Turn off reflex", pin: "Keep this version", dismiss: "Dismiss proposed change" };
const explanation: Record<string, string> = {
  approve: "Accept the current reflex or the proposed change shown below. A nascent or off advisory reflex can enter service; an already active reflex stays active. Approval does not increase its strength or allow automatic execution.",
  strengthen: "Change prime advice to fire: a stronger instruction to your agent, within this reflex's effect ceiling. This does not allow automatic execution.",
  weaken: "Change fire to prime: suggest the step instead of directing it. This choice is kept across learning.",
  extinguish: "Stop delivering this reflex and keep it off across learning.",
  pin: "Keep the current version. Future learning may propose changes but will not replace it without review.",
  dismiss: "Keep the current version and dismiss this proposed change.",
};
const message = (e: unknown) => e instanceof Error ? e.message : String(e);

export function ReviewActions({ route }: { route: Route }) {
  const [access, setAccess] = useState<ReviewAccess | null>(null);
  const [accessError, setAccessError] = useState("");
  const [target, setTarget] = useState<Route | null>(null);
  const [verb, setVerb] = useState<string | null>(null);
  const [preview, setPreview] = useState<ReviewPreview | null>(null);
  const [result, setResult] = useState<ReviewResult | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let live = true;
    void reviewAccess().then(v => { if (live) setAccess(v); }).catch(e => { if (live) setAccessError(message(e)); });
    return () => { live = false; };
  }, []);
  async function inspect(next: string, subject = route) {
    setTarget(subject); setVerb(next); setPreview(null); setResult(null); setError(""); setBusy(true);
    try { setPreview(await reviewPreview(subject.reference, subject.id)); }
    catch (e) { setError(message(e)); }
    finally { setBusy(false); }
  }
  async function confirm() {
    if (!target || !preview || !verb || !access?.token || busy) return;
    setBusy(true); setError("");
    try {
      const saved = await confirmReview(access.token, target.reference, target.id, verb, preview.revision);
      setResult(saved);
      setPreview(null);
      // Installed directory identity survives a manifest version bump. Canonicalize legacy links.
      setUrl({ reflex: JSON.stringify([saved.repo, saved.reference, saved.reflex]), version: null });
      // refresh reports its own read error; it never turns a saved decision into a failed write.
      await refresh();
    } catch (e) { setError(message(e)); setPreview(null); }
    finally { setBusy(false); }
  }
  const verbs = orderedVerbs(route);
  return <section aria-label="Review this reflex" className="flex flex-col gap-2 rounded-md border border-line-strong p-3">
    <h3 className="m-0 text-sm font-medium">Review this reflex</h3>
    <p className="m-0 text-xs text-ink-2">Approval accepts the reflex. Strength controls how firmly Adage advises. Execution needs a separate local grant.</p>
    {access?.enabled ? <div className="flex flex-wrap gap-2">{verbs.map(v => <button key={v} type="button" className="rounded-md border border-accent/40 px-3 py-2 text-sm text-accent-soft hover:bg-raised" onClick={() => void inspect(v)}>{labels[v] ?? v}</button>)}</div>
      : <>
        <p className="m-0 text-xs text-ink-2" role="status">{access?.reason ?? (accessError ? "Review is unavailable in this map. Use the terminal command below, or reopen adage map from your own terminal." : "Checking whether review is available…")}</p>
        <details><summary className="cursor-pointer text-xs text-ink-2">Review at your terminal</summary><div className="mt-2 flex flex-col gap-2">{verbs.map(v => <CommandButton key={v} command={reviewCommand(v, route)} why={`${labels[v] ?? v}: ${verbWhy(v)}`} />)}</div></details>
      </>}
    <Modal label={`${labels[verb ?? ""] ?? "Review"} · ${target?.id ?? route.id}`} open={verb !== null} onClose={() => { if (!busy) setVerb(null); }} focusInput={false}>
      <div className="scroll-thin flex max-h-[80vh] flex-col gap-4 overflow-auto p-5">
        <h2 className="m-0 text-xl font-semibold">{labels[verb ?? ""]} · <span className="break-all font-mono">{target?.id ?? route.id}</span></h2>
        <p className="m-0 text-sm text-ink-2">{target?.execution_only && verb === "approve" ? "Accept this execution candidate's evidence only. It remains silent without its separate exact execution grant." : explanation[verb ?? ""]}</p>
        {busy && <p role="status">{result ? "Refreshing the saved status…" : preview ? "Saving your decision…" : "Loading the exact installed version…"}</p>}
        {preview && <>
          {preview.reasons?.map(([kind, why]) => <p key={kind} className="m-0 text-sm text-ink-2">{why}</p>)}
          <h3 className="m-0 text-sm">Current response</h3><pre className="scroll-thin max-h-52 overflow-auto whitespace-pre-wrap rounded border border-line p-3 text-xs">{preview.current_directive}</pre>
          {preview.proposal && <><h3 className="m-0 text-sm">Proposed change {verb === "approve" ? "to accept" : "(not applied by this decision)"}</h3><p className="m-0 text-sm">{preview.proposal.reason}</p><dl className="m-0 text-xs">{preview.proposal.diff.map(([field, before, after]) => <div key={field}><dt className="font-medium">{field}</dt><dd className="ml-3 break-words">{before} → {after}</dd></div>)}</dl><pre className="scroll-thin max-h-52 overflow-auto whitespace-pre-wrap rounded border border-line p-3 text-xs">{preview.proposal.directive}</pre></>}
          <p className="m-0 text-xs text-ink-3">Your decision applies only to this inspected revision. If learning changes it, reload and review again.</p>
        </>}
        {error && <p role="alert" className="m-0 text-sm text-inhibit">{error}</p>}
        {result && <div role="status" className="flex flex-col gap-2 rounded border border-accent/40 p-3"><strong>{result.already ? "Already in the requested state; no further change needed." : "Decision saved."}</strong><span>Lifecycle: {lifecycle(result.state)}. Advice strength: {strength(result.strength)}.</span>{result.proposal_applied && <span>The inspected proposal is now installed.</span>}{result.pack_approved.length > 0 && <span>Also approved: {result.pack_approved.join(", ")}</span>}{target?.instinct && strength(result.strength) === "prime" && <span>This instinct remains prime after approval. Prime describes suggestion strength, not a pending approval.</span>}{result.warnings?.map(warning => <span key={warning}>{warning}</span>)}<span className="text-xs">Automatic execution permission was not changed.</span></div>}
        <div className="flex flex-wrap gap-2">
          <button type="button" disabled={busy} className="rounded border border-line-strong px-4 py-2 disabled:opacity-50" onClick={() => setVerb(null)}>{result ? "Done" : "Cancel"}</button>
          {preview && <button type="button" disabled={busy || !preview.verbs.includes(verb ?? "")} className="rounded border border-accent/50 bg-[var(--sel)] px-4 py-2 disabled:opacity-50" onClick={() => void confirm()}>Confirm decision</button>}
          {error && <button type="button" disabled={busy} className="rounded border border-line-strong px-4 py-2" onClick={() => void inspect(verb!, target ?? route)}>Reload review</button>}
        </div>
      </div>
    </Modal>
  </section>;
}
