import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { manifestTeu, teuOf } from "@harbor/dispatch/cargo/manifest";
import type { Container } from "@harbor/dispatch/types";

const box = (id: string, lengthFt: 20 | 40 | 45): Container => ({ id, lengthFt, cargoKg: 5000, tareKg: 2000 });

describe("hidden INC-58", () => {
  test("a 45-foot box is two and a quarter TEU", () => {
    assert.equal(teuOf(box("H", 45)), 2.25);
  });

  test("20 and 40 are unchanged", () => {
    assert.equal(teuOf(box("A", 20)), 1);
    assert.equal(teuOf(box("B", 40)), 2);
  });

  test("mixed manifest total", () => {
    assert.equal(manifestTeu([box("A", 20), box("B", 40), box("H", 45)]), 5.25);
  });
});
