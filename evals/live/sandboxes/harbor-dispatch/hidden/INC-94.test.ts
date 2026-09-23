import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { BerthSchedule } from "@harbor/dispatch/berths/schedule";

const at = (hhmm: string) => new Date(`2026-05-04T${hhmm}:00Z`);

describe("hidden INC-94", () => {
  test("a booking may start when the previous one ends", () => {
    const s = new BerthSchedule();
    s.book({ berth: "B1", vessel: "MSC Ada", start: at("08:00"), end: at("12:00") });
    s.book({ berth: "B1", vessel: "Maersk Iva", start: at("12:00"), end: at("16:00") });
    assert.equal(s.bookingsFor("B1").length, 2);
  });

  test("a booking may end when the next one starts", () => {
    const s = new BerthSchedule();
    s.book({ berth: "B1", vessel: "Maersk Iva", start: at("12:00"), end: at("16:00") });
    s.book({ berth: "B1", vessel: "MSC Ada", start: at("08:00"), end: at("12:00") });
    assert.deepEqual(s.bookingsFor("B1").map((b) => b.vessel), ["MSC Ada", "Maersk Iva"]);
  });

  test("real overlaps are still refused", () => {
    const s = new BerthSchedule();
    s.book({ berth: "B1", vessel: "MSC Ada", start: at("08:00"), end: at("12:00") });
    assert.throws(() => s.book({ berth: "B1", vessel: "Maersk Iva", start: at("11:59"), end: at("16:00") }));
    assert.throws(() => s.book({ berth: "B1", vessel: "CMA Leo", start: at("09:00"), end: at("10:00") }));
  });
});
