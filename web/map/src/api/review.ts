import { fetchJson } from "./client";

export interface ReviewAccess { enabled: boolean; token: string | null; reason: string | null }
export interface ReviewPreview {
  revision: string;
  current_directive: string;
  verbs: string[];
  reasons: [string, string][];
  proposal: null | { on: string; reason: string; diff: [string, string, string][]; directive: string };
}
export interface ReviewResult {
  saved: boolean; verb: string; reflex: string; reference: string; repo: string; warnings: string[]; state: string; strength: string;
  recorded: string; proposal_applied: boolean; already: boolean; pack_approved: string[];
}
export const reviewAccess = () => fetchJson<ReviewAccess>("/api/v1/review");
export const reviewPreview = (reference: string, reflex: string) =>
  fetchJson<ReviewPreview>(`/api/v1/review?${new URLSearchParams({ reference, reflex })}`);

/** A timeout is ambiguous: the decision may have saved. Never automatically retry a write. */
export async function confirmReview(token: string, reference: string, reflex: string, verb: string, revision: string): Promise<ReviewResult> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 10_000);
  try {
    const response = await fetch("/api/v1/review", { method: "POST", cache: "no-store",
      headers: { "Content-Type": "application/json", "X-Adage-CSRF": token },
      body: JSON.stringify({ reference, reflex, verb, revision }), signal: controller.signal });
    if (!response.ok) throw new Error((await response.text()).trim());
    return await response.json() as ReviewResult;
  } catch (error) {
    if (controller.signal.aborted) throw new Error("The server did not answer in time. The decision may have saved; reload and inspect this reflex before trying again.");
    throw error;
  } finally { clearTimeout(timeout); }
}
