/**
 * Window extraction and templating.
 *
 * The discovery window is the span from a trigger to the session's first write action.
 * That is where context gathering happens, and it is the only span a compiled path can
 * cover. Templating generalizes an action so instances can be counted together — the
 * PoC equivalent of anti-unification.
 */

import type { CanonicalAction, Session, Window } from "./types.ts";

/** Actions per window, matching the spec's cap. */
const MAX_WINDOW = 8;

export function extractWindows(session: Session): Window[] {
  const out: Window[] = [];
  const recs = session.records;

  for (let i = 0; i < recs.length; i++) {
    if (recs[i].kind !== "prompt") continue;

    const actions: CanonicalAction[] = [];
    const turnStamps = new Set<number>();
    let closedByWrite = false;

    for (let j = i + 1; j < recs.length && actions.length < MAX_WINDOW; j++) {
      const r = recs[j];
      if (r.kind === "prompt") break;          // next trigger ends this window
      if (r.kind !== "tool_call" || !r.normalized) continue;

      if (r.normalized.mutating) { closedByWrite = true; break; }

      actions.push(r.normalized.action);
      turnStamps.add(r.ts);
    }

    if (actions.length) {
      out.push({
        sessionId: session.id,
        project: session.project,
        promptSeq: recs[i].seq,
        prompt: recs[i].text ?? "",
        actions,
        turns: turnStamps.size,
        closedByWrite,
      });
    }
  }
  return out;
}

/* --------------------------------------------------------------- templating */

/** Tokens that look like values rather than structure, replaced by a typed placeholder. */
function generalizeToken(t: string): string {
  if (/^\/.*/.test(t) || t.includes("/")) return "{path}";
  if (/^[0-9a-f]{7,40}$/i.test(t)) return "{sha}";
  if (/^\d+$/.test(t)) return "{int}";
  if (/^[A-Z]+-\d+$/.test(t)) return "{id}";
  if (/^https?:\/\//.test(t)) return "{url}";
  if (/\d{3,}/.test(t)) return "{id}";
  return t;
}

/**
 * A stable string key for an action, at one of two levels.
 *  - "exact":    the literal action, arguments and all
 *  - "template": arguments generalized to placeholders
 */
export function actionKey(a: CanonicalAction, level: "exact" | "template"): string {
  const g = (s: string) => (level === "template" ? generalizeToken(s) : s);

  switch (a.op) {
    case "read":
      return `read ${g(a.path)}`;
    case "search":
      return `search:${a.kind} ${g(a.pattern)} ${a.paths.map(g).join(",")}`;
    case "list":
      return `list ${g(a.path)}`;
    case "exec": {
      const argv = level === "template"
        ? [a.argv[0] ?? "", ...a.argv.slice(1).map(generalizeToken)]
        : a.argv;
      return `exec[${a.class}] ${argv.join(" ")}`;
    }
    case "edit":
    case "write":
      return `${a.op} ${g(a.path)}`;
    case "patch":
      return `patch ${a.paths.map(g).join(",")}`;
    case "fetch":
      return `fetch ${g(a.url)}`;
    case "mcp":
      return `mcp ${a.server}/${a.tool}`;
    case "task":
      return `task ${a.agent}`;
    case "skill":
      return `skill ${a.name}`;
    case "unknown":
      return `unknown ${a.raw}`;
  }
}
