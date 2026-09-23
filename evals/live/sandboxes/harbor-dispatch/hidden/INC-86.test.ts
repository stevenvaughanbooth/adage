import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { adjacentConflicts, mustSegregate } from "@harbor/dispatch/cargo/hazmat";

describe("hidden INC-86", () => {
  test("explosives and oxidisers, both ways", () => {
    assert.equal(mustSegregate("1", "5.1"), true);
    assert.equal(mustSegregate("5.1", "1"), true);
  });

  test("divisions on both sides", () => {
    assert.equal(mustSegregate("2.1", "5.1"), true);
    assert.equal(mustSegregate("5.2", "3"), true);
  });

  test("compatible pairs stay compatible", () => {
    assert.equal(mustSegregate("3", "8"), false);
    assert.equal(mustSegregate("8", "5.1"), false);
    assert.equal(mustSegregate("1", "3"), true);
  });

  test("a row with an oxidiser next to explosives is reported", () => {
    const row = [
      { id: "C1", lengthFt: 20 as const, cargoKg: 1, tareKg: 1, dgClass: "5.1" },
      { id: "C2", lengthFt: 20 as const, cargoKg: 1, tareKg: 1, dgClass: "1" },
    ];
    assert.deepEqual(adjacentConflicts(row), [["C1", "C2"]]);
  });
});
