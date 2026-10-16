import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { vesselClass } from "@harbor/dispatch/ports/registry";

describe("hidden INC-171", () => {
  test("Feedermax is in the registry", () => {
    const v = vesselClass("feedermax");
    assert.ok(v, "Feedermax not found");
    assert.equal(v.name, "Feedermax");
    assert.equal(v.lengthM, 200);
    assert.equal(v.beamM, 30);
    assert.equal(v.draftM, 10);
    assert.equal(v.teu, 3000);
  });

  test("existing classes are unchanged", () => {
    assert.equal(vesselClass("Panamax")?.teu, 5000);
    assert.equal(vesselClass("ULCV")?.draftM, 16);
  });
});
