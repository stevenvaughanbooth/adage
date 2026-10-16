import { type Page, expect, test } from "@playwright/test";
import AxeBuilder from "@axe-core/playwright";
import { DEMO_PORT } from "./ports";
const BASE = `http://127.0.0.1:${DEMO_PORT}`;

async function fixture(page: Page, failFirst = false) {
  const g = await (await page.request.get(`${BASE}/api/v1/graph`)).json();
  const r = g.routes[0];
  const repo = "fixture/repo+one",
    session = "older-session",
    key = JSON.stringify([r.repo, r.reference, r.id]);
  const events = Array.from({ length: 620 }, (_, i) => ({
    sequence: i + 1,
    ts: 1_800_000_000_000 + i * 1000,
    session,
    repo,
    arm: "treated",
    event: i === 0 ? "UserPromptSubmit" : "PostToolBatch",
    directed: i === 0 ? [r.id] : [],
    would: [],
    judged: i === 1 ? [["followed", r.id]] : [],
    observed: i === 1 ? ["cargo test --locked"] : [],
    continue: false,
    directives:
      i === 0
        ? [
            {
              route: r.id,
              track: r.track,
              version: "recorded-v1",
              text: "The original recorded response: cargo test --locked",
              route_key: key,
              attribution: "historical",
            },
          ]
        : [],
    would_have: [],
    judgements:
      i === 1
        ? [
            {
              kind: "followed",
              route: r.id,
              track: r.track,
              version: "recorded-v1",
              route_key: key,
              attribution: "historical",
              exposure_sequence: 1,
            },
          ]
        : [],
  }));
  g.trace_sessions = [
    {
      session,
      repo,
      arm: "treated",
      events_total: 620,
      events_visible: 0,
      start_ts: events[0]!.ts,
      end_ts: events[619]!.ts,
    },
  ];
  g.trace_events = [];
  g.events = [];
  g.sessions = { total: 1, treated: 1, control: 0 };
  r.measurement_history = [
    {
      version: "recorded-v1",
      arms: {
        treated: {
          sessions: 1,
          applied: 1,
          followed: 1,
          met: 0,
          hit_avoided: null,
        },
      },
      live: r.live,
      measurement_info: {
        status: "historical",
        description: "Recorded earlier version",
      },
      measurement_sessions: [
        {
          session,
          repo,
          arm: "treated",
          applied: [1],
          followed: [2],
          met: [],
          hit_avoided: [],
        },
      ],
    },
  ];
  let calls = 0;
  await page.route("**/api/v1/events", (r) => r.abort());
  await page.route("**/api/v1/graph", (r) => r.fulfill({ json: g }));
  await page.route("**/api/v1/trace?*", (r) => {
    const p = new URL(r.request().url()).searchParams;
    expect(p.get("session")).toBe(session);
    expect(p.get("repo")).toBe(repo);
    if (failFirst && calls++ === 0)
      return r.fulfill({ status: 503, body: "Temporarily unavailable" });
    return r.fulfill({
      json: {
        session,
        repo,
        complete: true,
        events_total: events.length,
        events,
      },
    });
  });
  return {
    g,
    r,
    repo,
    session,
    query: new URLSearchParams({
      view: "traces",
      session,
      traceRepo: repo,
      seq: "1",
    }),
  };
}

test("connect is discoverable in the palette and explains the terminal handoff", async ({
  page,
}) => {
  await page.goto(BASE);
  await page
    .getByRole("button", { name: "Search or jump to (Command K)" })
    .click();
  await page.getByRole("combobox").fill("connect");
  await page.getByRole("option", { name: /Set up my repository/ }).click();
  const setup = page.getByRole("main", { name: "Setup" });
  await expect(setup).toBeVisible();
  // The handoff names the one command that connects every agent found.
  await expect(setup.getByText("adage init", { exact: true })).toBeVisible();
  await expect(setup.getByText("Connect your agents", { exact: true })).toBeVisible();
});

test("an old complete trace reveals recorded responses, linked outcomes, and browser history beyond 400 events", async ({
  page,
}) => {
  const f = await fixture(page);
  await page.goto(`${BASE}/?${f.query}`);
  await expect(
    page.getByText("620 of 620 recorded events loaded"),
  ).toBeVisible();
  await expect(
    page.getByText("The original recorded response: cargo test --locked"),
  ).toBeVisible();
  await page.getByRole("button", { name: "Read event 2", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Event 2 · PostToolBatch" }),
  ).toBeVisible();
  await page.goBack();
  await expect(
    page.getByRole("heading", { name: "Event 1 · UserPromptSubmit" }),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Event 511: PostToolBatch", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Event 511 · PostToolBatch" }),
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Selected event details" }),
  ).toBeFocused();
  await page
    .getByRole("searchbox", { name: "Find a session" })
    .fill("no-match");
  await expect(
    page.getByRole("heading", { name: "Event 511 · PostToolBatch" }),
  ).toBeVisible();
  expect(
    await page.getByRole("combobox", { name: "Recorded session" }).inputValue(),
  ).toContain(f.session);
});

test("historical evidence opens its contributing recorded response", async ({
  page,
}) => {
  const f = await fixture(page);
  await page.goto(
    `${BASE}/?${new URLSearchParams({ reflex: JSON.stringify([f.r.repo, f.r.reference, f.r.id]) })}`,
  );
  await page
    .getByRole("combobox", { name: "Measured version" })
    .selectOption("recorded-v1");
  await page
    .getByRole("button", { name: "1 contributing guided traces →" })
    .click();
  await expect(
    page.getByText("The original recorded response: cargo test --locked"),
  ).toBeVisible();
  await expect(page).toHaveURL(/version=recorded-v1/);
});

test("failed complete traces can be retried without changing the requested event", async ({
  page,
}) => {
  const f = await fixture(page, true);
  f.query.set("seq", "511");
  await page.goto(`${BASE}/?${f.query}`);
  await page.getByRole("button", { name: "Retry complete trace" }).click();
  await expect(
    page.getByRole("heading", { name: "Event 511 · PostToolBatch" }),
  ).toBeVisible();
});

test("complete trace graph and inspector fit a phone and expose accessible controls", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const f = await fixture(page);
  await page.goto(`${BASE}/?${f.query}`);
  await expect(
    page.getByText("620 of 620 recorded events loaded"),
  ).toBeVisible();
  await page
    .getByRole("button", { name: "Event 2: PostToolBatch", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Event 2 · PostToolBatch" }),
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: "Event 2 · PostToolBatch" })).toBeInViewport();
  await expect(page.getByRole("region", { name: "Selected event details" })).toBeInViewport({ ratio: 0.2 });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  const audit = await new AxeBuilder({ page }).analyze();
  expect(
    audit.violations.filter(
      (v) => v.impact === "serious" || v.impact === "critical",
    ),
  ).toEqual([]);
});
