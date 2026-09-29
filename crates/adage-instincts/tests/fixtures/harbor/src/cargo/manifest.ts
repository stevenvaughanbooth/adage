import type { Container } from "@harbor/dispatch/types";

export function teuOf(container: Container): number {
  switch (container.lengthFt) {
    case 20:
      return 1;
    case 40:
      return 2;
    case 45:
      return 2;
    default:
      throw new RangeError(`unsupported container length: ${container.lengthFt}`);
  }
}

export function manifestTeu(containers: readonly Container[]): number {
  return containers.reduce((sum, c) => sum + teuOf(c), 0);
}

export function verifiedGrossMassTonnes(container: Container): number {
  if (container.cargoKg < 0 || container.tareKg < 0) {
    throw new RangeError(`negative mass on ${container.id}`);
  }
  return Math.round(container.cargoKg) / 1000;
}

export function manifestWeightTonnes(containers: readonly Container[]): number {
  const kg = containers.reduce((sum, c) => sum + Math.round(verifiedGrossMassTonnes(c) * 1000), 0);
  return kg / 1000;
}

export function reeferCount(containers: readonly Container[]): number {
  return containers.filter((c) => c.reefer === true).length;
}
