// The chapters of "How it works", in the order Adage does the work.

import type { Chapter } from "../types";
import { choose } from "./choose";
import { hook } from "./hook";
import { intent } from "./intent";
import { measure } from "./measure";
import { miners } from "./miners";
import { readTraces } from "./read";
import { seal } from "./seal";
import { subgraphs } from "./subgraphs";
import { train } from "./train";

export const CHAPTERS: Chapter[] = [readTraces, miners, intent, subgraphs, choose, train, seal, hook, measure];
