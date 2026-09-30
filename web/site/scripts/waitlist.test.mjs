import assert from "node:assert/strict";
import test from "node:test";
import { GET, POST } from "../api/waitlist.ts";

test("availability returns only a boolean, is uncached, and never contacts the destination", async () => {
  const previous = process.env.WAITLIST_WEBHOOK_URL;
  const originalFetch = globalThis.fetch;
  globalThis.fetch = () => { throw new Error("Availability must not contact the signup destination"); };
  try {
    for (const value of [undefined, "", "  ", "https://signup.example.invalid/private-destination"]) {
      if (value === undefined) delete process.env.WAITLIST_WEBHOOK_URL;
      else process.env.WAITLIST_WEBHOOK_URL = value;
      const response = GET();
      assert.equal(response.status, 200);
      assert.equal(response.headers.get("cache-control"), "no-store");
      assert.deepEqual(await response.json(), { available: Boolean(value?.trim()) });
      if (!value?.trim()) {
        const result = await POST(new Request("https://site.invalid/api/waitlist", { method: "POST" }));
        assert.equal(result.status, 503);
      }
    }
  } finally {
    globalThis.fetch = originalFetch;
    if (previous === undefined) delete process.env.WAITLIST_WEBHOOK_URL;
    else process.env.WAITLIST_WEBHOOK_URL = previous;
  }
});
