import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { adjacentConflicts, mustSegregate, primaryClass } from "@harbor/dispatch/cargo/hazmat";

describe("dangerous goods", () => {
  test("primary class", () => {
    assert.equal(primaryClass("4.1"), "4");
    assert.equal(primaryClass(" 3 "), "3");
  });

  test("explosives and flammable liquids are segregated", () => {
    assert.equal(mustSegregate("1", "3"), true);
    assert.equal(mustSegregate("3", "1"), true);
  });

  test("compatible classes may be stowed together", () => {
    assert.equal(mustSegregate("3", "8"), false);
    assert.equal(mustSegregate("9", "9"), false);
  });

  test("a row reports its adjacent conflicts", () => {
    const row = [
      { id: "C1", lengthFt: 20 as const, cargoKg: 1, tareKg: 1, dgClass: "3" },
      { id: "C2", lengthFt: 20 as const, cargoKg: 1, tareKg: 1, dgClass: "1" },
      { id: "C3", lengthFt: 20 as const, cargoKg: 1, tareKg: 1 },
    ];
    assert.deepEqual(adjacentConflicts(row), [["C1", "C2"]]);
  });
});
