import assert from "node:assert/strict";
import { describe, test } from "node:test";
import { countryOf, isValidLocode, normaliseLocode } from "@harbor/dispatch/ports/locode";
import { classesForPort, portByCode, portsInCountry, vesselClass } from "@harbor/dispatch/ports/registry";

describe("locode", () => {
  test("case is normalised", () => {
    assert.equal(normaliseLocode(" nlrtm "), "NLRTM");
    assert.equal(isValidLocode("sgsin"), true);
  });

  test("malformed codes are invalid", () => {
    assert.equal(isValidLocode("NL"), false);
    assert.throws(() => countryOf("12345"), RangeError);
  });

  test("country", () => {
    assert.equal(countryOf("DEHAM"), "DE");
  });
});

describe("registry", () => {
  test("ports by code", () => {
    assert.equal(portByCode("nlrtm")?.name, "Rotterdam");
    assert.equal(portByCode("XXZZZ"), undefined);
  });

  test("ports by country", () => {
    assert.deepEqual(portsInCountry("us").map((p) => p.code), ["USLAX", "USNYC"]);
  });

  test("vessel classes", () => {
    assert.equal(vesselClass("panamax")?.teu, 5000);
    const hamburg = portByCode("DEHAM");
    assert.ok(hamburg);
    assert.equal(classesForPort(hamburg).some((v) => v.name === "ULCV"), false);
  });
});
