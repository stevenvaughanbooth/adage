import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { fitsBerth } from "@harbor/dispatch/berths/fit";
import type { Berth, VesselClass } from "@harbor/dispatch/types";

const vessel: VesselClass = { name: "Test", lengthM: 300, beamM: 40, draftM: 14, teu: 9000 };
const berth = (depthM: number, lengthM = 350): Berth => ({ id: "B", port: "NLRTM", lengthM, depthM });

describe("hidden INC-107", () => {
  test("a berth exactly as deep as the draft does not fit", () => {
    assert.equal(fitsBerth(vessel, berth(14)), false);
  });

  test("less than half a metre of clearance does not fit", () => {
    assert.equal(fitsBerth(vessel, berth(14.4)), false);
  });

  test("half a metre or more of clearance fits", () => {
    assert.equal(fitsBerth(vessel, berth(14.5)), true);
    assert.equal(fitsBerth(vessel, berth(15)), true);
  });

  test("length is still checked", () => {
    assert.equal(fitsBerth(vessel, berth(20, 250)), false);
  });
});
