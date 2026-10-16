/**
 * Claude Code history importer.
 *
 * `~/.claude/projects/<project>/<session-id>.jsonl`, where <project> is the cwd with
 * non-alphanumerics replaced by `-`. Subagent transcripts live in a `<session-id>/subagents/`
 * subdirectory — note this contradicts the design spec, which says subagents carry no parent
 * link. The directory nesting *is* the parent link.
 *
 * Streaming and line-oriented: never loads a whole file, so a 200MB transcript costs little.
 */

import { createReadStream } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { createInterface } from "node:readline";
import { join } from "node:path";
import { homedir } from "node:os";

import type { Record_, Session } from "../types.ts";
import { normalizeTool } from "../normalize/tools.ts";

export const CLAUDE_ROOT = join(homedir(), ".claude", "projects");

export interface DiscoveredSession {
  id: string;
  project: string;
  file: string;
  isSidechain: boolean;
  bytes: number;
}

/** Find every session transcript, including subagent sidechains. */
export async function discover(root = CLAUDE_ROOT): Promise<DiscoveredSession[]> {
  const out: DiscoveredSession[] = [];

  const walk = async (dir: string, project: string, sidechain: boolean) => {
    let entries;
    try { entries = await readdir(dir, { withFileTypes: true }); } catch { return; }

    for (const e of entries) {
      const p = join(dir, e.name);
      if (e.isDirectory()) {
        await walk(p, project, sidechain || e.name === "subagents");
      } else if (e.name.endsWith(".jsonl")) {
        const s = await stat(p).catch(() => null);
        out.push({
          id: e.name.replace(/\.jsonl$/, ""),
          project,
          file: p,
          isSidechain: sidechain,
          bytes: s?.size ?? 0,
        });
      }
    }
  };

  let projects;
  try { projects = await readdir(root, { withFileTypes: true }); } catch { return out; }
  for (const p of projects) {
    if (p.isDirectory()) await walk(join(root, p.name), p.name, false);
  }
  return out;
}

/** Project directory name → original cwd (lossy: `-` could have been `/`, `_` or `.`). */
export function projectToCwd(project: string): string {
  return project.replace(/^-/, "/").replace(/-/g, "/");
}

export interface ImportStats {
  lines: number;
  parsed: number;
  quarantined: number;
  prompts: number;
  toolCalls: number;
  unknownTools: number;
}

/** Read one transcript into records, normalizing every tool call. */
export async function readSession(
  d: DiscoveredSession,
  stats: ImportStats,
): Promise<Session> {
  const cwd = projectToCwd(d.project);
  const records: Record_[] = [];
  let seq = 0;
  let startedAt = 0;

  const rl = createInterface({
    input: createReadStream(d.file, { encoding: "utf8" }),
    crlfDelay: Infinity,
  });

  for await (const line of rl) {
    if (!line.trim()) continue;
    stats.lines++;

    let rec: any;
    try { rec = JSON.parse(line); stats.parsed++; }
    catch { stats.quarantined++; continue; }

    const ts = Date.parse(rec.timestamp ?? "") || 0;
    if (ts && !startedAt) startedAt = ts;
    const recCwd = rec.cwd || cwd;

    // User prompt. `last-prompt` records carry the text but no timestamp or cwd, so the
    // real source is `user` records with string content. Those also carry synthetic
    // messages (command output, caveats, meta) which are not user intent — filter them.
    if (rec.type === "user" && typeof rec.message?.content === "string") {
      const text: string = rec.message.content;
      const synthetic =
        rec.isMeta === true ||
        /^<(local-command|command-|system-reminder)/.test(text.trimStart());
      if (!synthetic && text.trim()) {
        records.push({
          sessionId: d.id, seq: seq++, kind: "prompt", ts, cwd: recCwd,
          text, isSidechain: rec.isSidechain ?? d.isSidechain,
        });
        stats.prompts++;
      }
      continue;
    }

    // assistant tool calls
    if (rec.type === "assistant") {
      const content = rec.message?.content;
      if (!Array.isArray(content)) continue;
      for (const b of content) {
        if (b?.type !== "tool_use") continue;
        const normalized = normalizeTool(b.name, b.input, recCwd);
        stats.toolCalls++;
        if (normalized.some((n) => n.action.op === "unknown")) stats.unknownTools++;
        for (const n of normalized) {
          records.push({
            sessionId: d.id, seq: seq++, kind: "tool_call", ts, cwd: recCwd,
            tool: b.name, input: b.input, normalized: n,
            isSidechain: d.isSidechain,
          });
        }
      }
    }
  }

  return { id: d.id, project: d.project, cwd, records, startedAt };
}
