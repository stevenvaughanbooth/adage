import type { Container } from "@harbor/dispatch/types";

const SEGREGATION: Record<string, readonly string[]> = {
  "1": ["2.1", "3", "4.1", "5.1", "5.2", "8"],
  "2.1": ["1", "5.1"],
  "3": ["1", "5.1", "5.2"],
  "4.1": ["1", "5.1"],
  "5.1": ["1", "2.1", "3", "4.1"],
  "5.2": ["1", "3"],
  "8": ["1"],
};

export function primaryClass(dgClass: string): string {
  return dgClass.trim().split(".")[0];
}

export function mustSegregate(a: string, b: string): boolean {
  const left = SEGREGATION[primaryClass(a)] ?? [];
  const right = SEGREGATION[primaryClass(b)] ?? [];
  return left.includes(primaryClass(b)) || right.includes(primaryClass(a));
}

export function adjacentConflicts(row: readonly Container[]): Array<[string, string]> {
  const out: Array<[string, string]> = [];
  for (let i = 1; i < row.length; i++) {
    const a = row[i - 1];
    const b = row[i];
    if (a.dgClass && b.dgClass && mustSegregate(a.dgClass, b.dgClass)) {
      out.push([a.id, b.id]);
    }
  }
  return out;
}
