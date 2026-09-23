export const KM_PER_NM = 1.852;

export function distanceKm(distanceNm: number): number {
  return distanceNm * KM_PER_NM;
}

export function transitHours(distanceNm: number, speedKn: number): number {
  if (!(speedKn > 0)) {
    throw new RangeError("speed must be positive");
  }
  if (distanceNm < 0) {
    throw new RangeError("distance must not be negative");
  }
  return distanceKm(distanceNm) / speedKn;
}

export function arrivalTime(departure: Date, distanceNm: number, speedKn: number, pilotageHours = 0): Date {
  const hours = transitHours(distanceNm, speedKn) + pilotageHours;
  return new Date(departure.getTime() + Math.round(hours * 3_600_000));
}
