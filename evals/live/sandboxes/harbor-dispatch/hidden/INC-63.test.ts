import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { isValidLocode, normaliseLocode } from "@harbor/dispatch/ports/locode";
import { portByCode } from "@harbor/dispatch/ports/registry";

describe("hidden INC-63", () => {
  test("spaces and hyphens are removed", () => {
    assert.equal(normaliseLocode("nl-rtm"), "NLRTM");
    assert.equal(normaliseLocode("NL RTM"), "NLRTM");
    assert.equal(normaliseLocode(" sg sin "), "SGSIN");
  });

  test("lookups accept the written forms", () => {
    assert.equal(portByCode("NL RTM")?.name, "Rotterdam");
    assert.equal(portByCode("de-ham")?.name, "Hamburg");
    assert.equal(isValidLocode("GB FXT"), true);
  });

  test("plain codes still work", () => {
    assert.equal(normaliseLocode("nlrtm"), "NLRTM");
    assert.equal(isValidLocode("NL"), false);
  });
});
