import type { Route } from "../api/types";
import { lifecycle } from "./words";

/** Approval, service readiness and advice strength are independent. */
export function approvalLabel(r: Route): string {
  const review = r.last_review;
  if (!review) return "No saved human decision";
  if (review.decision === "approved") return review.current ? "Approved" : "Approval needs another look";
  return `${review.decision}${review.current ? "" : " · earlier version"}`;
}

export function readinessLabel(r: Route): string {
  const state = lifecycle(r.state);
  if (state === "extinguished") return "Off";
  if (r.execution_only) return "Execution candidate; separate permission required";
  if (r.quiet || r.review?.reasons.some(reason => reason.kind === "quiet" || reason.kind === "approval")) return "Held for your decision";
  if (state === "nascent") return "Not in service yet";
  if (r.in_index) return "Active advice · ready when its trigger matches";
  return "Not active in the current index";
}
