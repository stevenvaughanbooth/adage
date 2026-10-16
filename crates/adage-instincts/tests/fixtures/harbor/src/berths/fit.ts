import type { Berth, VesselClass } from "@harbor/dispatch/types";

export const UNDER_KEEL_CLEARANCE_M = 0.5;

export function fitsBerth(vessel: VesselClass, berth: Berth): boolean {
  return vessel.lengthM <= berth.lengthM && vessel.draftM <= berth.depthM;
}

export function berthsFor(vessel: VesselClass, berths: readonly Berth[]): Berth[] {
  return berths.filter((b) => fitsBerth(vessel, b)).sort((a, b) => a.lengthM - b.lengthM || a.id.localeCompare(b.id));
}
