import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { arrivalTime, distanceKm, transitHours } from "@harbor/dispatch/eta/transit";

describe("hidden INC-112", () => {
  test("hours are nautical miles over knots", () => {
    assert.equal(transitHours(240, 12), 20);
    assert.equal(transitHours(90, 18), 5);
  });

  test("arrival uses the corrected transit time", () => {
    const dep = new Date("2026-05-04T00:00:00Z");
    assert.equal(arrivalTime(dep, 240, 12, 2).toISOString(), "2026-05-04T22:00:00.000Z");
  });

  test("kilometre conversion is kept", () => {
    assert.ok(Math.abs(distanceKm(10) - 18.52) < 1e-9);
  });
});
