/**
 * Claude Code native tool call → canonical action.
 *
 * Native names drift between versions, so in production this table is data (one JSON file
 * per harness version), not code. Kept inline for the PoC.
 */

import type { Normalized } from "../types.ts";
import { normalizeShell, resolvePath } from "./shell.ts";

type Input = Record<string, any>;

export function normalizeTool(tool: string, input: unknown, cwd: string): Normalized[] {
  const i = (input ?? {}) as Input;

  switch (tool) {
    case "Read": {
      const path = resolvePath(cwd, i.file_path ?? "");
      const range: [number, number] | undefined =
        i.offset != null || i.limit != null
          ? [Number(i.offset ?? 1), Number(i.offset ?? 1) + Number(i.limit ?? 2000) - 1]
          : undefined;
      return [{ action: { op: "read", path, ...(range ? { range } : {}) }, mutating: false, confidence: 1 }];
    }

    case "Grep":
      return [{
        action: {
          op: "search", kind: "content",
          pattern: i.pattern ?? "",
          paths: [resolvePath(cwd, i.path ?? ".")],
        },
        mutating: false, confidence: 1,
      }];

    case "Glob":
      return [{
        action: {
          op: "search", kind: "filename",
          pattern: i.pattern ?? "*",
          paths: [resolvePath(cwd, i.path ?? ".")],
        },
        mutating: false, confidence: 1,
      }];

    case "LS":
      return [{ action: { op: "list", path: resolvePath(cwd, i.path ?? ".") }, mutating: false, confidence: 1 }];

    case "Bash":
      return normalizeShell(String(i.command ?? ""), cwd);

    case "Edit":
    case "MultiEdit":
      return [{ action: { op: "edit", path: resolvePath(cwd, i.file_path ?? "") }, mutating: true, confidence: 1 }];

    case "Write":
      return [{ action: { op: "write", path: resolvePath(cwd, i.file_path ?? "") }, mutating: true, confidence: 1 }];

    case "NotebookEdit":
      return [{ action: { op: "edit", path: resolvePath(cwd, i.notebook_path ?? "") }, mutating: true, confidence: 1 }];

    case "WebFetch":
      return [{ action: { op: "fetch", url: String(i.url ?? "") }, mutating: false, confidence: 1 }];

    case "WebSearch":
      return [{ action: { op: "fetch", url: `search:${i.query ?? ""}` }, mutating: false, confidence: 1 }];

    case "Task":
    case "Agent":
      return [{ action: { op: "task", agent: String(i.subagent_type ?? "general") }, mutating: false, confidence: 1 }];

    case "Skill":
      return [{ action: { op: "skill", name: String(i.skill ?? "") }, mutating: false, confidence: 1 }];

    default: {
      const m = tool.match(/^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/);
      if (m) return [{ action: { op: "mcp", server: m[1], tool: m[2] }, mutating: false, confidence: 0.8 }];
      return [{ action: { op: "unknown", raw: tool }, mutating: false, confidence: 0 }];
    }
  }
}
