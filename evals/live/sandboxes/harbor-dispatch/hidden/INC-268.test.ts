import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { classesForPort, portByCode, vesselClass } from "@harbor/dispatch/ports/registry";

describe("hidden INC-268", () => {
  test("Neopanamax is in the registry", () => {
    const v = vesselClass("Neopanamax");
    assert.ok(v, "Neopanamax not found");
    assert.equal(v.lengthM, 366);
    assert.equal(v.beamM, 51);
    assert.equal(v.draftM, 15);
    assert.equal(v.teu, 14000);
  });

  test("port suitability", () => {
    const sg = portByCode("SGSIN");
    const pl = portByCode("PLGDY");
    assert.ok(sg && pl);
    assert.equal(classesForPort(sg).some((v) => v.name === "Neopanamax"), true);
    assert.equal(classesForPort(pl).some((v) => v.name === "Neopanamax"), false);
  });
});
