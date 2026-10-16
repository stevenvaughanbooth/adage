// Adage for OpenCode — agent traces become reflexes.
//
// A thin bridge: each documented plugin hook below spawns the adage binary (no shell) with a small
// JSON payload on stdin and reads back {"context": text} or {"continue": reason}. The text is
// rendered by adage from fixed templates; this file only places it. It never runs it.
//
// Fail open, always: a missing binary, a spawn error, a non-JSON reply, or no reply within 1 s
// means nothing happens. Install with `adage install opencode`, which fills in ADAGE_BIN.
// Hooks: https://opencode.ai/docs/plugins · context injection: https://opencode.ai/docs/sdk

import { spawn } from "node:child_process"

const ADAGE_BIN = "__ADAGE_BIN__"
const TIMEOUT_MS = 1000
const MAX_REPLY = 64 * 1024
const MAX_FIELD = 256 * 1024
const MAX_TEXT = 10000

function ask(event, payload) {
  return new Promise((resolve) => {
    let done = false
    let out = ""
    let child
    const finish = (v) => {
      if (done) return
      done = true
      clearTimeout(timer)
      if (v === null) try { child && child.kill("SIGKILL") } catch {}
      resolve(v)
    }
    const timer = setTimeout(() => finish(null), TIMEOUT_MS)
    try {
      child = spawn(ADAGE_BIN, ["hook", "opencode", event], { stdio: ["pipe", "pipe", "ignore"], shell: false })
      child.on("error", () => finish(null))
      child.stdin.on("error", () => {})
      child.stdout.on("data", (d) => {
        out += d
        if (out.length > MAX_REPLY) finish(null)
      })
      child.on("close", () => {
        try {
          const r = out.trim() ? JSON.parse(out) : null
          finish(r && typeof r === "object" ? r : null)
        } catch {
          finish(null)
        }
      })
      child.stdin.end(JSON.stringify(payload))
    } catch {
      finish(null)
    }
  })
}

const str = (v) => (typeof v === "string" && v.length > 0 && v.length <= MAX_TEXT ? v : null)
const clip = (v) => String(v ?? "").slice(0, MAX_FIELD)
const within = (p) => Promise.race([p, new Promise((r) => setTimeout(r, TIMEOUT_MS))]).catch(() => {})

export const Adage = async ({ client, directory }) => {
  const queued = new Map() // sessionID → directive texts waiting for the next tool result
  const continued = new Set() // sessions Adage asked to continue at their last idle
  const base = (sid) => ({ session_id: String(sid ?? ""), cwd: directory })

  return {
    // The user's prompt. Directives go in as a separate context-only message (noReply), marked
    // synthetic, never by editing what the user wrote.
    "chat.message": async (input, output) => {
      try {
        const sid = input.sessionID
        const prompt = (output.parts || [])
          .filter((p) => p && p.type === "text" && !p.synthetic && typeof p.text === "string")
          .map((p) => p.text)
          .join("\n")
        if (!prompt) return
        continued.delete(sid)
        const text = str((await ask("chat.message", { ...base(sid), prompt: clip(prompt) }))?.context)
        if (text) {
          await within(client.session.prompt({ path: { id: sid }, body: { noReply: true, parts: [{ type: "text", text, synthetic: true }] } }))
        }
      } catch {}
    },

    // Every tool result. Directives are appended to the result the model is about to read.
    "tool.execute.after": async (input, output) => {
      try {
        const sid = input.sessionID
        const exit = typeof output?.metadata?.exit === "number" ? output.metadata.exit : null
        const r = await ask("tool.execute.after", { ...base(sid), tool: String(input.tool), args: input.args ?? null, output: clip(output.output), exit })
        const texts = queued.get(sid) || []
        queued.delete(sid)
        const text = str(r?.context)
        if (text) texts.push(text)
        if (texts.length && typeof output.output === "string") output.output += "\n\n" + texts.join("\n\n")
      } catch {}
    },

    event: async ({ event }) => {
      try {
        const p = event?.properties || {}
        switch (event?.type) {
          case "session.created":
            await ask("session.created", base(p.info?.id))
            break
          case "session.deleted":
            queued.delete(p.info?.id)
            continued.delete(p.info?.id)
            await ask("session.deleted", base(p.info?.id))
            break
          case "message.part.updated": {
            // A tool that threw: no tool.execute.after follows, so hold the directive for the next result.
            const part = p.part
            if (part?.type !== "tool" || part.state?.status !== "error") break
            const text = str((await ask("tool.error", { ...base(part.sessionID), tool: String(part.tool), args: part.state.input ?? null, error: clip(part.state.error) }))?.context)
            if (text) queued.set(part.sessionID, [...(queued.get(part.sessionID) || []), text].slice(-3))
            break
          }
          case "session.idle": {
            // The finish line: at most once per obligation and twice per session (adage enforces
            // both). The reason says plainly that it comes from Adage, not the user.
            const sid = p.sessionID
            queued.delete(sid)
            const reason = str((await ask("session.idle", { ...base(sid), stop_hook_active: continued.has(sid) }))?.continue)
            if (!reason) break
            continued.add(sid)
            client.session.prompt({ path: { id: sid }, body: { parts: [{ type: "text", text: reason, synthetic: true }] } }).catch(() => {})
            break
          }
        }
      } catch {}
    },
  }
}
