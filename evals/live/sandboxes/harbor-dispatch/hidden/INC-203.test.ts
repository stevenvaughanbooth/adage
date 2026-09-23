import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { manifestWeightTonnes, verifiedGrossMassTonnes } from "@harbor/dispatch/cargo/manifest";
import type { Container } from "@harbor/dispatch/types";

const box = (id: string, cargoKg: number, tareKg: number): Container => ({ id, lengthFt: 40, cargoKg, tareKg });

describe("hidden INC-203", () => {
  test("VGM is cargo plus tare", () => {
    assert.equal(verifiedGrossMassTonnes(box("A", 20_000, 2_200)), 22.2);
    assert.equal(verifiedGrossMassTonnes(box("B", 0, 3_800)), 3.8);
  });

  test("manifest weight sums VGMs", () => {
    assert.equal(manifestWeightTonnes([box("A", 20_000, 2_200), box("B", 10_000, 3_800)]), 36);
  });

  test("negative masses are still rejected", () => {
    assert.throws(() => verifiedGrossMassTonnes(box("C", 1_000, -1)), RangeError);
  });
});
