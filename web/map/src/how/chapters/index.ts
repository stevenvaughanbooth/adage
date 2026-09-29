// The chapters of "How it works", in the order Adage does the work.

import type { Chapter } from "../types";
import { choose } from "./choose";
import { hook } from "./hook";
import { intent } from "./intent";
import { judge } from "./judge";
import { measure } from "./measure";
import { readTraces } from "./read";
import { seal } from "./seal";
import { subgraphs } from "./subgraphs";
import { train } from "./train";

export const CHAPTERS: Chapter[] = [readTraces, subgraphs, choose, judge, intent, train, seal, hook, measure];

/** Chapter ids that were renamed, so older links still land on their chapter. */
const RENAMED: Record<string, string> = { miners: "judge" };

/** The index of the chapter a link names (0 when it names none). */
export function chapterIndex(id: string | null | undefined): number {
  const want = id ? (RENAMED[id] ?? id) : null;
  return Math.max(0, CHAPTERS.findIndex((c) => c.id === want));
}
