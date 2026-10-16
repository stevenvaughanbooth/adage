import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { localArrival } from "@harbor/dispatch/eta/local";

describe("hidden INC-219", () => {
  test("Singapore is eight hours ahead", () => {
    assert.equal(localArrival(new Date("2026-03-01T12:00:00Z"), "SGSIN"), "2026-03-01 20:00");
  });

  test("Rotterdam in winter is one hour ahead", () => {
    assert.equal(localArrival(new Date("2026-03-01T12:00:00Z"), "NLRTM"), "2026-03-01 13:00");
  });

  test("New York in summer crosses midnight", () => {
    assert.equal(localArrival(new Date("2026-07-01T02:30:00Z"), "USNYC"), "2026-06-30 22:30");
  });

  test("unknown ports are still rejected", () => {
    assert.throws(() => localArrival(new Date("2026-03-01T12:00:00Z"), "XXZZZ"), RangeError);
  });
});
