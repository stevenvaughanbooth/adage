import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { berthsFor, fitsBerth } from "@harbor/dispatch/berths/fit";
import { BerthSchedule } from "@harbor/dispatch/berths/schedule";
import type { Berth, VesselClass } from "@harbor/dispatch/types";

const vessel: VesselClass = { name: "Test", lengthM: 300, beamM: 40, draftM: 12, teu: 8000 };
const berth = (id: string, lengthM: number, depthM: number): Berth => ({ id, port: "NLRTM", lengthM, depthM });
const at = (hhmm: string) => new Date(`2026-05-04T${hhmm}:00Z`);

describe("berth fit", () => {
  test("a vessel longer than the berth does not fit", () => {
    assert.equal(fitsBerth(vessel, berth("B1", 280, 16)), false);
  });

  test("a deep, long berth fits", () => {
    assert.equal(fitsBerth(vessel, berth("B2", 350, 16)), true);
  });

  test("candidate berths, shortest first", () => {
    const got = berthsFor(vessel, [berth("B3", 400, 17), berth("B1", 280, 16), berth("B2", 350, 16)]);
    assert.deepEqual(got.map((b) => b.id), ["B2", "B3"]);
  });
});

describe("berth schedule", () => {
  test("overlapping bookings on one berth are refused", () => {
    const s = new BerthSchedule();
    s.book({ berth: "B1", vessel: "MSC Ada", start: at("08:00"), end: at("12:00") });
    assert.throws(() => s.book({ berth: "B1", vessel: "Maersk Iva", start: at("10:00"), end: at("14:00") }));
  });

  test("other berths are independent", () => {
    const s = new BerthSchedule();
    s.book({ berth: "B1", vessel: "MSC Ada", start: at("08:00"), end: at("12:00") });
    s.book({ berth: "B2", vessel: "Maersk Iva", start: at("10:00"), end: at("14:00") });
    assert.equal(s.bookingsFor("B2").length, 1);
  });

  test("a booking must end after it starts", () => {
    const s = new BerthSchedule();
    assert.throws(() => s.book({ berth: "B1", vessel: "MSC Ada", start: at("12:00"), end: at("08:00") }), RangeError);
  });
});
