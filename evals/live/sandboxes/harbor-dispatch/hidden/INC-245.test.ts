import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { portByCode } from "@harbor/dispatch/ports/registry";

describe("hidden INC-245", () => {
  test("Rotterdam takes 24 m of draft", () => {
    assert.equal(portByCode("NLRTM")?.maxDraftM, 24);
  });

  test("the rest of Rotterdam is unchanged", () => {
    const p = portByCode("NLRTM");
    assert.ok(p);
    assert.equal(p.name, "Rotterdam");
    assert.equal(p.timezone, "Europe/Amsterdam");
    assert.equal(p.pilotageHours, 2);
  });

  test("other ports are unchanged", () => {
    assert.equal(portByCode("BEANR")?.maxDraftM, 16);
    assert.equal(portByCode("DEHAM")?.maxDraftM, 15.1);
  });
});
