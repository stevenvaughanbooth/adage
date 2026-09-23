import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { portByCode, portsInCountry } from "@harbor/dispatch/ports/registry";

describe("hidden INC-164", () => {
  test("Gdansk is in the registry", () => {
    const p = portByCode("PLGDN");
    assert.ok(p, "PLGDN not found");
    assert.equal(p.name, "Gdansk");
    assert.equal(p.country, "PL");
    assert.equal(p.timezone, "Europe/Warsaw");
    assert.equal(p.maxDraftM, 15);
    assert.equal(p.pilotageHours, 2);
  });

  test("both Polish ports are listed", () => {
    assert.deepEqual(portsInCountry("PL").map((p) => p.code).sort(), ["PLGDN", "PLGDY"]);
  });
});
