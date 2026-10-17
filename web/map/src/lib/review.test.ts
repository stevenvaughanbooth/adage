import { describe, expect, it } from "vitest";
import { normalizeRoute } from "../api/normalize";
import { approvalLabel, readinessLabel } from "./review";

const route = (extra = {}) => normalizeRoute({ state: "forming", strength: "prime", in_index: true, ...extra });
describe("review and readiness", () => {
  it("keeps active prime advice distinct from saved human approval", () => {
    const r = route({ instinct: true, last_review: { decision: "approved", on: "2026-10-17", current: true } });
    expect(approvalLabel(r)).toBe("Approved");
    expect(readinessLabel(r)).toMatch(/^Active advice/);
    expect(r.strength).toBe("prime");
    expect(approvalLabel(route())).toBe("No saved human decision");
  });
  it("does not mistake advisory review notes for delivery blockers", () => {
    expect(readinessLabel(route({ review: { reasons: [{ kind: "changed", why: "Review new evidence" }] } }))).toMatch(/^Active advice/);
    expect(readinessLabel(route({ review: { reasons: [{ kind: "approval", why: "Needs permission" }] } }))).toBe("Held for your decision");
  });
  it("does not call approved execution candidates or unindexed reflexes active advice", () => {
    expect(readinessLabel(route({ execution_only: true }))).toContain("separate permission required");
    expect(readinessLabel(route({ in_index: false }))).toBe("Not active in the current index");
    expect(readinessLabel(route({ state: "extinguished" }))).toBe("Off");
    expect(approvalLabel(route({ last_review: { decision: "approved", current: false } }))).toBe("Approval needs another look");
  });
});
