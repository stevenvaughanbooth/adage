import { afterEach, describe, expect, it, vi } from "vitest";
import { confirmReview, reviewPreview } from "./review";

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });
describe("review requests", () => {
  it("confirms only the inspected identity and revision with the capability token", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response('{"saved":true}', { status: 200 }));
    vi.stubGlobal("fetch", fetch);
    await confirmReview("token", "acme/checks", "verify", "approve", "abc");
    expect(fetch).toHaveBeenCalledTimes(1);
    const [url, request] = fetch.mock.calls[0]!;
    expect(url).toBe("/api/v1/review");
    expect(request.headers).toEqual({ "Content-Type": "application/json", "X-Adage-CSRF": "token" });
    expect(JSON.parse(request.body)).toEqual({ reference: "acme/checks", reflex: "verify", verb: "approve", revision: "abc" });
  });
  it("surfaces stale decisions without retrying a write", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response("Changed after opening. Nothing was written.", { status: 409 }));
    vi.stubGlobal("fetch", fetch);
    await expect(confirmReview("token", "acme/checks", "verify", "approve", "old")).rejects.toThrow("Nothing was written");
    expect(fetch).toHaveBeenCalledTimes(1);
  });
  it("encodes installed references for read-only previews", async () => {
    const fetch = vi.fn().mockResolvedValue(new Response("{}"));
    vi.stubGlobal("fetch", fetch);
    await reviewPreview("@imported/acme/checks@1", "a & b");
    expect(fetch.mock.calls[0]![0]).toBe("/api/v1/review?reference=%40imported%2Facme%2Fchecks%401&reflex=a+%26+b");
  });
  it("warns that a timed out save may have applied, without retrying", async () => {
    vi.useFakeTimers();
    const fetch = vi.fn().mockImplementation((_url, request) => new Promise((_resolve, reject) => request.signal.addEventListener("abort", () => reject(new Error("aborted")))));
    vi.stubGlobal("fetch", fetch);
    const result = expect(confirmReview("token", "acme/checks", "verify", "approve", "old")).rejects.toThrow("may have saved");
    await vi.advanceTimersByTimeAsync(10_000);
    await result;
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});
