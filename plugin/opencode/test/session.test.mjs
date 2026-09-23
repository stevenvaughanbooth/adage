// Drives plugin/opencode/adage.js through a whole OpenCode session with a fake client, against the
// real adage binary. The session is a script written by the Rust simulator
// (`adage_sim::opencode::plugin_script`): each step calls one plugin hook the way OpenCode would,
// and says what should follow — the route ids (by prefix) in the context message after the
// prompt, the route ids appended to each tool result, and whether `session.idle` asks to continue.
// Run by `cargo test -p adage-cli --test harness_sim` (skipped if node is absent).
//
// Env: PLUGIN (plugin file, from `adage install opencode --print`), SCRIPT (the JSON script),
// REPO (the session's directory; replaces "{REPO}" in the script).

import { pathToFileURL } from "node:url"
import assert from "node:assert/strict"
import { readFileSync } from "node:fs"

const repo = process.env.REPO
const script = JSON.parse(readFileSync(process.env.SCRIPT, "utf8"), (_, v) => (typeof v === "string" ? v.replaceAll("{REPO}", repo) : v))
const sid = script.session

const calls = []
const client = { session: { prompt: async (req) => { calls.push(req); return {} } } }
const mod = await import(pathToFileURL(process.env.PLUGIN).href)
const hooks = await mod.Adage({ client, directory: repo, worktree: repo, project: {}, $: null })

const routes = (text) => [...String(text).matchAll(/<adage-signal [^>]*route="([^"]+)"/g)].map((m) => m[1])
function covers(got, want, what) {
  assert.equal(got.length, want.length, `${what}: got ${JSON.stringify(got)}, want ${JSON.stringify(want)}`)
  for (const w of want) assert.ok(got.some((g) => g.startsWith(w)), `${what}: no ${w} in ${JSON.stringify(got)}`)
}
// session.idle sends its continuation fire-and-forget.
const settle = () => new Promise((r) => setTimeout(r, 50))

let n = 0
for (const step of script.steps) {
  n++
  const before = calls.length
  const what = `step ${n} (${step.hook} ${step.tool ?? step.event?.type ?? ""})`
  switch (step.hook) {
    case "chat.message": {
      await hooks["chat.message"](
        { sessionID: sid, agent: "build", model: { providerID: "anthropic", modelID: "claude-sonnet-4-6" }, messageID: `msg_${n}` },
        { message: {}, parts: [{ type: "text", text: step.text }] },
      )
      const made = calls.slice(before)
      if (step.expect.context.length) {
        assert.equal(made.length, 1, `${what}: one context message`)
        assert.equal(made[0].path.id, sid)
        assert.equal(made[0].body.noReply, true, `${what}: context only, no model reply`)
        assert.equal(made[0].body.parts[0].synthetic, true)
        covers(routes(made[0].body.parts[0].text), step.expect.context, what)
      } else {
        assert.equal(made.length, 0, `${what}: no context message`)
      }
      break
    }
    case "tool.execute.after": {
      const output = { title: step.tool, output: step.output, metadata: step.metadata }
      await hooks["tool.execute.after"]({ tool: step.tool, sessionID: sid, callID: step.callID, args: step.args }, output)
      assert.ok(output.output.startsWith(step.output), `${what}: the tool's own output is kept`)
      covers(routes(output.output.slice(step.output.length)), step.expect.appended, what)
      assert.equal(calls.length, before, `${what}: nothing sent to the session`)
      break
    }
    case "event": {
      await hooks.event({ event: step.event })
      await settle()
      const made = calls.slice(before)
      if (step.expect?.continue) {
        assert.equal(made.length, 1, `${what}: one continuation prompt`)
        const req = made[0]
        assert.equal(req.path.id, sid)
        assert.equal(req.body.noReply, undefined, `${what}: the continuation asks for a reply`)
        assert.equal(req.body.parts[0].synthetic, true)
        assert.match(req.body.parts[0].text, /^Adage finish-line check:/)
        // OpenCode delivers that prompt back through chat.message as a synthetic part: not a prompt.
        await hooks["chat.message"]({ sessionID: sid }, { message: {}, parts: req.body.parts })
        assert.equal(calls.length, before + 1, `${what}: the continuation is not treated as a prompt`)
      } else {
        assert.equal(made.length, 0, `${what}: nothing sent`)
      }
      break
    }
    default:
      assert.fail(`unknown step ${JSON.stringify(step)}`)
  }
}
console.log(`session ok: ${script.steps.length} steps, ${calls.length} messages sent`)
