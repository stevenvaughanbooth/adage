const LOCODE = /^[A-Z]{2}[A-Z2-9]{3}$/;

export function normaliseLocode(input: string): string {
  return input.trim().toUpperCase();
}

export function isValidLocode(input: string): boolean {
  return LOCODE.test(normaliseLocode(input));
}

export function countryOf(locode: string): string {
  const code = normaliseLocode(locode);
  if (!LOCODE.test(code)) {
    throw new RangeError(`not a UN/LOCODE: ${locode}`);
  }
  return code.slice(0, 2);
}
