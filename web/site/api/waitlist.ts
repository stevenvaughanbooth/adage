// POST /api/waitlist: a Vercel Function. It checks the address and forwards it to WAITLIST_WEBHOOK_URL
// (any endpoint that accepts a JSON POST: a Slack or Discord incoming webhook, Zapier, Make, a form
// service). No address is stored here. Without the variable, sign-ups answer 503 and the page says
// they aren't open yet.

// Deliberately plain addresses only: nothing that chat markup (Slack's <!channel>, <url|text>, Markdown
// links) could read as a mention or a link.
const EMAIL = /^[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,63}$/;

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json", "cache-control": "no-store" } });

export async function POST(request: Request): Promise<Response> {
  const hook = process.env.WAITLIST_WEBHOOK_URL;
  if (!hook) return json(503, { error: "sign-ups are not open yet" });

  let body: { email?: unknown; hp_x7?: unknown } | null;
  try {
    if (Number(request.headers.get("content-length") ?? 0) > 2048) return json(413, { error: "too large" });
    body = await request.json();
  } catch {
    return json(400, { error: "expected JSON" });
  }
  if (!body || typeof body !== "object") return json(400, { error: "expected an object" });
  // The hidden field only a bot fills in: accept quietly, forward nothing.
  if (typeof body.hp_x7 === "string" && body.hp_x7 !== "") return json(200, { ok: true });
  const email = typeof body.email === "string" ? body.email.trim().toLowerCase() : "";
  if (!EMAIL.test(email) || email.length > 320) return json(400, { error: "invalid email" });

  const payload = {
    email,
    source: "adage-site",
    at: new Date().toISOString(),
    // Slack and Discord webhooks show `text` / `content`; other endpoints read the fields above.
    // The address is already plain (EMAIL above); it is shown as code and may never ping anyone.
    text: `Adage early access: \`${email}\``,
    content: `Adage early access: \`${email}\``,
    allowed_mentions: { parse: [] },
  };
  try {
    const r = await fetch(hook, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(payload), signal: AbortSignal.timeout(5000) });
    if (!r.ok) return json(502, { error: "could not record the sign-up" });
  } catch {
    return json(502, { error: "could not record the sign-up" });
  }
  return json(200, { ok: true });
}
