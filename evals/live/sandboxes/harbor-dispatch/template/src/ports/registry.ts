import type { Port, VesselClass } from "@harbor/dispatch/types";
import { PORTS, VESSEL_CLASSES } from "@harbor/dispatch/generated/registry";
import { normaliseLocode } from "@harbor/dispatch/ports/locode";

export function portByCode(code: string): Port | undefined {
  const key = normaliseLocode(code);
  return PORTS.find((p) => p.code === key);
}

export function portsInCountry(country: string): Port[] {
  const cc = country.trim().toUpperCase();
  return PORTS.filter((p) => p.country === cc);
}

export function vesselClass(name: string): VesselClass | undefined {
  const key = name.trim().toLowerCase();
  return VESSEL_CLASSES.find((v) => v.name.toLowerCase() === key);
}

export function classesForPort(port: Port): VesselClass[] {
  return VESSEL_CLASSES.filter((v) => v.draftM <= port.maxDraftM);
}
