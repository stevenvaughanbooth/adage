import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { manifestTeu, reeferCount, teuOf, verifiedGrossMassTonnes } from "@harbor/dispatch/cargo/manifest";
import type { Container } from "@harbor/dispatch/types";

const box = (id: string, lengthFt: 20 | 40 | 45, extra: Partial<Container> = {}): Container => ({
  id,
  lengthFt,
  cargoKg: 10_000,
  tareKg: 0,
  ...extra,
});

describe("manifest", () => {
  test("twenty and forty foot boxes", () => {
    assert.equal(teuOf(box("A", 20)), 1);
    assert.equal(teuOf(box("B", 40)), 2);
    assert.equal(manifestTeu([box("A", 20), box("B", 40), box("C", 20)]), 4);
  });

  test("unknown lengths are rejected", () => {
    assert.throws(() => teuOf({ ...box("X", 20), lengthFt: 30 as unknown as 20 }), RangeError);
  });

  test("mass is reported in tonnes", () => {
    assert.equal(verifiedGrossMassTonnes(box("A", 20, { cargoKg: 12_000 })), 12);
    assert.throws(() => verifiedGrossMassTonnes(box("A", 20, { cargoKg: -1 })), RangeError);
  });

  test("reefers are counted", () => {
    assert.equal(reeferCount([box("A", 40, { reefer: true }), box("B", 40), box("C", 20, { reefer: true })]), 2);
  });
});
