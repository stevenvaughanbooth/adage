import type { Port, VesselClass } from "../types.ts";

export const PORTS: readonly Port[] = [
  { code: "BEANR", name: "Antwerp", country: "BE", timezone: "Europe/Brussels", maxDraftM: 16, pilotageHours: 3 },
  { code: "NLRTM", name: "Rotterdam", country: "NL", timezone: "Europe/Amsterdam", maxDraftM: 22.6, pilotageHours: 2 },
];

export const VESSEL_CLASSES: readonly VesselClass[] = [
  { name: "Feeder", lengthM: 140, beamM: 22, draftM: 8, teu: 1000 },
  { name: "Panamax", lengthM: 294, beamM: 32, draftM: 12, teu: 5000 },
];
