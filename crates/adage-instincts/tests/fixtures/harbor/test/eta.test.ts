import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { localArrival } from "@harbor/dispatch/eta/local";
import { arrivalTime, distanceKm, transitHours } from "@harbor/dispatch/eta/transit";

describe("transit", () => {
  test("speed must be positive", () => {
    assert.throws(() => transitHours(100, 0), RangeError);
  });

  test("zero distance takes no time", () => {
    assert.equal(transitHours(0, 14), 0);
  });

  test("pilotage is added to the arrival", () => {
    const dep = new Date("2026-05-04T06:00:00Z");
    assert.equal(arrivalTime(dep, 0, 14, 2).toISOString(), "2026-05-04T08:00:00.000Z");
  });

  test("distance conversion", () => {
    assert.ok(Math.abs(distanceKm(100) - 185.2) < 1e-9);
  });
});

describe("local arrival", () => {
  test("Felixstowe in winter", () => {
    assert.equal(localArrival(new Date("2026-01-15T09:30:00Z"), "GBFXT"), "2026-01-15 09:30");
  });

  test("unknown ports are rejected", () => {
    assert.throws(() => localArrival(new Date("2026-01-15T09:30:00Z"), "XXZZZ"), RangeError);
  });
});
