/**
 * Canonical action vocabulary — the normalization target.
 *
 * Every harness's native tool call, including raw shell, maps onto this. Cross-harness
 * mining depends on it entirely: a Codex `sed -n '1,200p' a.ts` and a Claude Code
 * `Read a.ts` must become the same action or paths never transfer.
 */

export type ExecClass =
  | "test" | "build" | "lint" | "typecheck"
  | "vcs-read" | "vcs-write"
  | "lookup" | "install" | "mutating" | "other";

export type CanonicalAction =
  | { op: "read"; path: string; range?: [number, number] }
  | { op: "search"; kind: "content" | "filename"; pattern: string; paths: string[] }
  | { op: "list"; path: string; depth?: number }
  | { op: "exec"; argv: string[]; cwd: string; class: ExecClass }
  | { op: "edit" | "write"; path: string }
  | { op: "patch"; paths: string[] }
  | { op: "fetch"; url: string }
  | { op: "mcp"; server: string; tool: string }
  | { op: "task"; agent: string }
  | { op: "skill"; name: string }
  | { op: "unknown"; raw: string };

export interface Normalized {
  action: CanonicalAction;
  /** True if this action closes the discovery window. */
  mutating: boolean;
  /** 0..1 — how confident the normalizer is it understood the call. */
  confidence: number;
}

/** One record from a session transcript, harness-agnostic. */
export interface Record_ {
  sessionId: string;
  seq: number;
  kind: "prompt" | "tool_call" | "tool_result";
  ts: number;
  cwd: string;
  text?: string;                 // prompt text
  tool?: string;                 // native tool name
  input?: unknown;               // native tool input
  normalized?: Normalized;
  isSidechain: boolean;
}

export interface Session {
  id: string;
  project: string;
  cwd: string;
  records: Record_[];
  startedAt: number;
}

/** A trigger and the context-gathering that followed it, before the first write. */
export interface Window {
  sessionId: string;
  project: string;
  promptSeq: number;
  prompt: string;
  actions: CanonicalAction[];
  /** Distinct assistant turns the actions occupied — savings are counted in turns. */
  turns: number;
  /** True if the window was closed by a write rather than by session end. */
  closedByWrite: boolean;
}
