import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

beforeEach(() => { vi.resetModules(); vi.useFakeTimers(); });
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.unstubAllGlobals(); });

const response = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status });

describe("local server recovery", () => {
  it("retries a failed initial snapshot without needing a stream event", async () => {
    const fetch = vi.fn().mockResolvedValueOnce(response({ error: "busy" }, 503)).mockResolvedValue(response({}));
    vi.stubGlobal("fetch", fetch);
    const { refresh } = await import("./client");
    await refresh();
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetch).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("backs off persistent failures and deduplicates a manual retry", async () => {
    const fetch = vi.fn().mockResolvedValue(response({}, 503));
    vi.stubGlobal("fetch", fetch);
    const { refresh } = await import("./client");
    await refresh();
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetch).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1999);
    expect(fetch).toHaveBeenCalledTimes(2);
    await Promise.all([refresh(), refresh()]);
    expect(fetch).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(1);
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("does not cache a failed health check", async () => {
    const fetch = vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue(response({ checks: [] }));
    vi.stubGlobal("fetch", fetch);
    const { loadHealth } = await import("./client");
    await expect(loadHealth()).rejects.toThrow("offline");
    await expect(loadHealth()).resolves.toMatchObject({ checks: [] });
    expect(fetch).toHaveBeenCalledTimes(2);
  });

  it("aborts a stalled request and allows the next attempt", async () => {
    vi.stubGlobal("fetch", vi.fn((_url, init: RequestInit) => new Promise((_resolve, reject) => {
      init.signal?.addEventListener("abort", () => reject(new Error("aborted")));
    })));
    const { fetchJson } = await import("./client");
    const result = expect(fetchJson("/api/v1/graph")).rejects.toThrow("took too long");
    await vi.advanceTimersByTimeAsync(10_000);
    await result;
  });

  it("refreshes during a continuous decision stream", async () => {
    const handlers = new Map<string, (event: MessageEvent) => void>();
    vi.stubGlobal("EventSource", class {
      readyState = 1;
      addEventListener(name: string, handler: (event: MessageEvent) => void) { handlers.set(name, handler); }
    });
    const fetch = vi.fn().mockResolvedValue(response({}));
    vi.stubGlobal("fetch", fetch);
    const { start, refresh } = await import("./client");
    start();
    await refresh();
    for (let i = 0; i < 6; i++) {
      handlers.get("decision")!({ data: "{}" } as MessageEvent);
      await vi.advanceTimersByTimeAsync(500);
    }
    expect(fetch.mock.calls.length).toBeGreaterThanOrEqual(3);
  });
});
