// Exercises plugin/opencode/adage.js with a fake OpenCode client, against the real adage binary and
// against broken ones. Run by `cargo test -p adage-cli --test hooks` (skipped if node is absent),
// which generates each plugin variant with `adage install opencode --print --bin <path>`.
//
// Env: PLUGIN_REAL, PLUGIN_MISSING, PLUGIN_SLOW, PLUGIN_GARBAGE, PLUGIN_BADTYPE, PLUGIN_HUGE,
// PLUGIN_RECORD (plugin files); RECORD_FILE (where the recording binary writes); REPO (cwd).

import { pathToFileURL } from "node:url"
import assert from "node:assert/strict"
import { existsSync, readFileSync } from "node:fs"

const repo = process.env.REPO

function fakeClient() {
  const calls = []
  return { calls, session: { prompt: async (req) => { calls.push(req); return {} } } }
}

async function load(env) {
  const client = fakeClient()
  const mod = await import(pathToFileURL(process.env[env]).href)
  const hooks = await mod.Adage({ client, directory: repo, worktree: repo, project: {}, $: null })
  return { client, hooks }
}

const userMessage = (text, sid = "ses_js") => [{ sessionID: sid }, { message: {}, parts: [{ type: "text", text }] }]

async function real() {
  const { client, hooks } = await load("PLUGIN_REAL")
  await hooks.event({ event: { type: "session.created", properties: { info: { id: "ses_js" } } } })
  await hooks["chat.message"](...userMessage("Pick up W-2042 please"))
  assert.equal(client.calls.length, 1, "one context message for the prompt")
  const ctx = client.calls[0]
  assert.equal(ctx.path.id, "ses_js")
  assert.equal(ctx.body.noReply, true, "context only: no model reply")
  assert.equal(ctx.body.parts[0].synthetic, true)
  assert.match(ctx.body.parts[0].text, /^<adage-signal [^\n]*route="lookup"/)
  assert.match(ctx.body.parts[0].text, /wi-lookup W-2042 --json/)

  // Directives ride on the tool result the model reads next; the original output is kept.
  const out = { title: "", output: '{"linked_files":["acme/billing.py"]}', metadata: { exit: 0 } }
  await hooks["tool.execute.after"]({ tool: "bash", sessionID: "ses_js", callID: "c1", args: { command: "./scripts/wi-lookup W-2042 --json" } }, out)
  assert.ok(out.output.startsWith('{"linked_files":["acme/billing.py"]}\n\n<adage-signal'), out.output)
  assert.match(out.output, /route="read-linked"/)

  // A tool that threw: held, then delivered with the next tool result.
  await hooks.event({ event: { type: "message.part.updated", properties: { part: { type: "tool", sessionID: "ses_js", tool: "bash", state: { status: "error", input: { command: "python3 -m pytest tests" }, error: "ACME_ENV is not set" } } } } })
  const edit = { title: "", output: "Success", metadata: {} }
  await hooks["tool.execute.after"]({ tool: "apply_patch", sessionID: "ses_js", callID: "c2", args: { patchText: "*** Begin Patch\n*** Update File: schema/a.json\n*** End Patch" } }, edit)
  assert.match(edit.output, /route="dead-pytest"/)
  assert.match(edit.output, /route="gen"/)

  // The finish line at idle: once, as a synthetic message, then never again for that obligation.
  await hooks.event({ event: { type: "session.idle", properties: { sessionID: "ses_js" } } })
  await new Promise((r) => setTimeout(r, 50))
  assert.equal(client.calls.length, 2)
  assert.equal(client.calls[1].body.noReply, undefined)
  assert.match(client.calls[1].body.parts[0].text, /^Adage finish-line check:/)
  assert.equal(client.calls[1].body.parts[0].synthetic, true)
  // That continuation comes back through chat.message as a synthetic part: not a prompt.
  await hooks["chat.message"]({ sessionID: "ses_js" }, { message: {}, parts: [{ type: "text", text: client.calls[1].body.parts[0].text, synthetic: true }] })
  await hooks.event({ event: { type: "session.idle", properties: { sessionID: "ses_js" } } })
  await new Promise((r) => setTimeout(r, 50))
  assert.equal(client.calls.length, 2, "stop_hook_active: no second continuation")
}

async function failsOpen(env, label) {
  const t0 = Date.now()
  const { client, hooks } = await load(env)
  await hooks["chat.message"](...userMessage("Pick up W-2042 please"))
  const out = { title: "", output: "original", metadata: { exit: 1 } }
  await hooks["tool.execute.after"]({ tool: "bash", sessionID: "ses_js", callID: "c", args: { command: "python3 -m pytest" } }, out)
  await hooks.event({ event: { type: "session.idle", properties: { sessionID: "ses_js" } } })
  await hooks.event({ event: null })
  await hooks["chat.message"]({}, {})
  await hooks["tool.execute.after"]({}, {})
  assert.equal(out.output, "original", `${label}: tool output untouched`)
  assert.equal(client.calls.length, 0, `${label}: nothing injected`)
  return Date.now() - t0
}

async function synthetic() {
  const { hooks } = await load("PLUGIN_RECORD")
  await hooks["chat.message"]({ sessionID: "s" }, { message: {}, parts: [{ type: "text", text: "<adage-signal x>", synthetic: true }] })
  assert.ok(!existsSync(process.env.RECORD_FILE), "synthetic parts are never sent as prompts")
  await hooks["chat.message"]({ sessionID: "s" }, { message: {}, parts: [{ type: "text", text: "hello" }, { type: "file", url: "x" }] })
  const sent = JSON.parse(readFileSync(process.env.RECORD_FILE, "utf8"))
  assert.deepEqual(sent, { session_id: "s", cwd: repo, prompt: "hello" })
}

await real()
for (const env of ["PLUGIN_MISSING", "PLUGIN_GARBAGE", "PLUGIN_BADTYPE", "PLUGIN_HUGE"]) await failsOpen(env, env)
const slow = await failsOpen("PLUGIN_SLOW", "slow binary")
// Four awaited hook calls, each abandoned at the 1 s deadline.
assert.ok(slow < 4 * 1000 + 1500, `slow binary abandoned at the deadline (${slow} ms)`)
await synthetic()
console.log("plugin ok")
