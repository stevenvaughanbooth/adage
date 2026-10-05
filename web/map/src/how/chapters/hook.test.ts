import { describe, expect, it } from "vitest";
import { armDraw, plainToken, sha256 } from "./hook";

const hex = (b: Uint8Array) => [...b].map((x) => x.toString(16).padStart(2, "0")).join("");

describe("the hook chapter's arm assignment", () => {
  it("hashes like SHA-256", () => {
    expect(hex(sha256(""))).toBe("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    expect(hex(sha256("abc"))).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    // Two blocks, and a multi-byte character.
    expect(hex(sha256("a".repeat(64)))).toBe("ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb");
    expect(hex(sha256("é"))).toBe("4a99557e4033c3539de2eb65472017cad5f9557f7a0625a09f1c3f6e2ba69c4c");
  });

  it("draws x as adage_runtime::assign_arm does", () => {
    // Checked against the Rust function: assign_arm("T2", 0.0638) is treated, 0.0639 control;
    // "T11" flips between 0.0871 and 0.0872; "T1" between 0.2961 and 0.2962.
    expect(armDraw("T2")).toBeGreaterThan(0.0638);
    expect(armDraw("T2")).toBeLessThan(0.0639);
    expect(armDraw("T11")).toBeGreaterThan(0.0871);
    expect(armDraw("T11")).toBeLessThan(0.0872);
    expect(armDraw("T1")).toBeCloseTo(0.2961652968768415, 12);
  });

  it("accepts only plain tokens", () => {
    expect(plainToken("W-1042")).toBe(true);
    expect(plainToken("src/a.py")).toBe(true);
    expect(plainToken("--help")).toBe(false);
    expect(plainToken("W-1042;rm")).toBe(false);
    expect(plainToken("$(curl x)")).toBe(false);
    expect(plainToken("")).toBe(false);
    expect(plainToken("a".repeat(201))).toBe(false);
  });
});
